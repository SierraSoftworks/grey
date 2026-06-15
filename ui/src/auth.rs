//! Browser-side OIDC Authorization Code + PKCE login.
//!
//! The SPA is a public OIDC client: it runs the whole PKCE flow itself, stores the resulting ID
//! token in `sessionStorage`, and sends it as an `Authorization: Bearer` header on admin requests
//! (see [`crate::api`]). The agent only *validates* the token. Everything here is browser-only; the
//! SSR build gets inert stubs so the shared component tree still compiles.

use grey_api::UiAuthConfig;

#[cfg(feature = "wasm")]
mod browser {
    use super::UiAuthConfig;
    use base64::prelude::*;
    use sha2::{Digest, Sha256};
    use web_sys::window;

    const TOKEN_KEY: &str = "grey.admin.token";
    const VERIFIER_KEY: &str = "grey.oidc.verifier";
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

    fn code_challenge(verifier: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        BASE64_URL_SAFE_NO_PAD.encode(hasher.finalize())
    }

    fn origin() -> String {
        window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_default()
    }

    fn redirect_uri() -> String {
        format!("{}{CALLBACK_PATH}", origin())
    }

    /// Fetches `(authorization_endpoint, token_endpoint)` from the provider's discovery document.
    async fn endpoints(issuer: &str) -> Result<(String, String), String> {
        let url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        let response = gloo::net::http::Request::get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let doc: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        let authorize = doc["authorization_endpoint"]
            .as_str()
            .ok_or("discovery document is missing authorization_endpoint")?
            .to_string();
        let token = doc["token_endpoint"]
            .as_str()
            .ok_or("discovery document is missing token_endpoint")?
            .to_string();
        Ok((authorize, token))
    }

    fn callback_params() -> Option<(String, String)> {
        let search = window()?.location().search().ok()?;
        let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
        Some((params.get("code")?, params.get("state")?))
    }

    pub fn has_pending_callback() -> bool {
        callback_params().is_some()
    }

    /// Begins login: generate PKCE material, stash it, and navigate to the provider.
    pub async fn begin_login(config: &UiAuthConfig) {
        let verifier = random_token(48);
        let state = random_token(24);
        let challenge = code_challenge(&verifier);

        if let Some(storage) = session() {
            let _ = storage.set_item(VERIFIER_KEY, &verifier);
            let _ = storage.set_item(STATE_KEY, &state);
            if let Some(path) = window().and_then(|w| w.location().pathname().ok()) {
                let _ = storage.set_item(RETURN_KEY, &path);
            }
        }

        let (authorize, _token) = match endpoints(&config.issuer).await {
            Ok(endpoints) => endpoints,
            Err(err) => {
                gloo::console::error!(format!("Failed to start OIDC login: {err}"));
                return;
            }
        };

        let mut scopes = vec!["openid".to_string()];
        scopes.extend(config.scopes.iter().filter(|s| *s != "openid").cloned());
        let scope = scopes.join(" ");

        let url = format!(
            "{authorize}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            enc(&config.client_id),
            enc(&redirect_uri()),
            enc(&scope),
            enc(&state),
            enc(&challenge),
        );

        if let Some(w) = window() {
            let _ = w.location().set_href(&url);
        }
    }

    /// If the current URL is an OIDC callback, exchange the code for a token and store it. Returns
    /// the token on success, `None` when there is no callback to process.
    pub async fn complete_callback(config: &UiAuthConfig) -> Result<Option<String>, String> {
        let Some((code, state)) = callback_params() else {
            return Ok(None);
        };
        let storage = session().ok_or("session storage is unavailable")?;

        let expected_state = storage.get_item(STATE_KEY).ok().flatten();
        if expected_state.as_deref() != Some(state.as_str()) {
            return Err("the login response state did not match (possible CSRF or stale login)".into());
        }
        let verifier = storage
            .get_item(VERIFIER_KEY)
            .ok()
            .flatten()
            .ok_or("the PKCE verifier is missing; please sign in again")?;

        let (_authorize, token_endpoint) = endpoints(&config.issuer).await?;

        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            enc(&code),
            enc(&redirect_uri()),
            enc(&config.client_id),
            enc(&verifier),
        );

        let response = gloo::net::http::Request::post(&token_endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.ok() {
            return Err(format!(
                "the identity provider rejected the token exchange (HTTP {})",
                response.status()
            ));
        }

        let tokens: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        let id_token = tokens["id_token"]
            .as_str()
            .ok_or("the token response did not include an id_token")?
            .to_string();

        store_token(&id_token);

        // Clear the one-time PKCE material and scrub the code/state from the address bar.
        let _ = storage.remove_item(VERIFIER_KEY);
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

        Ok(Some(id_token))
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
