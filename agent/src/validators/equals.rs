use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::SampleValue;

use super::Validator;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Equals(SampleValue);

impl Validator for Equals {
    fn validate(
        &self,
        field: &str,
        sample: &crate::SampleValue,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if sample == &self.0 {
            Ok(())
        } else {
            Err(format!(
                "The {} value '{}' did not match the expected value '{}'.",
                field, sample, self.0
            )
            .into())
        }
    }
}

impl Display for Equals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "== {}", self.0)
    }
}

impl From<SampleValue> for Equals {
    fn from(value: SampleValue) -> Self {
        Equals(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate() {
        let validator = Equals("test".into());
        let sample = SampleValue::String("test".to_string());
        let result = validator.validate("test", &sample);
        assert!(result.is_ok());
    }

    #[test]
    fn display() {
        let validator = Equals("test".into());
        assert_eq!(validator.to_string(), "== test");
    }
}
