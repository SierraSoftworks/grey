use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Mergeable, ProbeHistoryBucket, Streak};
use crate::observation::Observation;

/// Raw probe data as returned by the /api/v1/probes endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Probe {
    pub name: String,

    #[serde(default)]
    pub tags: HashMap<String, String>,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_updated: chrono::DateTime<chrono::Utc>,

    #[serde(default)]
    pub history: Vec<ProbeHistoryBucket>,

    /// Observations collected from this probe, keyed by observer ID
    #[serde(default)]
    pub observations: HashMap<String, Observation>,

    /// The cluster-converged record of this probe's pass/fail streaks
    #[serde(default)]
    pub streak: Streak,
}

impl Probe {
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

    /// This probe's cluster-converged streak record, or `None` when it carries no
    /// observations (e.g. data recorded by older agents).
    pub fn current_streak(&self) -> Option<&Streak> {
        (!self.streak.is_empty()).then_some(&self.streak)
    }

    /// Whether this probe is currently passing, falling back to the latest history
    /// bucket's result when the streak record carries no observations.
    pub fn passing(&self) -> bool {
        self.current_streak()
            .map(|s| s.passing())
            .unwrap_or_else(|| self.history.last().map(|h| h.pass).unwrap_or(true))
    }

    /// Calculate recent availability percentage based on successful vs total samples
    pub fn recent(&self, max_hours: usize) -> Observation {
        self
            .history
            .iter()
            .filter(|h| {
                h.start_time > chrono::Utc::now() - chrono::Duration::hours(max_hours as i64)
            })
            .map(|h| h.total())
            .fold(Observation::default(), |mut acc, obs| {
                acc.merge(&obs);
                acc
            })
    }
}

impl Mergeable for Probe {
    fn merge(&mut self, other: &Self) {
        if other.last_updated > self.last_updated {
            self.name = other.name.clone();
            self.tags = other.tags.clone();
        }

        self.last_updated = self.last_updated.max(other.last_updated);
        self.observations.extend(other.observations.clone());
        self.streak.join(&other.streak);

        let mut i = 0;
        let mut j = 0;

        while i < self.history.len() && j < other.history.len() {
            if self.history[i].start_time == other.history[j].start_time {
                self.history[i].merge(&other.history[j]);
                i += 1;
                j += 1;
            } else if self.history[i].start_time < other.history[j].start_time {
                i += 1;
            } else {
                self.history.insert(i, other.history[j].clone());
                i += 1;
                j += 1;
            }
        }

        while j < other.history.len() {
            self.history.push(other.history[j].clone());
            j += 1;
        }

        self.history
            .retain(|h| h.start_time > chrono::Utc::now() - chrono::Duration::hours(24 * 2));
    }
}

