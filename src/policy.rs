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
