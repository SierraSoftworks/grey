use std::fmt;
use std::str::FromStr;
use base64::prelude::*;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

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
        BASE64_URL_SAFE_NO_PAD.decode(text).ok().and_then(|bytes| {
            if bytes.len() != 8 {
                return None;
            }
            let mut value = 0u64;
            for &byte in &bytes {
                value = (value << 8) | byte as u64;
            }
            Some(Self(value))
        })
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", BASE64_URL_SAFE_NO_PAD.encode(self.0.to_be_bytes()))
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

/// The identifier of a single incident update, backed by a `u128` whose high 64 bits are the parent
/// [`Identifier`] (incident id) and whose low 64 bits are the update's own snowflake. Embedding the
/// incident id lets every update for an incident be found as a contiguous key range
/// `[incident<<64, (incident+1)<<64)`, and keeps updates globally unique without coordination.
/// Serialized/displayed as base64url of its 16 big-endian bytes, mirroring [`Identifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct IncidentUpdateId(u128);

impl IncidentUpdateId {
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Composes an update id from its parent incident and the update's own snowflake half.
    pub fn compose(incident: Identifier, update_snowflake: u64) -> Self {
        Self(((u64::from(incident) as u128) << 64) | update_snowflake as u128)
    }

    /// The parent incident id (the high 64 bits).
    pub fn incident_id(self) -> Identifier {
        Identifier::new((self.0 >> 64) as u64)
    }

    /// The update's own snowflake half (the low 64 bits).
    pub fn update_part(self) -> u64 {
        self.0 as u64
    }

    /// Parses the base64url-of-16-bytes form back into an id; `None` for malformed input.
    pub fn parse(text: &str) -> Option<Self> {
        BASE64_URL_SAFE_NO_PAD.decode(text).ok().and_then(|bytes| {
            if bytes.len() != 16 {
                return None;
            }
            let mut value = 0u128;
            for &byte in &bytes {
                value = (value << 8) | byte as u128;
            }
            Some(Self(value))
        })
    }
}

impl fmt::Display for IncidentUpdateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", BASE64_URL_SAFE_NO_PAD.encode(self.0.to_be_bytes()))
    }
}

impl From<u128> for IncidentUpdateId {
    fn from(value: u128) -> Self {
        Self(value)
    }
}

impl From<IncidentUpdateId> for u128 {
    fn from(id: IncidentUpdateId) -> u128 {
        id.0
    }
}

impl FromStr for IncidentUpdateId {
    type Err = InvalidIdentifier;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(InvalidIdentifier)
    }
}

impl Serialize for IncidentUpdateId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for IncidentUpdateId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| D::Error::custom(format!("invalid update id: {text}")))
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
        assert_eq!(serde_json::to_string(&Identifier::from(0u64)).unwrap(), "\"AAAAAAAAAAA\"");
        assert!(serde_json::from_str::<Identifier>("\"//\"").is_err());
    }

    #[test]
    fn update_id_composes_and_decomposes() {
        let incident = Identifier::from(0x1234_5678_9abc_def0u64);
        let snowflake = 0x0fed_cba9_8765_4321u64;
        let id = IncidentUpdateId::compose(incident, snowflake);

        // High 64 bits recover the incident, low 64 the update half.
        assert_eq!(id.incident_id(), incident);
        assert_eq!(id.update_part(), snowflake);

        // Updates for the same incident share the high-bits prefix, so they form a contiguous range.
        let other = IncidentUpdateId::compose(incident, 1);
        assert_eq!(u128::from(other) >> 64, u128::from(id) >> 64);

        // serde + Display round-trip through the base64url form.
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<IncidentUpdateId>(&json).unwrap(), id);
        assert_eq!(IncidentUpdateId::parse(&id.to_string()), Some(id));
    }
}
