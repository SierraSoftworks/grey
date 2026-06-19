use serde::{Deserialize, Serialize};

/// A structured error returned by the HTTP API.
///
/// Every failure the API reports carries a human-readable [`message`](Self::message) describing what
/// happened, plus a list of [`advice`](Self::advice) entries the caller can act on to resolve the
/// problem themselves. The [`code`](Self::code) mirrors the HTTP status so clients can classify the
/// failure without inspecting the response object separately.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    /// The HTTP status code associated with this error (e.g. `404`). Lets clients branch on the
    /// failure class (auth, conflict, not-found, server) without re-reading the transport status.
    #[serde(default)]
    pub code: u16,

    /// A short, human-readable description of what went wrong (e.g. "The incident you requested
    /// could not be found.").
    pub message: String,

    /// Suggested actions the caller can take to resolve the issue from the client side (e.g. "Check
    /// that the incident ID you've provided is correct.").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advice: Vec<String>,
}

impl ApiError {
    /// Creates an error with the given message and no advice or status code.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: 0,
            message: message.into(),
            advice: Vec::new(),
        }
    }

    /// Sets the HTTP status code, returning the error for chaining.
    pub fn with_code(mut self, code: u16) -> Self {
        self.code = code;
        self
    }

    /// Fills in the status code only if one has not already been set (`code == 0`). Unlike
    /// [`with_code`](Self::with_code), which always overwrites, this lets a caller stamp a
    /// transport-derived status (e.g. the HTTP response code) onto a body without clobbering a code the
    /// server already supplied — used on the client read path, where the body's own code wins.
    pub fn ensure_code(mut self, code: u16) -> Self {
        if self.code == 0 {
            self.code = code;
        }
        self
    }

    /// Appends a single piece of advice, returning the error for chaining.
    pub fn with_advice(mut self, advice: impl Into<String>) -> Self {
        self.advice.push(advice.into());
        self
    }

    /// Appends several pieces of advice, returning the error for chaining.
    pub fn with_advice_lines<I, S>(mut self, advice: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.advice.extend(advice.into_iter().map(Into::into));
        self
    }
}

/// Constructors for the HTTP failures the API reports, each pre-stamped with its status code so the
/// body's [`code`](ApiError::code) always matches the response status. Add advice with
/// [`with_advice`](ApiError::with_advice); with the `server` feature these convert straight into an
/// actix `HttpResponse` (see the `server` module below).
impl ApiError {
    /// `400 Bad Request` — the caller's input was malformed or failed validation.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(message).with_code(400)
    }

    /// `401 Unauthorized` — authentication is missing or no longer valid.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(message).with_code(401)
    }

    /// `403 Forbidden` — the caller is authenticated but not permitted to perform the action.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(message).with_code(403)
    }

    /// `404 Not Found` — the requested resource does not exist (or is not visible to the caller).
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(message).with_code(404)
    }

    /// `412 Precondition Failed` — a check-and-set `If-Match` version no longer matches.
    pub fn precondition_failed(message: impl Into<String>) -> Self {
        Self::new(message).with_code(412)
    }

    /// `413 Payload Too Large` — the submitted body exceeds the accepted size.
    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(message).with_code(413)
    }

    /// `428 Precondition Required` — the request must carry an `If-Match` version but did not.
    pub fn precondition_required(message: impl Into<String>) -> Self {
        Self::new(message).with_code(428)
    }

    /// `500 Internal Server Error` — an unexpected server-side failure.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(message).with_code(500)
    }
}

/// Server-only conveniences for returning an [`ApiError`] from an actix-web handler. Compiled only
/// when the `server` feature is enabled so the UI (which shares the type for deserialization) never
/// pulls in actix-web.
#[cfg(feature = "server")]
mod server {
    use super::ApiError;
    use actix_web::{HttpResponse, ResponseError, http::StatusCode};

    impl ApiError {
        /// The response status this error maps to, derived from [`code`](ApiError::code). An unset or
        /// out-of-range code falls back to `500 Internal Server Error`.
        fn status_code_or_internal(&self) -> StatusCode {
            StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }

    /// Lets handlers return `Result<_, ApiError>` and `?` an `ApiError`: actix renders it as a JSON
    /// body with the matching HTTP status.
    impl ResponseError for ApiError {
        fn status_code(&self) -> StatusCode {
            self.status_code_or_internal()
        }

        fn error_response(&self) -> HttpResponse {
            // Route through `status_code()` (rather than the inherent helper) so the trait method is
            // the single place the body's code becomes the response status.
            HttpResponse::build(self.status_code()).json(self)
        }
    }

    /// Lets handlers build a response directly: `Ok(ApiError::not_found("…").into())`.
    impl From<ApiError> for HttpResponse {
        fn from(error: ApiError) -> Self {
            error.error_response()
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_is_omitted_when_empty() {
        let error = ApiError::new("Something went wrong.");
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json, serde_json::json!({ "code": 0, "message": "Something went wrong." }));
    }

    #[test]
    fn code_round_trips() {
        let error = ApiError::new("The incident you requested could not be found.").with_code(404);
        let json = serde_json::to_string(&error).unwrap();
        let parsed: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, error);
        assert_eq!(parsed.code, 404);
    }

    #[test]
    fn advice_round_trips() {
        let error = ApiError::new("The incident you requested could not be found.")
            .with_advice("Check that the incident ID you've provided is correct.")
            .with_advice_lines(["It may have been deleted since you last loaded the page."]);

        let json = serde_json::to_string(&error).unwrap();
        let parsed: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, error);
        assert_eq!(parsed.advice.len(), 2);
    }

    #[test]
    fn ensure_code_fills_only_when_unset() {
        // An explicit code on the body is preserved; a transport status only fills an unset one.
        assert_eq!(ApiError::new("x").ensure_code(503).code, 503);
        assert_eq!(ApiError::not_found("x").ensure_code(503).code, 404);
        assert_eq!(ApiError::new("x").with_code(0).ensure_code(500).code, 500);
    }

    #[test]
    fn semantic_constructors_stamp_their_status_code() {
        assert_eq!(ApiError::bad_request("x").code, 400);
        assert_eq!(ApiError::unauthorized("x").code, 401);
        assert_eq!(ApiError::forbidden("x").code, 403);
        assert_eq!(ApiError::not_found("x").code, 404);
        assert_eq!(ApiError::precondition_failed("x").code, 412);
        assert_eq!(ApiError::payload_too_large("x").code, 413);
        assert_eq!(ApiError::precondition_required("x").code, 428);
        assert_eq!(ApiError::internal("x").code, 500);
    }
}

#[cfg(all(test, feature = "server"))]
mod server_tests {
    use super::ApiError;
    use actix_web::{HttpResponse, ResponseError, http::StatusCode};

    #[test]
    fn into_response_uses_the_error_code_as_status() {
        let response: HttpResponse = ApiError::not_found("nope").into();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn an_unset_code_falls_back_to_500() {
        // `ApiError::new` leaves `code` at 0, which is not a valid HTTP status.
        assert_eq!(ApiError::new("boom").status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
