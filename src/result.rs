use std::{collections::HashMap, fmt::Display};

use chrono::{DateTime, Duration, Utc};

#[derive(Debug)]
pub struct ProbeResult {
    pub start_time: DateTime<Utc>,
    pub duration: Duration,
    pub attempts: u8,
    pub pass: bool,
    pub message: String,
    pub validations: HashMap<String, ValidationResult>,
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
}

#[derive(Debug)]
pub struct ValidationResult {
    pub condition: String,
    pub pass: bool,
    pub message: Option<String>,
}

impl ValidationResult {
    pub fn pass<P: Display>(probe: P) -> Self {
        Self {
            condition: probe.to_string(),
            pass: true,
            message: None,
        }
    }

    pub fn fail<P: Display, M: ToString>(probe: P, message: M) -> Self {
        Self {
            condition: probe.to_string(),
            pass: false,
            message: Some(message.to_string()),
        }
    }

    pub fn with_message<M: ToString>(mut self, message: M) -> Self {
        self.message = Some(message.to_string());
        self
    }
}
