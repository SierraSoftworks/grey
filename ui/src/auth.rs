//! Browser-side OIDC Authorization Code login (without PKCE), via a popup.
//!
//! The provider does not support PKCE, and a public browser client must never hold the client
//! secret, so the agent performs the confidential code exchange on the SPA's behalf: the browser
//! runs the authorization request in a popup, the popup POSTs the resulting `code` to the agent,
//! which exchanges it (using its server-held secret) and returns an ID token plus a refresh token.
//!
//! Sign-in uses a popup so the main page is never navigated away from: [`begin_login`] opens the
//! authorization URL in a popup and waits for it to report success. The popup loads the SPA at the
//! callback URL, [`complete_callback`] exchanges the code and hands the tokens back to the opener
//! through a short-lived `localStorage` slot (popups don't share `sessionStorage` with their
//! opener), then closes. The opener stores the tokens in `sessionStorage` and sends the ID token as
//! an `Authorization: Bearer` header (see [`crate::api`]). The refresh token lets [`refresh_session`]
//! renew an expired ID token without another interactive login.
//!
//! Everything the browser calls is same-origin (the agent), so the provider never needs to permit
//! cross-origin requests. Browser-only; the SSR build gets inert stubs.

// Consumed by the wasm `browser` implementation and by the non-SSR stubs; the SSR build references
// neither, so the import is excluded there to stay warning-free.
#[cfg(any(feature = "wasm", not(feature = "ssr")))]
use grey_api::UiAuthConfig;

#[cfg(feature = "wasm")]
mod browser {
    use super::UiAuthConfig;
    use base64::prelude::*;
    use futures::FutureExt;
    use futures::future::{LocalBoxFuture, Shared};
    use std::cell::{Cell, RefCell};
    use std::time::Duration;
    use web_sys::window;

    const TOKEN_KEY: &str = "grey.admin.token";
    const REFRESH_KEY: &str = "grey.admin.refresh";
    const STATE_KEY: &str = "grey.oidc.state";
    /// Short-lived `localStorage` slot the popup uses to hand tokens back to its opener.
    const POPUP_RESULT_KEY: &str = "grey.oidc.popup_result";
    /// The OAuth redirect lands on the dedicated callback route; the SPA detects `?code&state`
    /// there and finishes the exchange (see [`crate::views::AuthCallback`]).
    const CALLBACK_PATH: &str = "/auth/callback";
    /// How long the opener waits for the popup to complete before giving up.
    const POPUP_POLL_INTERVAL: Duration = Duration::from_millis(300);
    const POPUP_MAX_POLLS: u32 = 2_000; // ~10 minutes

    fn session() -> Option<web_sys::Storage> {
        window()?.session_storage().ok().flatten()
    }

    fn local() -> Option<web_sys::Storage> {
        window()?.local_storage().ok().flatten()
    }

    pub fn stored_token() -> Option<String> {
        session()?.get_item(TOKEN_KEY).ok().flatten()
    }

    fn stored_refresh_token() -> Option<String> {
        session()?.get_item(REFRESH_KEY).ok().flatten()
    }

    /// Persists the ID token, and the refresh token when one was issued (providers that don't rotate
    /// refresh tokens omit it, so we keep any one we already hold).
    fn store_tokens(token: &str, refresh: Option<&str>) {
        if let Some(storage) = session() {
            let _ = storage.set_item(TOKEN_KEY, token);
            if let Some(refresh) = refresh {
                let _ = storage.set_item(REFRESH_KEY, refresh);
            }
        }
    }

    pub fn clear_token() {
        if let Some(storage) = session() {
            let _ = storage.remove_item(TOKEN_KEY);
            let _ = storage.remove_item(REFRESH_KEY);
        }
    }

