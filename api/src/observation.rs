use chrono::{DateTime, Utc};
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

    /// Whether the most recent sample in this observation was successful.
    #[serde(default)]
    pub passing: bool,

    /// The time at which this observation entered its current passing/failing state.
    /// Remains at the Unix epoch for observations recorded by agents which pre-date
    /// state tracking (see [`Observation::has_state`]).
    #[serde(default, with = "chrono::serde::ts_milliseconds")]
    pub since: DateTime<Utc>,
}

impl Observation {
    pub fn from_sample(success: bool, retries: u64, latency: std::time::Duration, timestamp: DateTime<Utc>) -> Self {
        let mut obs = Observation::default();
        obs.add_sample(success, retries, latency, timestamp);
        obs
    }

    /// Adds a sample to this observation.
    pub fn add_sample(&mut self, success: bool, retries: u64, latency: std::time::Duration, timestamp: DateTime<Utc>) {
        // The first sample establishes the current state; subsequent samples only move it
        // when the result flips, so `since` marks the most recent state transition.
        if self.total_samples == 0 || (success != self.passing && timestamp >= self.since) {
            self.passing = success;
            self.since = timestamp;
        }

        self.total_samples += 1;
        if success {
            self.successful_samples += 1;
        }
        self.total_retries += retries;
        self.total_latency += latency;
    }

    /// Whether this observation carries a known most-recent state. Observations recorded
    /// by older agents only carry aggregate counters and leave `since` at the Unix epoch.
    pub fn has_state(&self) -> bool {
        self.since > DateTime::UNIX_EPOCH
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
        // Most-recent-wins for the current state: whichever side transitioned most recently
        // dictates the merged passing/since pair, with ties broken pessimistically so a
        // simultaneous failure is surfaced rather than hidden.
        if other.has_state()
            && (!self.has_state()
                || other.since > self.since
                || (other.since == self.since && !other.passing))
        {
            self.passing = other.passing;
            self.since = other.since;
        }

        self.total_samples += other.total_samples;
        self.successful_samples += other.successful_samples;
        self.total_retries += other.total_retries;
        self.total_latency += other.total_latency;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn test_observation_merge() {
        let mut obs1 = Observation {
            total_samples: 10,
            successful_samples: 8,
            total_retries: 2,
            total_latency: std::time::Duration::from_millis(500),
            passing: false,
            since: ts(100),
        };

        let obs2 = Observation {
            total_samples: 5,
            successful_samples: 4,
            total_retries: 1,
            total_latency: std::time::Duration::from_millis(300),
            passing: true,
            since: ts(200),
        };

        obs1.merge(&obs2);

        assert_eq!(obs1.total_samples, 15);
        assert_eq!(obs1.successful_samples, 12);
        assert_eq!(obs1.total_retries, 3);
        assert_eq!(obs1.total_latency, std::time::Duration::from_millis(800));

        // The most recent state transition wins.
        assert!(obs1.passing);
        assert_eq!(obs1.since, ts(200));
    }

    #[test]
    fn test_observation_merge_most_recent_wins() {
        // An older state on the other side doesn't override a newer local one.
        let mut newer = Observation { passing: true, since: ts(300), total_samples: 1, successful_samples: 1, ..Default::default() };
        let older = Observation { passing: false, since: ts(200), total_samples: 1, ..Default::default() };
        newer.merge(&older);
        assert!(newer.passing);
        assert_eq!(newer.since, ts(300));

        // A tie is broken pessimistically (failing wins).
        let mut tied = Observation { passing: true, since: ts(300), total_samples: 1, successful_samples: 1, ..Default::default() };
        tied.merge(&Observation { passing: false, since: ts(300), total_samples: 1, ..Default::default() });
        assert!(!tied.passing);

        // A stateless observation (from an older agent) never overrides a known state.
        let mut known = Observation { passing: true, since: ts(300), total_samples: 1, successful_samples: 1, ..Default::default() };
        known.merge(&Observation { total_samples: 5, successful_samples: 0, ..Default::default() });
        assert!(known.passing);
        assert_eq!(known.since, ts(300));

        // ...but a known state fills in a stateless one.
        let mut stateless = Observation { total_samples: 5, successful_samples: 5, ..Default::default() };
        stateless.merge(&Observation { passing: false, since: ts(100), total_samples: 1, ..Default::default() });
        assert!(!stateless.passing);
        assert_eq!(stateless.since, ts(100));
    }

    #[test]
    fn test_add_sample_tracks_state_transitions() {
        let mut obs = Observation::from_sample(true, 0, std::time::Duration::from_millis(10), ts(100));
        assert!(obs.passing);
        assert_eq!(obs.since, ts(100));
        assert!(obs.has_state());

        // Repeating the same state keeps the original transition time.
        obs.add_sample(true, 0, std::time::Duration::from_millis(10), ts(200));
        assert!(obs.passing);
        assert_eq!(obs.since, ts(100));

        // A flip moves the transition time forward.
        obs.add_sample(false, 1, std::time::Duration::from_millis(10), ts(300));
        assert!(!obs.passing);
        assert_eq!(obs.since, ts(300));

        // An out-of-order older sample doesn't rewind the state.
        obs.add_sample(true, 0, std::time::Duration::from_millis(10), ts(250));
        assert!(!obs.passing);
        assert_eq!(obs.since, ts(300));
    }

    #[test]
    fn test_observation_metrics() {
        let obs = Observation {
            total_samples: 10,
            successful_samples: 8,
            total_retries: 2,
            total_latency: std::time::Duration::from_millis(500),
            ..Default::default()
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
            passing: true,
            since: ts(1_700_000_000),
        };

        let packed = rmp_serde::to_vec(&obs).unwrap();
        let unpacked: Observation = rmp_serde::from_slice(&packed).unwrap();

        assert_eq!(obs, unpacked);
    }

    #[test]
    fn test_decodes_legacy_observations() {
        // Observations stored or gossiped by older agents lack the passing/since fields;
        // they must decode with a stateless default in both wire formats.
        #[derive(Serialize)]
        struct LegacyObservation {
            total: u64,
            success: u64,
            retry: u64,
            latency: u64,
        }

        let legacy = LegacyObservation { total: 10, success: 8, retry: 2, latency: 500 };

        for packed in [rmp_serde::to_vec(&legacy).unwrap(), rmp_serde::to_vec_named(&legacy).unwrap()] {
            let unpacked: Observation = rmp_serde::from_slice(&packed).unwrap();
            assert_eq!(unpacked.total_samples, 10);
            assert_eq!(unpacked.successful_samples, 8);
            assert_eq!(unpacked.total_retries, 2);
            assert_eq!(unpacked.total_latency, std::time::Duration::from_millis(500));
            assert!(!unpacked.has_state());
        }

        let json: Observation = serde_json::from_str(r#"{"total":10,"success":8,"retry":2,"latency":500}"#).unwrap();
        assert!(!json.has_state());
    }
}
