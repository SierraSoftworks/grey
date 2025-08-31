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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<u8>,

    #[serde(with = "humantime_serde")]
    pub timeout: std::time::Duration,
}
