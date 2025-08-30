use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Mergeable, ProbeHistoryBucket};

/// Raw probe data as returned by the /api/v1/probes endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Probe {
    pub name: String,

    #[serde(default)]
    pub policy: Option<Policy>,

    #[serde(default)]
    pub target: String,

    #[serde(default)]
    pub tags: HashMap<String, String>,

    #[serde(default)]
    pub validators: HashMap<String, String>,

    pub sample_count: u64,
    pub successful_samples: u64,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_updated: chrono::DateTime<chrono::Utc>,

    #[serde(default)]
    pub history: Vec<ProbeHistoryBucket>,

    // The number of unique observers which are running this probe
    pub observers: u32,
}

impl Probe {
    /// Calculate availability percentage based on successful vs total samples
    pub fn availability(&self) -> f64 {
        if self.sample_count == 0 {
            100.0
        } else {
            100.0 * self.successful_samples as f64 / self.sample_count as f64
        }
    }

    /// Calculate recent availability percentage based on successful vs total samples
    pub fn recent_availability(&self, max_hours: usize) -> f64 {
        let (samples, success) = self
            .history
            .iter()
            .filter(|h| {
                h.start_time > chrono::Utc::now() - chrono::Duration::hours(max_hours as i64)
            })
            .map(|h| (h.sample_count, h.successful_samples))
            .fold((0u64, 0u64), |acc, (samples, success)| {
                (acc.0 + samples, acc.1 + success)
            });

        if samples == 0 {
            100.0
        } else {
            100.0 * success as f64 / samples as f64
        }
    }
}

impl Mergeable for Probe {
    fn merge(&mut self, other: &Self) {
        if other.last_updated > self.last_updated {
            self.name = other.name.clone();
            self.policy = other.policy.clone();
            self.target = other.target.clone();
            self.tags = other.tags.clone();
            self.validators = other.validators.clone();
        }

        self.sample_count += other.sample_count;
        self.successful_samples += other.successful_samples;
        self.last_updated = self.last_updated.max(other.last_updated);
        self.observers += other.observers;

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
