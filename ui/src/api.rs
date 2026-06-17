//! The SPA's single API client.
//!
//! [`ApiClient`] exposes one method per entity (`probes`, `notices`, `incidents`, `peers`, the
//! incident CRUD calls and `me`), so callers never hand-build requests. Every failure surfaces as a
//! [`grey_api::ApiError`], whose `code` mirrors the HTTP status so callers can classify problems.
//!
//! Authenticated calls attach the stored ID token as an `Authorization: Bearer` header. When the
//! agent rejects a token as expired (HTTP 401), the client transparently renews it from the stored
//! refresh token (see [`crate::auth`]) and retries the request once; interactive sign-in is handled
//! separately via a popup (see [`crate::auth::begin_login`]). Browser-only; the SSR build gets stubs.

use grey_api::{AdminUser, ApiError, CreateIncident, Incident, IncidentEdit, Peer, UiAuthConfig};

/// A client for the agent's HTTP API. Cheap to clone; holds only the public OIDC config it needs to
/// renew sessions on the caller's behalf.
#[derive(Clone, PartialEq)]
pub struct ApiClient {
    /// The public OIDC parameters, used to renew an expired session. `None` when admin auth is not
    /// configured, in which case authenticated calls simply fail with 401.
    auth: Option<UiAuthConfig>,
}

impl ApiClient {
    /// Creates a client. Pass the UI's [`UiAuthConfig`] (from the config context) so the client can
    /// renew expired sessions; `None` disables renewal.
    pub fn new(auth: Option<UiAuthConfig>) -> Self {
        Self { auth }
    }
}

#[cfg(feature = "wasm")]
mod browser {
    use super::*;
    use gloo::net::http::{Request, RequestBuilder, Response};
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    const BASE: &str = "/api/v1";
    const ADMIN: &str = "/api/v1/admin";

    enum Verb {
        Get,
        Post,
        Put,
        Delete,
    }

    fn bearer(token: &str) -> String {
        format!("Bearer {token}")
    }

    /// The advice we fall back to when nothing more specific applies: reloading the page re-runs the
    /// whole bootstrap (config, session, data) and clears most transient client-side faults.
    const REFRESH_ADVICE: &str = "Refresh the page to try again.";

    /// Guarantees an error carries at least one actionable suggestion, defaulting to a page refresh.
    fn with_fallback_advice(mut error: ApiError) -> ApiError {
        if error.advice.is_empty() {
            error.advice.push(REFRESH_ADVICE.to_string());
        }
        error
    }

    /// Coerces a transport-level failure (a browser/JS error raised before any HTTP response) into a
    /// friendly [`ApiError`] with tailored advice. These are the connectivity and serialization
    /// faults the client can hit on the way to (or back from) the agent, none of which carry an HTTP
    /// status of their own.
    fn net(err: gloo::net::Error) -> ApiError {
        use gloo::net::Error;

        match err {
            // A `JsError` here almost always means the request never reached the server: the network
            // dropped, the host is unreachable, or the browser aborted the fetch ("Failed to fetch"
            // is the canonical message). Treat it as a connectivity problem.
            Error::JsError(_) => ApiError::new("We couldn't reach the server.").with_advice_lines([
                "Check that your device is still connected to the internet.",
                "The service may be briefly unavailable — wait a moment, then refresh the page.",
            ]),
            // The server answered, but its body wasn't the JSON we expected — usually a transient
            // proxy/error page or a version skew between the UI and the agent.
            Error::SerdeError(_) => ApiError::new("The server returned an unexpected response.")
                .with_advice(REFRESH_ADVICE),
            // Anything else gloo surfaces; keep the detail but still point at a recovery step.
            other => {
                ApiError::new(format!("Something went wrong: {other}")).with_advice(REFRESH_ADVICE)
            }
        }
    }

    /// Builds an [`ApiError`] from a non-success response, preferring the agent's structured error
    /// body and falling back to a generic message stamped with the HTTP status. Either way the
    /// result is guaranteed to carry advice so the banner always has something actionable to show.
    async fn error_from(response: Response) -> ApiError {
        let status = response.status();
        let error = match response.json::<ApiError>().await {
            Ok(mut error) if !error.message.is_empty() => {
                if error.code == 0 {
                    error.code = status;
                }
                error
            }
            _ => ApiError::new(format!("Request failed (HTTP {status}).")).with_code(status),
        };
        with_fallback_advice(error)
    }

    /// Decodes a JSON success body, or surfaces the structured error on a non-success response.
    async fn read_json<T: DeserializeOwned>(response: Response) -> Result<T, ApiError> {
        if response.ok() {
            response.json::<T>().await.map_err(net)
        } else {
            Err(error_from(response).await)
        }
    }

    /// Builds a request for the given verb, attaching the bearer token, `If-Match` ETag and JSON
    /// body when supplied.
    fn build<B: Serialize>(
        verb: &Verb,
        url: &str,
        token: Option<&str>,
        if_match: Option<u64>,
        body: Option<&B>,
    ) -> Result<Request, ApiError> {
        let mut builder: RequestBuilder = match verb {
            Verb::Get => Request::get(url),
            Verb::Post => Request::post(url),
            Verb::Put => Request::put(url),
            Verb::Delete => Request::delete(url),
        };
        if let Some(token) = token {
            builder = builder.header("Authorization", &bearer(token));
        }
        if let Some(version) = if_match {
            builder = builder.header("If-Match", &format!("\"{version}\""));
        }
        match body {
            Some(body) => builder.json(body).map_err(net),
            None => builder.build().map_err(net),
        }
    }

