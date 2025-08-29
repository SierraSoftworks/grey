use std::{collections::HashMap, time::Instant};

use serde::{Deserialize, Serialize};

use crate::{targets::TargetType, utils::random_start_offset, validators::ValidatorType, Policy};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Probe {
    pub name: String,
    pub policy: Policy,
    pub target: TargetType,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub validators: HashMap<String, ValidatorType>,
}

impl Probe {
    pub fn next_start_time(&self) -> Instant {
        Instant::now() + random_start_offset(self.policy.interval)
    }
}

impl Into<grey_api::Probe> for &Probe {
    fn into(self) -> grey_api::Probe {
        grey_api::Probe {
            name: self.name.clone(),
            policy: grey_api::Policy {
                interval: std::time::Duration::from_millis(self.policy.interval.as_millis() as u64),
                retries: self.policy.retries,
                timeout: std::time::Duration::from_millis(self.policy.timeout.as_millis() as u64),
            },
            target: format!("{}", &self.target),
            tags: self.tags.clone(),
            validators: self
                .validators
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            sample_count: 0,
            successful_samples: 0,
            last_updated: chrono::DateTime::UNIX_EPOCH,
            history: Vec::new(),
            observers: 1,
        }
    }
}
