use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw probe data as returned by the /api/v1/probes endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Probe {
    pub name: String,
    pub policy: Policy,
    pub target: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub validators: HashMap<String, String>,
    pub availability: f64,
}

/// Probe policy information
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Policy {
    #[serde(with = "serde_duration_millis")]
    pub interval: std::time::Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<u8>,
    #[serde(with = "serde_duration_millis")]
    pub timeout: std::time::Duration,
}

/// Probe result from the history endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ProbeHistory {
    #[serde(with = "chrono::serde::ts_seconds")]
    pub start_time: chrono::DateTime<chrono::Utc>,
    #[serde(with = "serde_duration_millis")]
    pub latency: std::time::Duration,
    /// Duration this state was active (for UI weighting)
    #[serde(with = "serde_duration_millis")]
    pub state_duration: std::time::Duration,
    pub attempts: u64,
    pub pass: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub validations: HashMap<String, ValidationResult>,
    /// Number of samples this entry represents (1 for recent, >1 for compressed)
    pub sample_count: u64,
    /// Number of successful samples within this entry
    pub successful_samples: u64,
}

impl ProbeHistory {
    /// Calculate availability percentage based on successful vs total samples
    pub fn availability(&self) -> f64 {
        if self.sample_count == 0 {
            100.0
        } else {
            100.0 * self.successful_samples as f64 / self.sample_count as f64
        }
    }

    /// Calculate retry rate based on attempts (1 attempt = 0 retries, 2 attempts = 1 retry, etc.)
    pub fn retry_rate(&self) -> f64 {
        if self.sample_count == 0 {
            0.0
        } else {
            100.0 * (self.attempts - self.sample_count) as f64 / self.sample_count as f64
        }
    }
}

/// Validation result within a probe result
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ValidationResult {
    pub condition: String,
    pub pass: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
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
