use serde::{Deserialize, Serialize};
use std::{fmt::Display, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Policy {
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
    #[serde(default)]
    pub retries: Option<u8>,
}

impl Policy {
    /// The longest acceptable gap between consecutive samples before the probe's state
    /// coverage is considered interrupted (e.g. across a process restart) and its
    /// "since" tracking restarts. Allows for one missed round plus a fully retried run.
    pub fn max_sample_gap(&self) -> chrono::Duration {
        let retries = self.retries.unwrap_or(0) as u32;
        chrono::Duration::from_std(self.interval * 2 + self.timeout * (retries + 1))
            .unwrap_or_else(|_| chrono::Duration::hours(1))
    }
}

impl Display for Policy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "interval: {}, timeout: {}, retries: {}",
            humantime::format_duration(self.interval),
            humantime::format_duration(self.timeout),
            self.retries.unwrap_or(0)
        )
    }
}
