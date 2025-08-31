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
    #[cfg(test)]
    pub fn test() -> Self {
        Self {
            name: "test".into(),
            policy: crate::Policy { interval: std::time::Duration::from_secs(60), timeout: std::time::Duration::from_secs(5), retries: Some(3) },
            target: crate::targets::TargetType::test(),
            tags: HashMap::new(),
            validators: vec![("output.test".into(), crate::validators::ValidatorType::test())].into_iter().collect(),
        }
    }

    pub fn next_start_time(&self) -> Instant {
        Instant::now() + random_start_offset(self.policy.interval)
    }
}

impl Into<grey_api::Probe> for &Probe {
    fn into(self) -> grey_api::Probe {
        grey_api::Probe {
            name: self.name.clone(),
            tags: self.tags.clone(),
            last_updated: chrono::DateTime::UNIX_EPOCH,
            history: Vec::new(),
            observations: HashMap::new(),
        }
    }
}
