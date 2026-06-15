//! Browser-side OIDC Authorization Code login (without PKCE).
//!
//! The provider does not support PKCE, and a public browser client must never hold the client
//! secret, so the agent performs the confidential code exchange on the SPA's behalf: the browser
//! runs the authorization redirect, then POSTs the resulting `code` to the agent, which exchanges it
//! (using its server-held secret) and returns the ID token. The SPA stores that token in
//! `sessionStorage` and sends it as an `Authorization: Bearer` header (see [`crate::api`]).
//!
//! Everything the browser calls is same-origin (the agent), so the provider never needs to permit
//! cross-origin requests. Browser-only; the SSR build gets inert stubs.

use grey_api::UiAuthConfig;

#[cfg(feature = "wasm")]
mod browser {
    use super::UiAuthConfig;
    use base64::prelude::*;
    use web_sys::window;

    const TOKEN_KEY: &str = "grey.admin.token";
    const STATE_KEY: &str = "grey.oidc.state";
    const RETURN_KEY: &str = "grey.oidc.return_to";
    /// The OAuth redirect lands back at the app root; the SPA detects `?code&state` on load.
    const CALLBACK_PATH: &str = "/";

    fn session() -> Option<web_sys::Storage> {
        window()?.session_storage().ok().flatten()
    }

    pub fn stored_token() -> Option<String> {
        session()?.get_item(TOKEN_KEY).ok().flatten()
    }

    fn store_token(token: &str) {
        if let Some(storage) = session() {
            let _ = storage.set_item(TOKEN_KEY, token);
        }
    }

    pub fn clear_token() {
        if let Some(storage) = session() {
            let _ = storage.remove_item(TOKEN_KEY);
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

    fn callback_params() -> Option<(String, String)> {
        let search = window()?.location().search().ok()?;
        let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
        Some((params.get("code")?, params.get("state")?))
    }

    pub fn has_pending_callback() -> bool {
        callback_params().is_some()
    }

    /// Begins login: stash a CSRF `state`, then navigate to the provider's authorization endpoint.
    pub async fn begin_login(config: &UiAuthConfig) {
        let state = random_token(24);

        if let Some(storage) = session() {
            let _ = storage.set_item(STATE_KEY, &state);
            if let Some(path) = window().and_then(|w| w.location().pathname().ok()) {
                let _ = storage.set_item(RETURN_KEY, &path);
            }
        }

        let authorize = match authorization_endpoint().await {
            Ok(url) => url,
            Err(err) => {
                gloo::console::error!(format!("Failed to start sign-in: {err}"));
                return;
            }
        };

        let mut scopes = vec!["openid".to_string()];
        scopes.extend(config.scopes.iter().filter(|s| *s != "openid").cloned());
        let scope = scopes.join(" ");

        let url = format!(
            "{authorize}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            enc(&config.client_id),
            enc(&redirect_uri()),
            enc(&scope),
            enc(&state),
        );

        if let Some(w) = window() {
            let _ = w.location().set_href(&url);
        }
    }

    /// If the current URL is an OIDC callback, hand the code to the agent to exchange and store the
    /// returned token. Returns the token on success, `None` when there is no callback to process.
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

        // The agent exchanges the code with its client secret and returns the token.
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

        store_token(&token);

        // Clear the one-time state and scrub the code/state from the address bar.
        let _ = storage.remove_item(STATE_KEY);
        let return_to = storage
            .get_item(RETURN_KEY)
            .ok()
            .flatten()
            .unwrap_or_else(|| "/".to_string());
        let _ = storage.remove_item(RETURN_KEY);

        if let Some(history) = window().and_then(|w| w.history().ok()) {
            let _ =
                history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&return_to));
        }

        Ok(Some(token))
    }
}

#[cfg(feature = "wasm")]
pub use browser::{begin_login, clear_token, complete_callback, has_pending_callback, stored_token};

// Inert SSR stubs so the shared component tree compiles without the browser-only dependencies.
#[cfg(not(feature = "wasm"))]
mod stub {
    use super::UiAuthConfig;

    pub fn stored_token() -> Option<String> {
        None
    }
    pub fn clear_token() {}
    pub fn has_pending_callback() -> bool {
        false
    }
    pub async fn begin_login(_config: &UiAuthConfig) {}
    pub async fn complete_callback(_config: &UiAuthConfig) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[cfg(not(feature = "wasm"))]
pub use stub::{begin_login, clear_token, complete_callback, has_pending_callback, stored_token};
