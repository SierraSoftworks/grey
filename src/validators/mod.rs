use std::fmt::Display;

use serde::{Serialize, Deserialize};

use crate::Sample;

mod content;
mod http_header;
mod http_status;

pub trait Validator: Display {
    fn validate(&self, sample: &Sample) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidatorType {
    HttpStatus(http_status::HttpStatus),
    HttpHeader(http_header::HttpHeader),
    Content(content::Content),
}

impl Validator for ValidatorType {
    fn validate(&self, sample: &Sample) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            ValidatorType::HttpStatus(validator) => validator.validate(sample),
            ValidatorType::HttpHeader(validator) => validator.validate(sample),
            ValidatorType::Content(validator) => validator.validate(sample),
        }
    }
}

impl Display for ValidatorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidatorType::HttpStatus(validator) => write!(f, "{}", validator),
            ValidatorType::HttpHeader(validator) => write!(f, "{}", validator),
            ValidatorType::Content(validator) => write!(f, "{}", validator),
        }
    }
}