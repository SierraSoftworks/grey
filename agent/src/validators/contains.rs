use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::sample::SampleValue;

use super::Validator;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Contains(SampleValue);

impl Validator for Contains {
    fn validate(
        &self,
        field: &str,
        sample: &crate::SampleValue,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
                    Err(format!(
                        "{} ([{}]) did not contain the item '{}'.",
                        field,
                        values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "),
                        item
                    ).into())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate() {
        let validator = Contains(SampleValue::String("world".to_string()));

        assert!(
            validator
                .validate("field", &SampleValue::String("hello world".to_string()))
                .is_ok()
        );
        assert!(
            validator
                .validate(
                    "field",
                    &SampleValue::List(vec![
                        SampleValue::String("hello".to_string()),
                        SampleValue::String("world".to_string())
                    ])
                )
                .is_ok()
        );
        assert!(
            validator
                .validate("field", &SampleValue::String("hello".to_string()))
                .is_err()
        );
        assert!(
            validator
                .validate(
                    "field",
                    &SampleValue::List(vec![
                        SampleValue::String("hello".to_string()),
                        SampleValue::String("worlds".to_string())
                    ])
                )
                .is_err()
        );
    }

    #[test]
    fn display() {
        let validator = Contains(SampleValue::String("world".to_string()));
        assert_eq!(format!("{}", validator), "contains \"world\"");
    }
}
