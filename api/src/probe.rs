
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw probe data as returned by the /api/v1/probes endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Probe {
    pub name: String,
    pub policy: Policy,
    pub target: String,
    pub tags: HashMap<String, String>,
    pub validators: HashMap<String, String>,
    pub availability: f64,
}

/// Probe policy information
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Policy {
    #[serde(with = "serde_duration_millis")]
    pub interval: std::time::Duration,
    pub retries: Option<u8>,
    #[serde(with = "serde_duration_millis")]
    pub timeout: std::time::Duration,
}

/// Probe result from the history endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ProbeResult {
    #[serde(with = "chrono::serde::ts_seconds")]
    pub start_time: chrono::DateTime<chrono::Utc>,
    #[serde(with = "serde_duration_millis")]
    pub duration: std::time::Duration,
    pub attempts: u8,
    pub pass: bool,
    pub message: String,
    pub validations: HashMap<String, ValidationResult>,
}

/// Validation result within a probe result
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ValidationResult {
    pub condition: String,
    pub pass: bool,
    pub message: Option<String>,
}

mod serde_duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}
