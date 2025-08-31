use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

use crate::Mergeable;
use crate::observation::Observation;

/// Probe result from the history endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ProbeHistoryBucket {
    #[serde(with = "chrono::serde::ts_seconds")]
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub pass: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub validations: HashMap<String, ValidationResult>,
    /// Observations collected from this probe, keyed by observer ID
    #[serde(default)]
    pub observations: HashMap<String, Observation>,
}

impl ProbeHistoryBucket {
    /// Aggregate all observations into a single total observation
    pub fn total(&self) -> Observation {
        self.observations.values().fold(Observation::default(), |mut acc, obs| {
            acc.merge(obs);
            acc
        })
    }

    /// Calculate availability percentage based on successful vs total samples
    pub fn availability(&self) -> f64 {
        self.total().success_rate()
    }
    
    pub fn max_availability(&self) -> f64 {
        if self.observations.is_empty() {
            return 100.0;
        }
        self.observations.values().map(|o| o.success_rate()).fold(f64::NAN, f64::max)
    }

    /// Calculates the average per-request latency for this time bucket.
    pub fn average_latency(&self) -> std::time::Duration {
        self.total().average_latency()
    }

    /// Calculate retry rate based on attempts (1 attempt = 0 retries, 2 attempts = 1 retry, etc.)
    pub fn retry_rate(&self) -> f64 {
        self.total().retry_rate()
    }
}

impl Mergeable for ProbeHistoryBucket {
    fn merge(&mut self, other: &Self) {
        if self.pass && !other.pass {
            self.pass = false;
            self.message = other.message.clone();
            self.validations = other.validations.clone();
        }
        
        self.observations.extend(other.observations.clone());
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
