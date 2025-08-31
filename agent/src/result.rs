use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use grey_api::ValidationResult;
use serde::{Deserialize, Serialize};

use crate::utils::{Elide, TimeAlignmentExt};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeResult {
    pub start_time: DateTime<Utc>,
    #[serde(with = "crate::serializers::chrono_duration_humantime")]
    pub duration: Duration,
    pub retries: u8,
    pub pass: bool,
    pub message: String,
    pub validations: HashMap<String, ValidationResult>,
}

impl Default for ProbeResult {
    fn default() -> Self {
        Self::new()
    }
}

impl ProbeResult {
    #[cfg(test)]
    pub fn test() -> Self {
        Self {
            start_time: Utc::now(),
            duration: Duration::zero(),
            retries: 0,
            pass: true,
            message: "Test probe".into(),
            validations: HashMap::new(),
        }
    }

    pub fn new() -> Self {
        Self {
            start_time: Utc::now(),
            duration: Duration::zero(),
            retries: 0,
            pass: false,
            message: String::new(),
            validations: HashMap::new(),
        }
    }

    pub fn finish(mut self) -> Self {
        self.duration = Utc::now() - self.start_time;
        self
    }

    pub fn apply<Id: ToString>(&self, node_id: Id, probe: &mut grey_api::Probe) {
        probe.last_updated = self.start_time + self.duration;

        let start_time = self.start_time.align(std::time::Duration::from_secs(3600));

        match probe.history.last_mut() {
            Some(last) if last.start_time == start_time => {
                if last.pass || !self.pass {
                    last.pass = self.pass;
                    last.message = self.message.clone();
                    last.validations = self
                        .validations
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone().elide(256)))
                        .collect();
                }

                let observation = last.observations.entry(node_id.to_string())
                    .or_insert_with(Default::default);

                observation.add_sample(self.pass, self.retries as u64, std::time::Duration::from_millis(self.duration.num_milliseconds() as u64));
            }
            _ => {
                probe.history.push(grey_api::ProbeHistoryBucket {
                    start_time,
                    pass: self.pass,
                    message: self.message.clone(),
                    validations: self
                        .validations
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone().elide(256)))
                        .collect(),
                    observations: vec![(
                        node_id.to_string(),
                        grey_api::Observation::from_sample(
                            self.pass,
                            self.retries as u64,
                            std::time::Duration::from_millis(self.duration.num_milliseconds() as u64)))]
                        .into_iter().collect(),
                });
            }
        }

        let observation = probe.observations.entry(node_id.to_string())
            .or_insert_with(Default::default);
        observation.add_sample(self.pass, self.retries as u64, std::time::Duration::from_millis(self.duration.num_milliseconds() as u64));
    }
}

impl Elide for ValidationResult {
    type Output = ValidationResult;
    fn elide(&self, len: usize) -> Self::Output {
        let mut vr = self.clone();
        if let Some(msg) = &vr.message {
            if msg.len() > len {
                vr.message = Some(format!("{}...", &msg[..len - 3]));
            }
        }
        vr
    }
}
