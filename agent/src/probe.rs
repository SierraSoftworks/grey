use std::{collections::HashMap, time::Instant};

use serde::{Deserialize, Serialize};

use crate::{Policy, targets::TargetType, utils::random_start_offset};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Probe {
    pub name: String,
    pub policy: Policy,
    pub target: TargetType,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// `filt-rs` expression checks evaluated against the whole sample as the way
    /// to assert on a probe's result. Each expression is parsed at config-load
    /// time and reported as its own validation. A probe fails as soon as any one
    /// of its checks does not match.
    #[serde(default)]
    pub checks: Vec<filt_rs::Filter>,
}

impl Probe {
    #[cfg(test)]
    pub fn test() -> Self {
        Self {
            name: "test".into(),
            policy: crate::Policy { interval: std::time::Duration::from_secs(60), timeout: std::time::Duration::from_secs(5), retries: Some(3) },
            target: crate::targets::TargetType::test(),
            tags: HashMap::new(),
            checks: vec![filt_rs::Filter::new("output.test == true").unwrap()],
        }
    }

    pub fn next_start_time(&self) -> Instant {
        Instant::now() + random_start_offset(self.policy.interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
name: example
policy:
  interval: 5s
  timeout: 2s
  retries: 3
target: !Http
  url: https://example.com
"#;

    #[test]
    fn checks_default_to_empty() {
        let probe: Probe = serde_yaml::from_str(BASE).expect("deserialize probe");
        assert!(probe.checks.is_empty());
    }

    #[test]
    fn deserializes_checks_into_filters() {
        let yaml = format!(
            "{BASE}checks:\n  - http.status >= 200 && http.status < 300\n  - http.header.content-type contains \"html\"\n"
        );
        let probe: Probe = serde_yaml::from_str(&yaml).expect("deserialize probe");
        assert_eq!(probe.checks.len(), 2);
        assert_eq!(
            probe.checks[0].raw(),
            "http.status >= 200 && http.status < 300"
        );
    }

    #[test]
    fn invalid_check_expression_fails_to_deserialize() {
        let yaml = format!("{BASE}checks:\n  - \"http.status >\"\n");
        assert!(serde_yaml::from_str::<Probe>(&yaml).is_err());
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
            streak: grey_api::Streak::default(),
        }
    }
}