    /// RFC 3986 percent-encoding of a URL query-component value.
    fn enc(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for byte in value.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char)
                }
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    }

    fn random_token(bytes: usize) -> String {
        let mut buf = vec![0u8; bytes];
        if let Some(crypto) = window().and_then(|w| w.crypto().ok()) {
            let _ = crypto.get_random_values_with_u8_array(&mut buf);
        }
        BASE64_URL_SAFE_NO_PAD.encode(&buf)
    }

    fn origin() -> String {
        window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_default()
    }

    fn redirect_uri() -> String {
        format!("{}{CALLBACK_PATH}", origin())
    }

    /// Whether this window was opened as a login popup (it has an opener). Used to decide whether
    /// [`complete_callback`] should hand tokens back and close, or store them and stay.
    fn is_popup() -> bool {
        window()
            .and_then(|w| w.opener().ok())
            .is_some_and(|opener| !opener.is_null() && !opener.is_undefined())
    }

    /// Fetches the provider's authorization endpoint from the agent (same-origin), so the browser
    /// never has to read the provider's discovery document cross-origin.
    async fn authorization_endpoint() -> Result<String, String> {
        let response = gloo::net::http::Request::get("/api/v1/auth/metadata")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.ok() {
            return Err(format!(
                "the agent could not provide the login endpoint (HTTP {})",
                response.status()
            ));
        }
        let doc: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        doc["authorization_endpoint"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| "the agent did not return an authorization endpoint".to_string())
    }

    fn build_authorize_url(authorize: &str, config: &UiAuthConfig, state: &str) -> String {
        let mut scopes = vec!["openid".to_string()];
        scopes.extend(config.scopes.iter().filter(|s| *s != "openid").cloned());
        let scope = scopes.join(" ");
        format!(
            "{authorize}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            enc(&config.client_id),
            enc(&redirect_uri()),
            enc(&scope),
            enc(state),
        )
    }

    fn callback_params() -> Option<(String, String)> {
        let search = window()?.location().search().ok()?;
        let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
        Some((params.get("code")?, params.get("state")?))
    }

    pub fn has_pending_callback() -> bool {
        callback_params().is_some()
    }

    /// Begins an interactive sign-in: opens the provider's authorization URL in a popup and waits for
    /// it to report success, returning the new ID token. Returns `Ok(None)` if the popup is dismissed
    /// without completing. Must be called from a user gesture so the popup isn't blocked.
    pub async fn begin_login(config: &UiAuthConfig) -> Result<Option<String>, String> {
        let state = random_token(24);
        if let Some(storage) = session() {
            let _ = storage.set_item(STATE_KEY, &state);
        }
        // Clear any stale handoff from a previous attempt before opening the popup.
        if let Some(storage) = local() {
            let _ = storage.remove_item(POPUP_RESULT_KEY);
        }

        let authorize = authorization_endpoint().await?;
        let url = build_authorize_url(&authorize, config, &state);

        let window = window().ok_or("no window is available")?;
        let popup = window
            .open_with_url_and_target_and_features(&url, "grey-login", "popup,width=480,height=720")
            .map_err(|_| "the browser blocked the sign-in popup".to_string())?;

        let Some(popup) = popup else {
            return Err("the browser blocked the sign-in popup".to_string());
        };

        // Poll the handoff slot until the popup reports tokens or is closed.
        for _ in 0..POPUP_MAX_POLLS {
            if let Some(result) = local().and_then(|s| s.get_item(POPUP_RESULT_KEY).ok().flatten()) {
                if let Some(storage) = local() {
                    let _ = storage.remove_item(POPUP_RESULT_KEY);
                }
                let tokens: serde_json::Value =
                    serde_json::from_str(&result).map_err(|e| e.to_string())?;
                let token = tokens["token"]
                    .as_str()
                    .ok_or("the sign-in response did not include a token")?
                    .to_string();
                store_tokens(&token, tokens["refresh_token"].as_str());
                return Ok(Some(token));
            }
            if popup.closed().unwrap_or(false) {
                return Ok(None);
            }
            gloo::timers::future::sleep(POPUP_POLL_INTERVAL).await;
        }
        Err("the sign-in popup did not complete in time".to_string())
    }

    /// If the current URL is an OIDC callback, exchange the code for tokens. In a popup, the tokens
    /// are handed back to the opener and the popup closes (returning `None`); otherwise (a direct
    /// navigation) the tokens are stored and the new ID token is returned. `None` means there was no
    /// callback to process or the work was delegated to the opener.
    pub async fn complete_callback(_config: &UiAuthConfig) -> Result<Option<String>, String> {
        let Some((code, state)) = callback_params() else {
            return Ok(None);
        };
        let storage = session().ok_or("session storage is unavailable")?;

        let expected_state = storage.get_item(STATE_KEY).ok().flatten();
        if expected_state.as_deref() != Some(state.as_str()) {
            return Err(
                "the login response state did not match (possible CSRF or stale login)".into(),
            );
        }

        // The agent exchanges the code with its client secret and returns the tokens.
        let request = serde_json::json!({ "code": code, "redirect_uri": redirect_uri() }).to_string();
        let response = gloo::net::http::Request::post("/api/v1/auth/token")
            .header("Content-Type", "application/json")
            .body(request)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.ok() {
            return Err(format!(
                "the sign-in could not be completed (HTTP {})",
                response.status()
            ));
        }

        let tokens: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        let token = tokens["token"]
            .as_str()
            .ok_or("the sign-in response did not include a token")?
            .to_string();

        let _ = storage.remove_item(STATE_KEY);

        if is_popup() {
            // Hand the tokens back to the opener through localStorage, then close. The opener picks
            // them up in `begin_login` and stores them in its own sessionStorage.
            if let Some(local) = local() {
                let _ = local.set_item(POPUP_RESULT_KEY, &tokens.to_string());
            }
            if let Some(w) = window() {
                let _ = w.close();
            }
            return Ok(None);
        }

        // Direct navigation (popup was blocked and we fell back, or the user opened the link): store
        // the tokens and scrub the code/state from the address bar.
        store_tokens(&token, tokens["refresh_token"].as_str());
        if let Some(history) = window().and_then(|w| w.history().ok()) {
            let _ = history.replace_state_with_url(
                &wasm_bindgen::JsValue::NULL,
                "",
                Some(CALLBACK_PATH),
            );
        }

        Ok(Some(token))
    }

    thread_local! {
        /// The renewal currently in flight, shared by every caller that needs a fresh token while it
        /// runs, so a single refresh-token redemption serves them all (see [`refresh_session`]). The
        /// generation tags each attempt so a caller only retires the slot if it still holds the very
        /// attempt it started, never a newer one.
        static REFRESH_IN_FLIGHT: RefCell<
            Option<(u64, Shared<LocalBoxFuture<'static, Result<String, String>>>)>,
        > = const { RefCell::new(None) };
        static REFRESH_GEN: Cell<u64> = const { Cell::new(0) };
    }

    /// Renews the session from the stored refresh token, returning a fresh ID token.
    ///
    /// Concurrent callers are coalesced into a single renewal: the first caller starts the refresh and
    /// the rest await its result. This matters because providers may *rotate* the refresh token —
    /// redeeming it returns a new one and invalidates the old. The SPA polls several endpoints on
    /// independent loops (the public entities and the cluster peers), and those loops wake together
    /// (e.g. the catch-up fetch when a backgrounded tab regains focus), so a lapsed ID token makes
    /// them all hit a 401 and reach for a refresh at once. Were each to redeem the stored refresh
    /// token independently, only the first would succeed; the rest would present the now-rotated token,
    /// fail, drop the freshly minted session, and surface a spurious "session expired" error even
    /// though the session had just been renewed. Sharing one redemption avoids that race.
    pub async fn refresh_session() -> Result<String, String> {
        let (generation, shared) = REFRESH_IN_FLIGHT.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Some((generation, existing)) = slot.as_ref() {
                (*generation, existing.clone())
            } else {
                let generation = REFRESH_GEN.with(|g| {
                    let next = g.get().wrapping_add(1);
                    g.set(next);
                    next
                });
                let shared = do_refresh_session().boxed_local().shared();
                *slot = Some((generation, shared.clone()));
                (generation, shared)
            }
        });

        let result = shared.await;

        // Retire this attempt so the next lapse starts a fresh refresh, but only if no later attempt
        // has already replaced it — otherwise we'd strand the newer one and let a second concurrent
        // redemption start, reintroducing the rotation race.
        REFRESH_IN_FLIGHT.with(|cell| {
            let mut slot = cell.borrow_mut();
            if matches!(slot.as_ref(), Some((g, _)) if *g == generation) {
                *slot = None;
            }
        });

        result
    }

    /// Performs a single session renewal against the agent. The agent uses its server-held secret to
    /// perform the refresh, so the browser only supplies the refresh token. On failure the stored
    /// session is dropped so the UI can prompt for an interactive sign-in. Callers go through
    /// [`refresh_session`], which coalesces concurrent renewals onto one invocation of this.
    async fn do_refresh_session() -> Result<String, String> {
        let refresh = stored_refresh_token().ok_or("no refresh token is available")?;
        let request = serde_json::json!({ "refresh_token": refresh }).to_string();
        let response = gloo::net::http::Request::post("/api/v1/auth/refresh")
            .header("Content-Type", "application/json")
            .body(request)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.ok() {
            clear_token();
            return Err(format!(
                "the session could not be renewed (HTTP {})",
                response.status()
            ));
        }

        let tokens: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        let token = tokens["token"]
            .as_str()
            .ok_or("the refresh response did not include a token")?
            .to_string();
        store_tokens(&token, tokens["refresh_token"].as_str());
        Ok(token)
    }
}

