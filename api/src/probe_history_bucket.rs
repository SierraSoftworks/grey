use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

use crate::Mergeable;

/// Probe result from the history endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ProbeHistoryBucket {
    #[serde(with = "chrono::serde::ts_seconds")]
    pub start_time: chrono::DateTime<chrono::Utc>,
    #[serde(with = "crate::serializers::duration_ms")]
    pub total_latency: std::time::Duration,
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

impl ProbeHistoryBucket {
    /// Calculate availability percentage based on successful vs total samples
    pub fn availability(&self) -> f64 {
        if self.sample_count == 0 {
            100.0
        } else {
            100.0 * self.successful_samples as f64 / self.sample_count as f64
        }
    }

    /// Calculates the average per-request latency for this time bucket.
    pub fn average_latency(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.total_latency.as_millis() as u64 / self.sample_count)
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

impl Mergeable for ProbeHistoryBucket {
    fn merge(&mut self, other: &Self) {
        self.total_latency += other.total_latency;
        self.attempts += other.attempts;
        self.sample_count += other.sample_count;
        self.successful_samples += other.successful_samples;

        if self.pass && !other.pass {
            self.pass = false;
            self.message = other.message.clone();
            self.validations = other.validations.clone();
        }
    }
}

/// Validation result within a probe result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ValidationResult {
    pub condition: String,
    pub pass: bool,
    pub message: Option<String>,
}

impl ValidationResult {
    pub fn pass<P: Display>(probe: P) -> Self {
        Self {
            condition: probe.to_string(),
            pass: true,
            message: None,
        }
    }

    pub fn fail<P: Display, M: ToString>(probe: P, message: M) -> Self {
        Self {
            condition: probe.to_string(),
            pass: false,
            message: Some(message.to_string()),
        }
    }

    pub fn with_message<M: ToString>(mut self, message: M) -> Self {
        self.message = Some(message.to_string());
        self
    }
}