    impl ApiClient {
        // --- Public endpoints -------------------------------------------------------------------

        /// Every probe's current state.
        pub async fn probes(&self) -> Result<Vec<grey_api::Probe>, ApiError> {
            self.get_json(&format!("{BASE}/probes")).await
        }

        /// The configured UI notices.
        pub async fn notices(&self) -> Result<Vec<grey_api::UiNotice>, ApiError> {
            self.get_json(&format!("{BASE}/notices")).await
        }

        /// The publicly visible incidents (hidden drafts are excluded).
        pub async fn incidents(&self) -> Result<Vec<Incident>, ApiError> {
            self.get_json(&format!("{BASE}/incidents")).await
        }

        // --- Admin endpoints --------------------------------------------------------------------

        /// The signed-in administrator, derived from the bearer token's claims.
        pub async fn me(&self) -> Result<AdminUser, ApiError> {
            self.get_json(&format!("{ADMIN}/me")).await
        }

        /// The cluster's peers as seen by this node (operator-only).
        pub async fn peers(&self) -> Result<Vec<Peer>, ApiError> {
            self.get_json(&format!("{ADMIN}/cluster/peers")).await
        }

        /// Every incident, including hidden drafts (admin view).
        pub async fn admin_incidents(&self) -> Result<Vec<Incident>, ApiError> {
            self.get_json(&format!("{ADMIN}/incidents")).await
        }

        /// A single incident (including hidden), by id.
        pub async fn incident(&self, id: &str) -> Result<Incident, ApiError> {
            self.get_json(&format!("{ADMIN}/incidents/{id}")).await
        }

        /// Creates an incident from a title and its opening update.
        pub async fn create_incident(&self, input: &CreateIncident) -> Result<Incident, ApiError> {
            let response = self
                .send(Verb::Post, &format!("{ADMIN}/incidents"), None, Some(input))
                .await?;
            read_json(response).await
        }

        /// Replaces an incident via check-and-set: `version` is sent as the `If-Match` ETag, so a
        /// concurrent change surfaces as a 412 [`ApiError`].
        pub async fn replace_incident(
            &self,
            id: &str,
            version: u64,
            edit: &IncidentEdit,
        ) -> Result<Incident, ApiError> {
            let response = self
                .send(
                    Verb::Put,
                    &format!("{ADMIN}/incidents/{id}"),
                    Some(version),
                    Some(edit),
                )
                .await?;
            read_json(response).await
        }

        /// Deletes an incident.
        pub async fn delete_incident(&self, id: &str) -> Result<(), ApiError> {
            let response = self
                .send::<()>(Verb::Delete, &format!("{ADMIN}/incidents/{id}"), None, None)
                .await?;
            if response.ok() {
                Ok(())
            } else {
                Err(error_from(response).await)
            }
        }

        // --- Request orchestration --------------------------------------------------------------

        /// Sends a GET and decodes a JSON success body, surfacing any error response.
        async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, ApiError> {
            let response = self.send::<()>(Verb::Get, url, None, None).await?;
            read_json(response).await
        }

        /// Sends a request, attaching the stored bearer when one is available. On a 401 it renews
        /// the session from the refresh token and retries once; a renewal failure drops the dead
        /// session so the UI can re-prompt for sign-in.
        async fn send<B: Serialize>(
            &self,
            verb: Verb,
            url: &str,
            if_match: Option<u64>,
            body: Option<&B>,
        ) -> Result<Response, ApiError> {
            let token = crate::auth::stored_token();
            let response = build(&verb, url, token.as_deref(), if_match, body)?
                .send()
                .await
                .map_err(net)?;

            if response.status() != 401 {
                return Ok(response);
            }

            // The token was rejected as expired/invalid: try a silent refresh, then retry once.
            if self.auth.is_some()
                && let Ok(fresh) = crate::auth::refresh_session().await
            {
                return build(&verb, url, Some(&fresh), if_match, body)?
                    .send()
                    .await
                    .map_err(net);
            }

            // Renewal isn't possible — drop the session so the UI prompts for an interactive login.
            crate::auth::clear_token();
            Ok(response)
        }
    }
}

#[cfg(not(feature = "wasm"))]
impl ApiClient {
    fn unavailable<T>() -> Result<T, ApiError> {
        Err(ApiError::new("the API is unavailable during server rendering"))
    }

    pub async fn probes(&self) -> Result<Vec<grey_api::Probe>, ApiError> {
        Self::unavailable()
    }
    pub async fn notices(&self) -> Result<Vec<grey_api::UiNotice>, ApiError> {
        Self::unavailable()
    }
    pub async fn incidents(&self) -> Result<Vec<Incident>, ApiError> {
        Self::unavailable()
    }
    pub async fn me(&self) -> Result<AdminUser, ApiError> {
        Self::unavailable()
    }
    pub async fn peers(&self) -> Result<Vec<Peer>, ApiError> {
        Self::unavailable()
    }
    pub async fn admin_incidents(&self) -> Result<Vec<Incident>, ApiError> {
        Self::unavailable()
    }
    pub async fn incident(&self, _id: &str) -> Result<Incident, ApiError> {
        Self::unavailable()
    }
    pub async fn create_incident(&self, _input: &CreateIncident) -> Result<Incident, ApiError> {
        Self::unavailable()
    }
    pub async fn replace_incident(
        &self,
        _id: &str,
        _version: u64,
        _edit: &IncidentEdit,
    ) -> Result<Incident, ApiError> {
        Self::unavailable()
    }
    pub async fn delete_incident(&self, _id: &str) -> Result<(), ApiError> {
        Self::unavailable()
    }
}
