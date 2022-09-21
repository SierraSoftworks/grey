use std::fmt::Display;

use serde::{Serialize, Deserialize};

use crate::sample::SampleValue;

use super::Validator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contains(SampleValue);

impl Validator for Contains {
    fn validate(&self, field: &str, sample: &crate::SampleValue) -> Result<(), Box<dyn std::error::Error>> {
        match (sample, &self.0) {
            (SampleValue::String(value), SampleValue::String(substr)) => {
                if value.contains(substr) {
                    Ok(())
                } else {
                    Err(format!("{} ('{}') did not contain the substring '{}'.", field, value, self.0).into())
                }
            }
            (SampleValue::List(values), item) => {
                if values.contains(item) {
                    Ok(())
                } else {
                    Err(format!("{} ('{:?}') did not contain the item '{}'.", field, values, item).into())
                }
            }
            _ => Err(format!("This validator is not compatible with fields of type '{}' and values of type '{}'.", sample.get_type(), self.0.get_type()).into()),
        }
    }
}

impl Display for Contains {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "contains {}", self.0)
    }
}