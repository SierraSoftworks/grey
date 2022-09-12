use std::fmt::Display;

use serde::{Serialize, Deserialize};

use crate::sample::SampleValue;

use super::Validator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content(String);

impl Validator for Content {
    #[instrument("validate.content", skip(self, sample), err)]
    fn validate(&self, sample: &crate::Sample) -> Result<(), Box<dyn std::error::Error>> {
        match sample.get("content") {
            SampleValue::String(value) => {
                if value.contains(&self.0) {
                    Ok(())
                } else {
                    Err(format!("Response body did not contain the expected pattern '{}'.", self.0).into())
                }
            }
            _ => Err("The probe target did not record a content field, this validator is not compatible.".into()),
        }
    }
}

impl Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Content contains '{}'", self.0)
    }
}