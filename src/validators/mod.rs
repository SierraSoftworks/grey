use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::sample::SampleValue;

mod contains;
mod equals;
mod not_equals;
mod one_of;

pub trait Validator: Display {
    fn validate(&self, field: &str, value: &SampleValue) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidatorType {
    Contains(contains::Contains),
    Equals(equals::Equals),
    NotEquals(not_equals::NotEquals),
    OneOf(one_of::OneOf),
}

impl Validator for ValidatorType {
    fn validate(
        &self,
        field: &str,
        sample: &SampleValue,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            ValidatorType::Contains(validator) => validator.validate(field, sample),
            ValidatorType::Equals(validator) => validator.validate(field, sample),
            ValidatorType::NotEquals(validator) => validator.validate(field, sample),
            ValidatorType::OneOf(validator) => validator.validate(field, sample),
        }
    }
}

impl Display for ValidatorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidatorType::Contains(validator) => write!(f, "{}", validator),
            ValidatorType::Equals(validator) => write!(f, "{}", validator),
            ValidatorType::NotEquals(validator) => write!(f, "{}", validator),
            ValidatorType::OneOf(validator) => write!(f, "{}", validator),
        }
    }
}
