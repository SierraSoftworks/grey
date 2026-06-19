//! The SPA's single API client.
//!
//! [`ApiClient`] exposes one method per entity operation, so callers never hand-build requests. Every
//! failure surfaces as a [`grey_api::ApiError`], whose `code` mirrors the HTTP status so callers can
//! classify problems.
//!
//! Authenticated calls attach the stored ID token as an `Authorization: Bearer` header. When the
//! agent rejects a token as expired (HTTP 401), the client transparently renews it from the stored
//! refresh token (see [`crate::auth`]) and retries the request once; interactive sign-in is handled
//! separately via a popup (see [`crate::auth::begin_login`]). Browser-only; the SSR build gets stubs.

use grey_api::{
    AdminUser, ApiError, CreateIncident, CreateUpdate, IncidentUpdateId, IncidentView,
    IncidentsPage, Peer, PutIncident, PutUpdate, UiAuthConfig,
};

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

    const REFRESH_ADVICE: &str = "Refresh the page to try again.";

    fn with_fallback_advice(mut error: ApiError) -> ApiError {
        if error.advice.is_empty() {
            error.advice.push(REFRESH_ADVICE.to_string());
        }
        error
    }

    fn net(err: gloo::net::Error) -> ApiError {
        use gloo::net::Error;

        match err {
            Error::JsError(_) => ApiError::new("We couldn't reach the server.").with_advice_lines([
                "Check that your device is still connected to the internet.",
                "The service may be briefly unavailable — wait a moment, then refresh the page.",
            ]),
            Error::SerdeError(_) => ApiError::new("The server returned an unexpected response.")
                .with_advice(REFRESH_ADVICE),
            other => {
                ApiError::new(format!("Something went wrong: {other}")).with_advice(REFRESH_ADVICE)
            }
        }
    }

    async fn error_from(response: Response) -> ApiError {
        let status = response.status();
        let error = match response.json::<ApiError>().await {
            Ok(error) if !error.message.is_empty() => error.ensure_code(status),
            _ => ApiError::new(format!("Request failed (HTTP {status}).")).with_code(status),
        };
        with_fallback_advice(error)
    }

    async fn read_json<T: DeserializeOwned>(response: Response) -> Result<T, ApiError> {
        if response.ok() {
            response.json::<T>().await.map_err(net)
        } else {
            Err(error_from(response).await)
        }
    }

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
            builder = builder.header("If-Match", &grey_api::version_etag(version));
        }
        match body {
            Some(body) => builder.json(body).map_err(net),
            None => builder.build().map_err(net),
        }
    }

    impl ApiClient {
        // --- Public endpoints -------------------------------------------------------------------

        pub async fn probes(&self) -> Result<Vec<grey_api::Probe>, ApiError> {
            self.get_json(&format!("{BASE}/probes")).await
        }

        pub async fn crons(&self) -> Result<Vec<grey_api::Cron>, ApiError> {
            self.get_json(&format!("{BASE}/crons")).await
        }

        pub async fn notices(&self) -> Result<Vec<grey_api::UiNotice>, ApiError> {
            self.get_json(&format!("{BASE}/notices")).await
        }

        /// The first page of publicly visible incidents (hidden drafts excluded), each with its
        /// updates embedded.
        pub async fn incidents(&self) -> Result<Vec<IncidentView>, ApiError> {
            let page: IncidentsPage = self.get_json(&format!("{BASE}/incidents")).await?;
            Ok(page.incidents)
        }

        /// A page of publicly visible incidents continuing from `cursor` (for "load more").
        pub async fn incidents_page(
            &self,
            cursor: Option<&str>,
        ) -> Result<IncidentsPage, ApiError> {
            let url = match cursor {
                Some(c) => format!("{BASE}/incidents?cursor={c}"),
                None => format!("{BASE}/incidents"),
            };
            self.get_json(&url).await
        }

        // --- Admin endpoints --------------------------------------------------------------------

        pub async fn me(&self) -> Result<AdminUser, ApiError> {
            self.get_json(&format!("{ADMIN}/me")).await
        }

        pub async fn peers(&self) -> Result<Vec<Peer>, ApiError> {
            self.get_json(&format!("{ADMIN}/cluster/peers")).await
        }

        /// The first page of incidents including hidden drafts (admin view).
        pub async fn admin_incidents(&self) -> Result<Vec<IncidentView>, ApiError> {
            let page: IncidentsPage = self.get_json(&format!("{ADMIN}/incidents")).await?;
            Ok(page.incidents)
        }

        /// A single incident (including hidden), by id.
        pub async fn incident(&self, id: &str) -> Result<IncidentView, ApiError> {
            self.get_json(&format!("{ADMIN}/incidents/{id}")).await
        }

        /// Creates an incident from a title and its opening update.
        pub async fn create_incident(
            &self,
            input: &CreateIncident,
        ) -> Result<IncidentView, ApiError> {
            let response = self
                .send(Verb::Post, &format!("{ADMIN}/incidents"), None, Some(input))
                .await?;
            read_json(response).await
        }

        /// Replaces an incident's title via check-and-set on its `version` (the `If-Match` ETag).
        pub async fn put_incident(
            &self,
            id: &str,
            version: u64,
            edit: &PutIncident,
        ) -> Result<IncidentView, ApiError> {
            let response = self
                .send(Verb::Put, &format!("{ADMIN}/incidents/{id}"), Some(version), Some(edit))
                .await?;
            read_json(response).await
        }

        /// Deletes an incident via check-and-set on its `version`.
        pub async fn delete_incident(&self, id: &str, version: u64) -> Result<(), ApiError> {
            let response = self
                .send::<()>(Verb::Delete, &format!("{ADMIN}/incidents/{id}"), Some(version), None)
                .await?;
            if response.ok() {
                Ok(())
            } else {
                Err(error_from(response).await)
            }
        }

        /// Adds a new update to an incident.
        pub async fn create_update(
            &self,
            incident_id: &str,
            input: &CreateUpdate,
        ) -> Result<IncidentView, ApiError> {
            let response = self
                .send(Verb::Post, &format!("{ADMIN}/incidents/{incident_id}/updates"), None, Some(input))
                .await?;
            read_json(response).await
        }

        /// Replaces an update's message via check-and-set on the update's `version`.
        pub async fn put_update(
            &self,
            update_id: IncidentUpdateId,
            version: u64,
            edit: &PutUpdate,
        ) -> Result<IncidentView, ApiError> {
            let incident = update_id.incident_id();
            let response = self
                .send(
                    Verb::Put,
                    &format!("{ADMIN}/incidents/{incident}/updates/{update_id}"),
                    Some(version),
                    Some(edit),
                )
                .await?;
            read_json(response).await
        }

        /// Deletes an update via check-and-set on its `version`.
        pub async fn delete_update(
            &self,
            update_id: IncidentUpdateId,
            version: u64,
        ) -> Result<(), ApiError> {
            let incident = update_id.incident_id();
            let response = self
                .send::<()>(
                    Verb::Delete,
                    &format!("{ADMIN}/incidents/{incident}/updates/{update_id}"),
                    Some(version),
                    None,
                )
                .await?;
            if response.ok() {
                Ok(())
            } else {
                Err(error_from(response).await)
            }
        }

        // --- Request orchestration --------------------------------------------------------------

        async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, ApiError> {
            let response = self.send::<()>(Verb::Get, url, None, None).await?;
            read_json(response).await
        }

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

            if self.auth.is_some()
                && let Ok(fresh) = crate::auth::refresh_session().await
            {
                return build(&verb, url, Some(&fresh), if_match, body)?
                    .send()
                    .await
                    .map_err(net);
            }

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
    pub async fn crons(&self) -> Result<Vec<grey_api::Cron>, ApiError> {
        Self::unavailable()
    }
    pub async fn notices(&self) -> Result<Vec<grey_api::UiNotice>, ApiError> {
        Self::unavailable()
    }
    pub async fn incidents(&self) -> Result<Vec<IncidentView>, ApiError> {
        Self::unavailable()
    }
    pub async fn incidents_page(&self, _cursor: Option<&str>) -> Result<IncidentsPage, ApiError> {
        Self::unavailable()
    }
    pub async fn me(&self) -> Result<AdminUser, ApiError> {
        Self::unavailable()
    }
    pub async fn peers(&self) -> Result<Vec<Peer>, ApiError> {
        Self::unavailable()
    }
    pub async fn admin_incidents(&self) -> Result<Vec<IncidentView>, ApiError> {
        Self::unavailable()
    }
    pub async fn incident(&self, _id: &str) -> Result<IncidentView, ApiError> {
        Self::unavailable()
    }
    pub async fn create_incident(&self, _input: &CreateIncident) -> Result<IncidentView, ApiError> {
        Self::unavailable()
    }
    pub async fn put_incident(
        &self,
        _id: &str,
        _version: u64,
        _edit: &PutIncident,
    ) -> Result<IncidentView, ApiError> {
        Self::unavailable()
    }
    pub async fn delete_incident(&self, _id: &str, _version: u64) -> Result<(), ApiError> {
        Self::unavailable()
    }
    pub async fn create_update(
        &self,
        _incident_id: &str,
        _input: &CreateUpdate,
    ) -> Result<IncidentView, ApiError> {
        Self::unavailable()
    }
    pub async fn put_update(
        &self,
        _update_id: IncidentUpdateId,
        _version: u64,
        _edit: &PutUpdate,
    ) -> Result<IncidentView, ApiError> {
        Self::unavailable()
    }
    pub async fn delete_update(
        &self,
        _update_id: IncidentUpdateId,
        _version: u64,
    ) -> Result<(), ApiError> {
        Self::unavailable()
    }
}
