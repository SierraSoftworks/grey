use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::SampleValue;

use super::Validator;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OneOf(Vec<SampleValue>);

impl Validator for OneOf {
    fn validate(
        &self,
        field: &str,
        sample: &crate::SampleValue,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.0.contains(sample) {
            Ok(())
        } else {
            Err(format!(
                "The {} value '{}' did not match any of the expected values ([{}]).",
                field, sample, self.0.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
            )
            .into())
        }
    }
}

impl Display for OneOf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "in [")?;
        for (i, value) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", value)?;
        }
        write!(f, "]")
    }
}
