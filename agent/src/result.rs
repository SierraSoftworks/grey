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
        let sample_time = self.start_time + self.duration;
        probe.last_updated = sample_time;

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

        probe.streak.observe(self.pass, sample_time);
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn result_at(start_time: DateTime<Utc>, pass: bool) -> ProbeResult {
        ProbeResult {
            start_time,
            duration: Duration::zero(),
            retries: 0,
            pass,
            message: String::new(),
            validations: HashMap::new(),
        }
    }

    fn empty_probe() -> grey_api::Probe {
        grey_api::Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: chrono::DateTime::UNIX_EPOCH,
            history: Vec::new(),
            observations: HashMap::new(),
            streak: grey_api::Streak::default(),
        }
    }

    /// The streak must span history bucket boundaries: hours of uninterrupted passing
    /// samples report a single coverage claim at the start of the run.
    #[test]
    fn apply_maintains_streak_across_buckets() {
        let mut probe = empty_probe();
        let start = Utc.with_ymd_and_hms(2026, 6, 7, 12, 0, 0).unwrap();

        for i in 0..180 {
            result_at(start + Duration::minutes(i), true).apply("node", &mut probe);
        }

        assert!(probe.history.len() >= 3, "three hours of samples should span multiple buckets");
        let sampled_until = start + Duration::minutes(179);
        assert!(probe.streak.passing_at(sampled_until));
        assert_eq!(probe.streak.since_at(sampled_until), Some(start));

        // A failure starts an episode immediately, and further failing samples refresh
        // it without moving the onset...
        let failed_at = start + Duration::minutes(180);
        result_at(failed_at, false).apply("node", &mut probe);
        result_at(failed_at + Duration::minutes(1), false).apply("node", &mut probe);
        assert!(probe.streak.failing_at(failed_at + Duration::minutes(1)));
        assert_eq!(probe.streak.since_at(failed_at + Duration::minutes(1)), Some(failed_at));

        // ...and once no failures have been observed for the recovery window, the probe
        // reads as passing since the last failing observation.
        let recovered = failed_at + Duration::minutes(1) + grey_api::Streak::recovery_window() + Duration::seconds(1);
        result_at(recovered, true).apply("node", &mut probe);
        assert!(probe.streak.passing_at(recovered));
        assert_eq!(probe.streak.since_at(recovered), Some(failed_at + Duration::minutes(1)));
    }
}
