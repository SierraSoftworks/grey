use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Mergeable, ProbeHistoryBucket, ProbeStatus};
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

    /// The current state of this probe as reported by each observer, keyed by observer ID
    #[serde(default)]
    pub status: HashMap<String, ProbeStatus>,
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

    /// The current state of this probe aggregated across all live observers: an active
    /// failure reported by any observer wins, while passing observers converge on the
    /// streak everyone can agree on — observers which witnessed the last failure vote on
    /// when the probe recovered, and the rest pool their coverage. Returns `None` when
    /// no observer has confirmed a status recently (e.g. data recorded by older agents).
    pub fn current_status(&self) -> Option<ProbeStatus> {
        self.current_status_at(chrono::Utc::now())
    }

    fn current_status_at(&self, now: chrono::DateTime<chrono::Utc>) -> Option<ProbeStatus> {
        self.status
            .values()
            .filter(|s| s.is_current(now))
            .fold(None, |acc, status| match acc {
                Some(mut merged) => {
                    merged.merge(status);
                    Some(merged)
                }
                None => Some(status.clone()),
            })
    }

    /// Whether this probe is currently passing, falling back to the latest history
    /// bucket's result when no observer reports a current status.
    pub fn passing(&self) -> bool {
        self.current_status()
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
        self.status.extend(other.status.clone());

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
    use crate::Streak;
    
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
            status: HashMap::new(),
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
            status: HashMap::new(),
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
            status: HashMap::new(),
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
            status: HashMap::new(),
        };

        let availability = probe.availability();
        assert_eq!(availability, (13.0 / 15.0) * 100.0);
    }
    
    #[test]
    fn test_probe_current_status() {
        let now = chrono::Utc::now();
        let mut probe = Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: now,
            history: vec![],
            observations: HashMap::new(),
            status: HashMap::new(),
        };

        // With no reported statuses (e.g. data from older agents), there is no current
        // status and the probe falls back to the latest history bucket's result.
        assert_eq!(probe.current_status(), None);
        assert!(probe.passing());
        probe.history.push(ProbeHistoryBucket {
            start_time: now,
            pass: false,
            message: "Timeout".into(),
            validations: HashMap::new(),
            observations: HashMap::new(),
        });
        assert!(!probe.passing());

        // A node which restarted recently has its unknown coverage repaired by a node
        // which has watched the probe pass for longer.
        probe.status.insert("restarted".into(), ProbeStatus::from_sample(true, now - chrono::Duration::minutes(5)));
        probe.status.insert("continuous".into(), ProbeStatus {
            observed: Streak { passing_since: Some(now - chrono::Duration::days(3)), failing_since: None },
            converged: Streak { passing_since: Some(now - chrono::Duration::days(3)), failing_since: None },
            updated: now,
        });
        let status = probe.current_status().expect("a current status");
        assert!(status.passing());
        assert_eq!(status.since(), Some(now - chrono::Duration::days(3)));
        assert!(probe.passing());

        // A failure reported by any live observer wins over the passing reports.
        probe.status.insert("failing".into(), ProbeStatus {
            observed: Streak {
                passing_since: Some(now - chrono::Duration::days(3)),
                failing_since: Some(now - chrono::Duration::minutes(30)),
            },
            converged: Streak {
                passing_since: Some(now - chrono::Duration::days(3)),
                failing_since: Some(now - chrono::Duration::minutes(30)),
            },
            updated: now,
        });
        let status = probe.current_status().expect("a current status");
        assert!(status.failing());
        assert_eq!(status.since(), Some(now - chrono::Duration::minutes(30)));
        assert!(!probe.passing());

        // ...but once that observer stops reporting, its stale status no longer counts.
        probe.status.get_mut("failing").unwrap().updated = now - chrono::Duration::hours(2);
        let status = probe.current_status().expect("a current status");
        assert!(status.passing());
        assert_eq!(status.since(), Some(now - chrono::Duration::days(3)));
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
            status: vec![("observer1".into(), ProbeStatus {
                observed: Streak {
                    passing_since: Some(chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap()),
                    failing_since: Some(chrono::DateTime::from_timestamp(1_699_999_000, 0).unwrap()),
                },
                converged: Streak {
                    passing_since: Some(chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap()),
                    failing_since: Some(chrono::DateTime::from_timestamp(1_699_999_000, 0).unwrap()),
                },
                updated: chrono::DateTime::from_timestamp(1_700_000_060, 0).unwrap(),
            })].into_iter().collect(),
        };

        let packed = rmp_serde::to_vec(&probe).unwrap();
        let unpacked: Probe = rmp_serde::from_slice(&packed).unwrap();
        assert_eq!(probe, unpacked);
    }

    #[test]
    fn test_decodes_legacy_probes() {
        // Probe records stored or gossiped by agents which pre-date status tracking lack
        // the status map; they must decode with an empty one in both wire formats.
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
            assert!(unpacked.status.is_empty());
            assert_eq!(unpacked.current_status(), None);
        }
    }
}