#[cfg(feature = "wasm")]
pub use browser::{
    begin_login, clear_token, complete_callback, has_pending_callback, refresh_session, stored_token,
};

// Inert SSR stubs so the shared component tree compiles without the browser-only dependencies.
#[cfg(not(feature = "wasm"))]
mod stub {
    // Only the sign-out path runs during server rendering; every other entrypoint is reached solely
    // from `#[cfg(feature = "wasm")]` code, so those stubs are excluded from the SSR build (where they
    // would otherwise be flagged as dead code) via `not(feature = "ssr")`.
    #[cfg(not(feature = "ssr"))]
    use super::UiAuthConfig;

    pub fn clear_token() {}

    #[cfg(not(feature = "ssr"))]
    pub fn stored_token() -> Option<String> {
        None
    }
    #[cfg(not(feature = "ssr"))]
    pub fn has_pending_callback() -> bool {
        false
    }
    #[cfg(not(feature = "ssr"))]
    pub async fn begin_login(_config: &UiAuthConfig) -> Result<Option<String>, String> {
        Ok(None)
    }
    #[cfg(not(feature = "ssr"))]
    pub async fn complete_callback(_config: &UiAuthConfig) -> Result<Option<String>, String> {
        Ok(None)
    }
    #[cfg(not(feature = "ssr"))]
    pub async fn refresh_session() -> Result<String, String> {
        Err("session renewal is unavailable during server rendering".into())
    }
}

#[cfg(not(feature = "wasm"))]
pub use stub::clear_token;
#[cfg(all(not(feature = "wasm"), not(feature = "ssr")))]
pub use stub::{
    begin_login, complete_callback, has_pending_callback, refresh_session, stored_token,
};
