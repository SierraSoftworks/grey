//! Authenticated client for the admin API. Mutating calls attach the stored ID token as an
//! `Authorization: Bearer` header and use the incident `version` as an `If-Match` ETag for
//! check-and-set updates. Browser-only; the SSR build gets stubs.

use grey_api::{AdminUser, CreateIncident, Incident, IncidentEdit};

/// A failure talking to the admin API, surfaced to the user.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// The token is missing, invalid, or expired (HTTP 401).
    Unauthorized,
    /// The account is not permitted to perform the action (HTTP 403).
    Forbidden,
    /// The incident was modified concurrently — the check-and-set failed (HTTP 412/428).
    Conflict,
    /// A transport-level failure (no response).
    Network(String),
    /// The server returned an error with a message.
    Server(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "Your session has expired. Please sign in again."),
            ApiError::Forbidden => {
                write!(f, "Your account is not permitted to perform this action.")
            }
            ApiError::Conflict => {
                write!(f, "This incident was changed elsewhere. Reload and try again.")
            }
            ApiError::Network(msg) => write!(f, "Network error: {msg}"),
            ApiError::Server(msg) => write!(f, "{msg}"),
        }
    }
}

#[cfg(feature = "wasm")]
mod browser {
    use super::*;
    use gloo::net::http::{Request, Response};
    use serde::de::DeserializeOwned;

    const BASE: &str = "/api/v1/admin";

    fn bearer(token: &str) -> String {
        format!("Bearer {token}")
    }

    fn net(err: gloo::net::Error) -> ApiError {
        ApiError::Network(err.to_string())
    }

    async fn error_from(response: Response) -> ApiError {
        match response.status() {
            401 => ApiError::Unauthorized,
            403 => ApiError::Forbidden,
            412 | 428 => ApiError::Conflict,
            status => {
                let message = response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                    .unwrap_or_else(|| format!("Request failed (HTTP {status})."));
                ApiError::Server(message)
            }
        }
    }

    async fn read_json<T: DeserializeOwned>(response: Response) -> Result<T, ApiError> {
        if response.ok() {
            response.json::<T>().await.map_err(net)
        } else {
            Err(error_from(response).await)
        }
    }

    pub async fn fetch_me(token: &str) -> Result<AdminUser, ApiError> {
        let response = Request::get(&format!("{BASE}/me"))
            .header("Authorization", &bearer(token))
            .send()
            .await
            .map_err(net)?;
        read_json(response).await
    }

    pub async fn list_incidents(token: &str) -> Result<Vec<Incident>, ApiError> {
        let response = Request::get(&format!("{BASE}/incidents"))
            .header("Authorization", &bearer(token))
            .send()
            .await
            .map_err(net)?;
        read_json(response).await
    }

    pub async fn get_incident(token: &str, id: &str) -> Result<Incident, ApiError> {
        let response = Request::get(&format!("{BASE}/incidents/{id}"))
            .header("Authorization", &bearer(token))
            .send()
            .await
            .map_err(net)?;
        read_json(response).await
    }

    pub async fn create_incident(token: &str, input: &CreateIncident) -> Result<Incident, ApiError> {
        let response = Request::post(&format!("{BASE}/incidents"))
            .header("Authorization", &bearer(token))
            .json(input)
            .map_err(net)?
            .send()
            .await
            .map_err(net)?;
        read_json(response).await
    }

    /// Replaces an incident via check-and-set: `version` is sent as the `If-Match` ETag, so a
    /// concurrent change surfaces as [`ApiError::Conflict`].
    pub async fn replace_incident(
        token: &str,
        id: &str,
        version: u64,
        edit: &IncidentEdit,
    ) -> Result<Incident, ApiError> {
        let response = Request::put(&format!("{BASE}/incidents/{id}"))
            .header("Authorization", &bearer(token))
            .header("If-Match", &format!("\"{version}\""))
            .json(edit)
            .map_err(net)?
            .send()
            .await
            .map_err(net)?;
        read_json(response).await
    }

    pub async fn delete_incident(token: &str, id: &str) -> Result<(), ApiError> {
        let response = Request::delete(&format!("{BASE}/incidents/{id}"))
            .header("Authorization", &bearer(token))
            .send()
            .await
            .map_err(net)?;
        if response.ok() {
            Ok(())
        } else {
            Err(error_from(response).await)
        }
    }
}

#[cfg(feature = "wasm")]
pub use browser::{
    create_incident, delete_incident, fetch_me, get_incident, list_incidents, replace_incident,
};

#[cfg(not(feature = "wasm"))]
mod stub {
    use super::*;

    fn unavailable<T>() -> Result<T, ApiError> {
        Err(ApiError::Network("the admin API is unavailable during server rendering".into()))
    }

    pub async fn fetch_me(_token: &str) -> Result<AdminUser, ApiError> {
        unavailable()
    }
    pub async fn list_incidents(_token: &str) -> Result<Vec<Incident>, ApiError> {
        unavailable()
    }
    pub async fn get_incident(_token: &str, _id: &str) -> Result<Incident, ApiError> {
        unavailable()
    }
    pub async fn create_incident(_t: &str, _i: &CreateIncident) -> Result<Incident, ApiError> {
        unavailable()
    }
    pub async fn replace_incident(
        _t: &str,
        _id: &str,
        _v: u64,
        _e: &IncidentEdit,
    ) -> Result<Incident, ApiError> {
        unavailable()
    }
    pub async fn delete_incident(_t: &str, _id: &str) -> Result<(), ApiError> {
        unavailable()
    }
}

#[cfg(not(feature = "wasm"))]
pub use stub::{
    create_incident, delete_incident, fetch_me, get_incident, list_incidents, replace_incident,
};
