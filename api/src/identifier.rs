use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// A compact, URL-friendly identifier backed by a `u32`. It is serialized and displayed as lowercase
/// base36 split into dash-separated groups of at most three characters (e.g. `1up-3mt-g`), which is
/// the human-friendly form used in URLs and the UI; the underlying numeric value is what the store
/// keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Identifier(u64);

impl Identifier {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Parses a grouped base36 string back into an identifier; dashes are ignored and letters are
    /// case-insensitive. Returns `None` for empty or out-of-range input.
    pub fn parse(text: &str) -> Option<Self> {
        let cleaned: String = text.chars().filter(|c| *c != '-').collect();
        if cleaned.is_empty() {
            return None;
        }
        u64::from_str_radix(&cleaned, 36).ok().map(Self)
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = Vec::new();
        let mut n = self.0;
        if n == 0 {
            chars.push(b'0');
        }
        while n > 0 {
            chars.push(DIGITS[(n % 36) as usize]);
            n /= 36;
        }
        chars.reverse();

        let grouped = chars
            .chunks(4)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("-");
        f.write_str(&grouped)
    }
}

impl From<u64> for Identifier {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Identifier> for u64 {
    fn from(id: Identifier) -> u64 {
        id.0
    }
}

/// Lenient conversion: an unparseable string yields the zero identifier. Prefer [`Identifier::parse`]
/// (or [`FromStr`]) when invalid input should be rejected.
impl From<&str> for Identifier {
    fn from(text: &str) -> Self {
        Self::parse(text).unwrap_or(Self(0))
    }
}

impl FromStr for Identifier {
    type Err = InvalidIdentifier;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(InvalidIdentifier)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidIdentifier;

impl fmt::Display for InvalidIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid identifier")
    }
}

impl std::error::Error for InvalidIdentifier {}

impl Serialize for Identifier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| D::Error::custom(format!("invalid identifier: {text}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_string_and_u64() {
        for value in [0u64, 1, 35, 36, 1_234_567, u64::MAX] {
            let id = Identifier::from(value);
            // Into<u64> recovers the value.
            assert_eq!(u64::from(id), value);
            // Display -> parse round-trips.
            assert_eq!(Identifier::parse(&id.to_string()), Some(id));
            // serde round-trips through the grouped-base36 string.
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(serde_json::from_str::<Identifier>(&json).unwrap(), id);
        }
    }

    #[test]
    fn display_groups_in_fours() {
        let encoded = Identifier::from(u64::MAX).to_string();
        assert!(encoded.split('-').all(|g| (1..=4).contains(&g.len())), "bad grouping: {encoded}");
    }

    #[test]
    fn from_str_is_strict_but_from_str_ref_is_lenient() {
        // dashes ignored, case-insensitive
        assert_eq!("1U-P3MT-G".parse::<Identifier>().ok(), "1up3mtg".parse::<Identifier>().ok());
        // strict parsing rejects junk
        assert!("".parse::<Identifier>().is_err());
        assert!("!!".parse::<Identifier>().is_err());
        // lenient From<&str> falls back to zero
        assert_eq!(Identifier::from("!!"), Identifier::from(0u64));
    }

    #[test]
    fn serializes_as_a_json_string() {
        assert_eq!(serde_json::to_string(&Identifier::from(0u64)).unwrap(), "\"0\"");
        assert!(serde_json::from_str::<Identifier>("\"!!\"").is_err());
    }
}
