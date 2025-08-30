use std::{collections::HashMap, time::Instant};

use serde::{Deserialize, Serialize};

use crate::{Policy, targets::TargetType, utils::random_start_offset, validators::ValidatorType};

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
            tags: self.tags.clone(),
            sample_count: 0,
            successful_samples: 0,
            last_updated: chrono::DateTime::UNIX_EPOCH,
            history: Vec::new(),
            observers: 1,
        }
    }
}
