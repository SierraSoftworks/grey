use std::fmt::Display;

use serde::{Serialize, Deserialize};

use crate::sample::SampleValue;

use super::Validator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpStatus(Vec<i64>);

impl Validator for HttpStatus {
    #[instrument("validate.http_status", skip(self, sample), err(Raw))]
    fn validate(&self, sample: &crate::Sample) -> Result<(), Box<dyn std::error::Error>> {
        match sample.get("http.status") {
            SampleValue::Int(status) => {
                if self.0.contains(status) {
                    Ok(())
                } else {
                    Err(format!("Received status code {} but expected one of {:?}", status, self.0).into())
                }
            }
            _ => Err("Status code not found".into()),
        }
    }
}

impl Display for HttpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Status code is one of {:?}", self.0)
    }
}