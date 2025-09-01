use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Mergeable, ProbeHistoryBucket};
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
        };

        let availability = probe.availability();
        assert_eq!(availability, (13.0 / 15.0) * 100.0);
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
        };
        
        let packed = rmp_serde::to_vec(&probe).unwrap();
        let unpacked: Probe = rmp_serde::from_slice(&packed).unwrap();
        assert_eq!(probe, unpacked);
    }
}