/// Probe policy information
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Policy {
    #[serde(with = "humantime_serde")]
    pub interval: std::time::Duration,

    #[serde(default)]
    pub retries: Option<u8>,

    #[serde(with = "humantime_serde")]
    pub timeout: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;
    use super::*;
    
    #[test]
    fn test_probe_merge() {
        let mut probe1 = Probe {
            name: "probe1".into(),
            tags: vec![("env".into(), "prod".into())].into_iter().collect(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: vec![("observer1".into(), Observation {
                total_samples: 10,
                successful_samples: 9,
                total_retries: 2,
                total_latency: std::time::Duration::from_secs(5),
            })].into_iter().collect(),
            streak: Streak::default(),
        };

        let probe2 = Probe {
            name: "probe2".into(),
            tags: vec![("env".into(), "staging".into())].into_iter().collect(),
            last_updated: chrono::Utc::now() + chrono::Duration::seconds(10),
            history: vec![],
            observations: vec![("observer2".into(), Observation {
                total_samples: 5,
                successful_samples: 4,
                total_retries: 1,
                total_latency: std::time::Duration::from_secs(3),
            })].into_iter().collect(),
            streak: Streak::default(),
        };

        probe1.merge(&probe2);

        assert_eq!(probe1.name, "probe2");
        assert_eq!(probe1.tags.get("env").unwrap(), "staging");
        assert_eq!(probe1.observations.len(), 2);
        assert_eq!(probe1.observations.get("observer1").unwrap().total_samples, 10);
        assert_eq!(probe1.observations.get("observer2").unwrap().total_samples, 5);
    }
    
    #[test]
    fn test_probe_total() {
        let probe = Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: vec![
                ("observer1".into(), Observation {
                    total_samples: 10,
                    successful_samples: 9,
                    total_retries: 2,
                    total_latency: std::time::Duration::from_secs(5),
                }),
                ("observer2".into(), Observation {
                    total_samples: 5,
                    successful_samples: 4,
                    total_retries: 1,
                    total_latency: std::time::Duration::from_secs(3),
                }),
            ].into_iter().collect(),
            streak: Streak::default(),
        };

        let total = probe.total();
        assert_eq!(total.total_samples, 15);
        assert_eq!(total.successful_samples, 13);
        assert_eq!(total.total_retries, 3);
        assert_eq!(total.total_latency, std::time::Duration::from_secs(8));
    }
    
    #[test]
    fn test_probe_availability() {
        let probe = Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: vec![
                ("observer1".into(), Observation {
                    total_samples: 10,
                    successful_samples: 9,
                    total_retries: 2,
                    total_latency: std::time::Duration::from_secs(5),
                }),
                ("observer2".into(), Observation {
                    total_samples: 5,
                    successful_samples: 4,
                    total_retries: 1,
                    total_latency: std::time::Duration::from_secs(3),
                }),
            ].into_iter().collect(),
            streak: Streak::default(),
        };

        let availability = probe.availability();
        assert_eq!(availability, (13.0 / 15.0) * 100.0);
    }
    
    #[test]
    fn test_probe_passing() {
        let now = chrono::Utc::now();
        let mut probe = Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: now,
            history: vec![],
            observations: HashMap::new(),
            streak: Streak::default(),
        };

        // With an empty streak record (e.g. data from older agents), the probe falls
        // back to the latest history bucket's result.
        assert_eq!(probe.current_streak(), None);
        assert!(probe.passing());
        probe.history.push(ProbeHistoryBucket {
            start_time: now,
            pass: false,
            message: "Timeout".into(),
            validations: HashMap::new(),
            observations: HashMap::new(),
        });
        assert!(!probe.passing());

        // A streak record with a long-standing coverage claim reports passing...
        probe.streak.observe(true, now - chrono::Duration::days(3));
        let streak = probe.current_streak().expect("a streak record");
        assert!(streak.passing_at(now));
        assert_eq!(streak.since_at(now), Some(now - chrono::Duration::days(3)));
        assert!(probe.passing());

        // ...until a failure is observed by any node, which wins immediately.
        probe.streak.observe(false, now - chrono::Duration::minutes(1));
        assert!(!probe.passing());
        assert_eq!(probe.streak.since_at(now), Some(now - chrono::Duration::minutes(1)));

        // Once no failures have been seen for the recovery window, the probe reads as
        // passing again, since the last failing observation.
        let later = now + Streak::recovery_window();
        assert!(probe.streak.passing_at(later));
        assert_eq!(probe.streak.since_at(later), Some(now - chrono::Duration::minutes(1)));
    }

    #[test]
    fn test_msgpack_roundtrip() {
        let probe = Probe {
            name: "probe".into(),
            tags: vec![("env".into(), "prod".into())].into_iter().collect(),
            last_updated: chrono::Utc::now().with_time(NaiveTime::from_hms_micro_opt(1, 2, 3, 0).unwrap()).unwrap(),
            history: vec![],
            observations: vec![("observer1".into(), Observation {
                total_samples: 10,
                successful_samples: 9,
                total_retries: 2,
                total_latency: std::time::Duration::from_secs(5),
            })].into_iter().collect(),
            streak: Streak {
                failing_since: Some(chrono::DateTime::from_timestamp(1_699_999_000, 0).unwrap()),
                failing_until: Some(chrono::DateTime::from_timestamp(1_699_999_900, 0).unwrap()),
                covered_since: Some(chrono::DateTime::from_timestamp(1_690_000_000, 0).unwrap()),
            },
        };

        let packed = rmp_serde::to_vec(&probe).unwrap();
        let unpacked: Probe = rmp_serde::from_slice(&packed).unwrap();
        assert_eq!(probe, unpacked);
    }

    #[test]
    fn test_decodes_legacy_probes() {
        // Probe records stored or gossiped by agents which pre-date streak tracking lack
        // the streak register; they must decode with an empty one in both wire formats.
        #[derive(Serialize)]
        struct LegacyProbe {
            name: String,
            tags: HashMap<String, String>,
            #[serde(with = "chrono::serde::ts_seconds")]
            last_updated: chrono::DateTime<chrono::Utc>,
            history: Vec<ProbeHistoryBucket>,
            observations: HashMap<String, Observation>,
        }

        let legacy = LegacyProbe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: chrono::Utc::now().with_time(NaiveTime::from_hms_opt(1, 2, 3).unwrap()).unwrap(),
            history: vec![],
            observations: vec![("observer1".into(), Observation {
                total_samples: 10,
                successful_samples: 9,
                total_retries: 2,
                total_latency: std::time::Duration::from_secs(5),
            })].into_iter().collect(),
        };

        for packed in [rmp_serde::to_vec(&legacy).unwrap(), rmp_serde::to_vec_named(&legacy).unwrap()] {
            let unpacked: Probe = rmp_serde::from_slice(&packed).unwrap();
            assert_eq!(unpacked.name, "probe");
            assert_eq!(unpacked.observations.len(), 1);
            assert!(unpacked.streak.is_empty());
            assert_eq!(unpacked.current_streak(), None);
        }
    }
}