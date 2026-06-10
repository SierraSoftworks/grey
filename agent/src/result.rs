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

    pub fn apply<Id: ToString>(&self, node_id: Id, probe: &mut grey_api::Probe, max_gap: Duration) {
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

        probe.status
            .entry(node_id.to_string())
            .and_modify(|status| status.record(self.pass, sample_time, max_gap))
            .or_insert_with(|| grey_api::ProbeStatus::from_sample(self.pass, sample_time));
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
            status: HashMap::new(),
        }
    }

    /// The status streak must span history bucket boundaries: hours of uninterrupted
    /// passing samples report a single `since` at the start of the run.
    #[test]
    fn apply_maintains_status_across_buckets() {
        let mut probe = empty_probe();
        let max_gap = Duration::minutes(2);
        let start = Utc.with_ymd_and_hms(2026, 6, 7, 12, 0, 0).unwrap();

        for i in 0..180 {
            result_at(start + Duration::minutes(i), true).apply("node", &mut probe, max_gap);
        }

        assert!(probe.history.len() >= 3, "three hours of samples should span multiple buckets");
        let status = probe.status.get("node").expect("a status for the observing node");
        assert!(status.passing());
        assert_eq!(status.since(), Some(start));
        assert_eq!(status.failing_since, None);

        // A failure starts a failing streak, keeping the passing marker as history...
        let failed_at = start + Duration::minutes(180);
        result_at(failed_at, false).apply("node", &mut probe, max_gap);
        let status = probe.status.get("node").unwrap();
        assert!(status.failing());
        assert_eq!(status.since(), Some(failed_at));
        assert_eq!(status.passing_since, Some(start));

        // ...and a long gap in sampling (e.g. a process restart) resets both markers, so
        // the time we weren't watching is treated as unknown and the node rejoins as a
        // fresh observer whose history is repaired from its peers.
        let restarted_at = start + Duration::minutes(300);
        result_at(restarted_at, true).apply("node", &mut probe, max_gap);
        let status = probe.status.get("node").unwrap();
        assert!(status.passing());
        assert_eq!(status.since(), Some(restarted_at));
        assert_eq!(status.failing_since, None);
    }
}
