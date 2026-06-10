use serde::{Deserialize, Serialize};
use crate::Mergeable;

/// Describes an aggregatable observation from a specific observer
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Observation {
    /// The total number of samples taken to form this observation.
    #[serde(rename = "total")]
    pub total_samples: u64,

    /// The number of samples which were successful in this observation.
    #[serde(rename = "success")]
    pub successful_samples: u64,

    /// The number of retries that were performed to get these samples.
    #[serde(rename = "retry")]
    pub total_retries: u64,

    /// The total time taken to acquire all samples, including retries.
    #[serde(rename = "latency")]
    #[serde(with = "crate::serializers::duration_ms")]
    pub total_latency: std::time::Duration,
}

impl Observation {
    pub fn from_sample(success: bool, retries: u64, latency: std::time::Duration) -> Self {
        let mut obs = Observation::default();
        obs.add_sample(success, retries, latency);
        obs
    }
    
    /// Adds a sample to this observation.
    pub fn add_sample(&mut self, success: bool, retries: u64, latency: std::time::Duration) {
        self.total_samples += 1;
        if success {
            self.successful_samples += 1;
        }
        self.total_retries += retries;
        self.total_latency += latency;
    }

    /// Calculates the success rate for samples in this observation.
    pub fn success_rate(&self) -> f64 {
        if self.total_samples == 0 {
            return 100.0;
        }

        100.0 * self.successful_samples as f64 / self.total_samples as f64
    }

    /// Calculates the retry rate for samples in this observation (as a percentage).
    pub fn retry_rate(&self) -> f64 {
        if self.total_samples == 0 {
            return 0.0;
        }

        100.0 * self.total_retries as f64 / self.total_samples as f64
    }

    /// Calculates the average per-sample latency for this observation.
    pub fn average_latency(&self) -> std::time::Duration {
        if self.successful_samples == 0 {
            return std::time::Duration::ZERO;
        }

        std::time::Duration::from_millis((self.total_latency.as_millis() / (self.total_samples as u128)) as u64)
    }
}

impl Mergeable for Observation {
    fn merge(&mut self, other: &Self) {
        self.total_samples += other.total_samples;
        self.successful_samples += other.successful_samples;
        self.total_retries += other.total_retries;
        self.total_latency += other.total_latency;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observation_merge() {
        let mut obs1 = Observation {
            total_samples: 10,
            successful_samples: 8,
            total_retries: 2,
            total_latency: std::time::Duration::from_millis(500),
        };

        let obs2 = Observation {
            total_samples: 5,
            successful_samples: 4,
            total_retries: 1,
            total_latency: std::time::Duration::from_millis(300),
        };

        obs1.merge(&obs2);

        assert_eq!(obs1.total_samples, 15);
        assert_eq!(obs1.successful_samples, 12);
        assert_eq!(obs1.total_retries, 3);
        assert_eq!(obs1.total_latency, std::time::Duration::from_millis(800));
    }

    #[test]
    fn test_observation_metrics() {
        let obs = Observation {
            total_samples: 10,
            successful_samples: 8,
            total_retries: 2,
            total_latency: std::time::Duration::from_millis(500),
        };

        assert_eq!(obs.success_rate(), 80.0);
        assert_eq!(obs.retry_rate(), 20.0);
        assert_eq!(obs.average_latency(), std::time::Duration::from_millis(50));
    }
    
    #[test]
    fn test_msgpack_roundtrip() {
        let obs = Observation {
            total_samples: 10,
            successful_samples: 8,
            total_retries: 2,
            total_latency: std::time::Duration::from_millis(500),
        };

        let packed = rmp_serde::to_vec(&obs).unwrap();
        let unpacked: Observation = rmp_serde::from_slice(&packed).unwrap();

        assert_eq!(obs, unpacked);
    }
}