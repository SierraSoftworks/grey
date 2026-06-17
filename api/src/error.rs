use serde::{Deserialize, Serialize};

/// A structured error returned by the HTTP API.
///
/// Every failure the API reports carries a human-readable [`message`](Self::message) describing what
/// happened, plus a list of [`advice`](Self::advice) entries the caller can act on to resolve the
/// problem themselves.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    /// A short, human-readable description of what went wrong (e.g. "The incident you requested
    /// could not be found.").
    pub message: String,

    /// Suggested actions the caller can take to resolve the issue from the client side (e.g. "Check
    /// that the incident ID you've provided is correct.").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advice: Vec<String>,
}

impl ApiError {
    /// Creates an error with the given message and no advice.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            advice: Vec::new(),
        }
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
        assert_eq!(json, serde_json::json!({ "message": "Something went wrong." }));
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
}
