use std::fmt::Display;

use serde::{Serialize, Deserialize};
use tracing::{field, Span};

use crate::sample::SampleValue;

use super::Validator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHeader {
    name: String,
    value: String,
}

impl Validator for HttpHeader {
    #[instrument("validate.http_header", skip(self, sample), err(Raw), fields(
        header_name=self.name,
        expected_value=self.value,
        header_value=field::Empty,
    ))]
    fn validate(&self, sample: &crate::Sample) -> Result<(), Box<dyn std::error::Error>> {
        match sample.get(format!("http.header.{}", &self.name)) {
            SampleValue::String(value) => {
                Span::current()
                    .record("header_value", &value.as_str());
                if value == &self.value {
                    Ok(())
                } else {
                    Err(format!("The {} header was '{}' but we expected '{}'.", self.name, value, self.value).into())
                }
            }
            _ => Err(format!("Header {} not found", self.name).into()),
        }
    }
}

impl Display for HttpHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Header {} is '{}'", self.name, self.value)
    }
}