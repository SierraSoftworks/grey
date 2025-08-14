use std::{collections::HashMap, time::Instant};

use serde::{Deserialize, Serialize};

use crate::{
    targets::TargetType,
    validators::ValidatorType,
    Policy,
};

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
        let start_delay = rand::random::<u128>()
            % self.policy.interval.as_millis();
        Instant::now() + std::time::Duration::from_millis(start_delay as u64)
    }
}
