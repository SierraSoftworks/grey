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

    /// The most recent pass/fail state reported by any observer in this bucket, falling
    /// back to the bucket-level `pass` flag when no observer reports a state (data
    /// recorded by older agents).
    pub fn passing(&self) -> bool {
        let total = self.total();
        if total.has_state() {
            total.passing
        } else {
            self.pass
        }
    }

    /// The time at which this bucket's most recent state was entered, if any observer
    /// reported one.
    pub fn since(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let total = self.total();
        total.has_state().then_some(total.since)
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

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;
    use super::*;
    
    #[test]
    fn test_probe_history_bucket_merge() {
        let mut bucket1 = ProbeHistoryBucket {
            start_time: chrono::Utc::now(),
            pass: true,
            message: "".into(),
            validations: HashMap::new(),
            observations: vec![
                ("observer1".into(), Observation { total_samples: 5, successful_samples: 5, total_retries: 0, total_latency: std::time::Duration::from_millis(500), ..Default::default() }),
                ("observer2".into(), Observation { total_samples: 5, successful_samples: 4, total_retries: 1, total_latency: std::time::Duration::from_millis(600), ..Default::default() }),
            ].into_iter().collect(),
        };
        
        let bucket2 = ProbeHistoryBucket {
            start_time: chrono::Utc::now(),
            pass: false,
            message: "Timeout".into(),
            validations: vec![
                ("response_time".into(), ValidationResult::fail("response_time", "Exceeded threshold")),
            ].into_iter().collect(),
            observations: vec![
                ("observer2".into(), Observation { total_samples: 5, successful_samples: 3, total_retries: 2, total_latency: std::time::Duration::from_millis(700), ..Default::default() }),
                ("observer3".into(), Observation { total_samples: 5, successful_samples: 5, total_retries: 0, total_latency: std::time::Duration::from_millis(400), ..Default::default() }),
            ].into_iter().collect(),
        };
        
        bucket1.merge(&bucket2);
        assert!(!bucket1.pass);
        assert_eq!(bucket1.message, "Timeout");
        assert_eq!(bucket1.validations.len(), 1);
        assert_eq!(bucket1.observations.len(), 3);
        assert_eq!(bucket1.observations.get("observer1").unwrap().total_samples, 5);
        assert_eq!(bucket1.observations.get("observer2").unwrap().total_samples, 5); // from bucket1, not merged
        assert_eq!(bucket1.observations.get("observer3").unwrap().total_samples, 5);
    }
    
    #[test]
    fn test_probe_history_bucket_passing_and_since() {
        let mut bucket = ProbeHistoryBucket {
            start_time: chrono::Utc::now(),
            pass: false,
            message: "Timeout".into(),
            validations: HashMap::new(),
            observations: HashMap::new(),
        };

        // Without any observer state we fall back to the bucket-level pass flag.
        assert!(!bucket.passing());
        assert_eq!(bucket.since(), None);

        // A recovery reported by an observer takes precedence over both the bucket's
        // pass flag and the average success rate.
        let recovered_at = chrono::DateTime::from_timestamp(200, 0).unwrap();
        bucket.observations.insert("observer1".into(), Observation {
            total_samples: 10,
            successful_samples: 2,
            passing: true,
            since: recovered_at,
            ..Default::default()
        });
        assert!(bucket.passing());
        assert_eq!(bucket.since(), Some(recovered_at));
    }

    #[test]
    fn test_validation_result_constructors() {
        let pass_result = ValidationResult::pass("status_code_200");
        assert!(pass_result.pass);
        assert_eq!(pass_result.condition, "status_code_200");
        assert!(pass_result.message.is_none());
        
        let fail_result = ValidationResult::fail("status_code_200", "Received 500");
        assert!(!fail_result.pass);
        assert_eq!(fail_result.condition, "status_code_200");
        assert_eq!(fail_result.message.unwrap(), "Received 500");
        
        let updated_result = ValidationResult::pass("status_code_200").with_message("Received 404");
        assert_eq!(updated_result.message.unwrap(), "Received 404");
    }
    
    #[test]
    fn test_probe_history_bucket_metrics() {
        let bucket = ProbeHistoryBucket {
            start_time: chrono::Utc::now(),
            pass: true,
            message: "".into(),
            validations: HashMap::new(),
            observations: vec![
                ("observer1".into(), Observation { total_samples: 10, successful_samples: 8, total_retries: 2, total_latency: std::time::Duration::from_millis(1000), ..Default::default() }),
                ("observer2".into(), Observation { total_samples: 5, successful_samples: 5, total_retries: 0, total_latency: std::time::Duration::from_millis(300), ..Default::default() }),
            ].into_iter().collect(),
        };
        
        let total_obs = bucket.total();
        assert_eq!(total_obs.total_samples, 15);
        assert_eq!(total_obs.successful_samples, 13);
        assert_eq!(total_obs.total_retries, 2);
        assert_eq!(total_obs.total_latency, std::time::Duration::from_millis(1300));
        
        assert!((bucket.availability() - (13.0 / 15.0 * 100.0)).abs() < f64::EPSILON);
        assert!((bucket.retry_rate() - (2.0 / 15.0 * 100.0)).abs() < f64::EPSILON);
        assert_eq!(bucket.average_latency(), std::time::Duration::from_millis(86)); // 1300ms / 15 samples
    }
    
    #[test]
    fn test_msgpack_roundtrip() {
        let bucket = ProbeHistoryBucket {
            start_time: chrono::Utc::now().with_time(NaiveTime::from_hms_micro_opt(1, 2, 3, 0).unwrap()).unwrap(),
            pass: true,
            message: "All good".into(),
            validations: vec![
                ("status_code".into(), ValidationResult::pass("status_code")),
                ("response_time".into(), ValidationResult::fail("response_time", "Too slow")),
            ].into_iter().collect(),
            observations: vec![
                ("observer1".into(), Observation { total_samples: 10, successful_samples: 9, total_retries: 1, total_latency: std::time::Duration::from_millis(900), ..Default::default() }),
            ].into_iter().collect(),
        };
        
        let packed = rmp_serde::to_vec(&bucket).unwrap();
        let unpacked: ProbeHistoryBucket = rmp_serde::from_slice(&packed).unwrap();
        
        assert_eq!(bucket, unpacked);
    }
}