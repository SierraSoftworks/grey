use std::collections::HashMap;

use opentelemetry::trace::{SpanKind, Status};
use serde::{Deserialize, Serialize};
use tracing::{field, Span};

use crate::{targets::TargetType, validators::ValidatorType, Policy, Target, Validator};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probe {
    pub name: String,
    pub policy: Policy,
    pub target: TargetType,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub validators: HashMap<String, ValidatorType>,
}

impl Probe {
    #[instrument(name = "probe.run", skip(self), err(Value), fields(
        otel.name=self.name,
        probe.name=self.name,
        probe.policy.interval=?self.policy.interval,
        probe.policy.timeout=?self.policy.timeout,
        probe.policy.retries=%self.policy.retries.unwrap_or(2),
        probe.target=%self.target,
        probe.validators=?self.validators,
        probe.tags=?self.tags,
        probe.attempts=0,
    ))]
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let total_attempts = self.policy.retries.unwrap_or(2);
        let mut attempt_number = 0;

        let result = tokio::time::timeout(self.policy.timeout, async {
            while attempt_number < total_attempts {
                attempt_number += 1;
                info!(
                    "Running probe attempt {}/{}...",
                    attempt_number, total_attempts,
                );
                match self.run_attempt().await {
                    Ok(_) => {
                        break;
                    }
                    Err(err) => {
                        warn!("Probe failed: {}", err);
                        if attempt_number == total_attempts {
                            error!("Probe failed after {} attempts: {}", attempt_number, err);
                            return Err(err);
                        }
                    }
                }
            }

            Ok(())
        })
        .await;

        Span::current().record("probe.attempts", attempt_number);

        match result {
            Ok(res) => res,
            Err(_) => Err(format!(
                "Probe timed out after {} milliseconds.",
                self.policy.timeout.as_millis()
            )
            .into()),
        }
    }

    #[instrument(name = "probe.attempt", skip(self), err(Debug), fields(otel.kind=?SpanKind::Internal))]
    async fn run_attempt(&self) -> Result<(), Box<dyn std::error::Error>> {
        let sample = self.target.run().await?;
        debug!(?sample, "Probe sample collected successfully.");
        for (path, validator) in &self.validators {
            let name = format!("{} {}", path, validator);
            let span = info_span!(
                "probe.validate",
                otel.name=name,
                field=%path,
                validator=%validator,
                otel.status_code=?Status::Unset,
                otel.status_message=field::Empty
            )
            .entered();

            match validator.validate(path, sample.get(path)) {
                Ok(_) => {
                    span.record("otel.status_code", "Ok");
                }
                Err(err) => {
                    span.record("otel.status_code", "Error")
                        .record("otel.status_message", &err.to_string());
                    error!(error = err, "{}", err);
                    return Err(err);
                }
            }
        }

        Ok(())
    }
}
