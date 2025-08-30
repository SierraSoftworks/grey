use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use grey_api::{Mergeable, ValidationResult};
use serde::{Deserialize, Serialize};

use crate::utils::{Elide, TimeAlignmentExt};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeResult {
    pub start_time: DateTime<Utc>,
    #[serde(with = "crate::serializers::chrono_duration_humantime")]
    pub duration: Duration,
    pub attempts: u8,
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
    pub fn new() -> Self {
        Self {
            start_time: Utc::now(),
            duration: Duration::zero(),
            attempts: 0,
            pass: false,
            message: String::new(),
            validations: HashMap::new(),
        }
    }

    pub fn finish(mut self) -> Self {
        self.duration = Utc::now() - self.start_time;
        self
    }

    pub fn apply(&self, probe: &mut grey_api::Probe) {
        probe.last_updated = self.start_time + self.duration;
        probe.sample_count += 1;
        probe.successful_samples += if self.pass { 1 } else { 0 };

        let start_time = self.start_time.align(std::time::Duration::from_secs(3600));

        let new_bucket = grey_api::ProbeHistoryBucket {
            start_time,
            total_latency: std::time::Duration::from_millis(self.duration.num_milliseconds() as u64),
            attempts: self.attempts as u64,
            pass: self.pass,
            message: self.message.clone(),
            validations: self
                .validations
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().elide(256)))
                .collect(),
            sample_count: 1,
            successful_samples: if self.pass { 1 } else { 0 },
        };

        match probe.history.last_mut() {
            Some(last) if last.start_time == start_time => {
                last.merge(&new_bucket);
            }
            _ => {
                probe.history.push(new_bucket);
            }
        }
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
