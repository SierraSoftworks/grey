//! The shared encoding of an entity `version` as an HTTP entity-tag (`ETag` / `If-Match`).
//!
//! Both ends of the API speak this format: the agent stamps an `ETag` response header and parses the
//! `If-Match` request header on a check-and-set write, while the UI builds the `If-Match` header for
//! those same writes. Keeping the one definition here means the (weak-validator-aware) wire format can
//! never drift between the two crates.

/// Formats an entity `version` as a strong entity-tag, quoted per RFC 7232 (e.g. `3` → `"3"`).
pub fn version_etag(version: u64) -> String {
    format!("\"{version}\"")
}

/// Parses a version out of an `ETag` / `If-Match` header value — the strong form `"3"` or the weak
/// form `W/"3"` — returning `None` when it is missing-shaped or non-numeric.
pub fn parse_if_match(raw: &str) -> Option<u64> {
    raw.trim().trim_start_matches("W/").trim_matches('"').parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_round_trips_through_the_etag_format() {
        for version in [0u64, 1, 42, u64::MAX] {
            assert_eq!(parse_if_match(&version_etag(version)), Some(version));
        }
    }

    #[test]
    fn version_etag_quotes_per_rfc7232() {
        assert_eq!(version_etag(7), "\"7\"");
    }

    #[test]
    fn parse_accepts_weak_validators_and_whitespace() {
        assert_eq!(parse_if_match("W/\"5\""), Some(5));
        assert_eq!(parse_if_match("  \"5\"  "), Some(5));
    }

    #[test]
    fn parse_rejects_non_numeric_or_empty() {
        assert_eq!(parse_if_match(""), None);
        assert_eq!(parse_if_match("\"\""), None);
        assert_eq!(parse_if_match("\"abc\""), None);
        assert_eq!(parse_if_match("*"), None);
    }
}
