//! Authenticated client for the admin API. Mutating calls attach the stored ID token as an
//! `Authorization: Bearer` header. Browser-only; the SSR build gets stubs.

use grey_api::{AdminUser, Incident, IncidentInput, NewIncidentUpdate};

/// A failure talking to the admin API, surfaced to the user.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// The token is missing, invalid, or expired (HTTP 401).
    Unauthorized,
    /// The account is not permitted to perform the action (HTTP 403).
    Forbidden,
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
            ApiError::Network(msg) => write!(f, "Network error: {msg}"),
            ApiError::Server(msg) => write!(f, "{msg}"),
        }
    }
}

#[cfg(feature = "wasm")]
mod browser {
    use super::*;
    use gloo::net::http::{Request, Response};
    use serde::Serialize;
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

    async fn send_json<B: Serialize>(
        builder: gloo::net::http::RequestBuilder,
        token: &str,
        body: &B,
    ) -> Result<Response, ApiError> {
        builder
            .header("Authorization", &bearer(token))
            .json(body)
            .map_err(net)?
            .send()
            .await
            .map_err(net)
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

    pub async fn create_incident(token: &str, input: &IncidentInput) -> Result<Incident, ApiError> {
        let response = send_json(Request::post(&format!("{BASE}/incidents")), token, input).await?;
        read_json(response).await
    }

    pub async fn update_incident(
        token: &str,
        id: &str,
        input: &IncidentInput,
    ) -> Result<Incident, ApiError> {
        let response =
            send_json(Request::put(&format!("{BASE}/incidents/{id}")), token, input).await?;
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

    pub async fn add_update(
        token: &str,
        id: &str,
        update: &NewIncidentUpdate,
    ) -> Result<Incident, ApiError> {
        let response = send_json(
            Request::post(&format!("{BASE}/incidents/{id}/updates")),
            token,
            update,
        )
        .await?;
        read_json(response).await
    }
}

#[cfg(feature = "wasm")]
pub use browser::{
    add_update, create_incident, delete_incident, fetch_me, list_incidents, update_incident,
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
    pub async fn create_incident(_t: &str, _i: &IncidentInput) -> Result<Incident, ApiError> {
        unavailable()
    }
    pub async fn update_incident(
        _t: &str,
        _id: &str,
        _i: &IncidentInput,
    ) -> Result<Incident, ApiError> {
        unavailable()
    }
    pub async fn delete_incident(_t: &str, _id: &str) -> Result<(), ApiError> {
        unavailable()
    }
    pub async fn add_update(
        _t: &str,
        _id: &str,
        _u: &NewIncidentUpdate,
    ) -> Result<Incident, ApiError> {
        unavailable()
    }
}

#[cfg(not(feature = "wasm"))]
pub use stub::{
    add_update, create_incident, delete_incident, fetch_me, list_incidents, update_incident,
};
