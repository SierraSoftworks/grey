use std::{collections::HashMap, sync::atomic::AtomicBool};

use serde::{Deserialize, Serialize};
use tracing_batteries::prelude::opentelemetry::trace::{
    SpanKind as OpenTelemetrySpanKind, Status as OpenTelemetryStatus,
};
use tracing_batteries::prelude::*;

use crate::{
    result::{ProbeResult, ValidationResult},
    targets::TargetType,
    validators::ValidatorType,
    Policy, Target, Validator,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Probe {
    pub name: String,
    pub policy: Policy,
    pub target: TargetType,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub validators: HashMap<String, ValidatorType>,

    #[serde(skip)]
    pub history: std::sync::RwLock<circular_buffer::CircularBuffer<100, ProbeResult>>,
}

impl Probe {
    #[tracing::instrument(name = "probe.run", skip(self), err(Display), fields(
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
    pub async fn run(&self, cancel: &AtomicBool) -> Result<(), Box<dyn std::error::Error>> {
        let mut history = ProbeResult::new();
        let total_attempts = self.policy.retries.unwrap_or(2);

        let result = tokio::time::timeout(self.policy.timeout, async {
            while history.attempts < total_attempts
                && !cancel.load(std::sync::atomic::Ordering::Relaxed)
            {
                history.attempts += 1;
                info!(
                    "Running probe attempt {}/{}...",
                    history.attempts, total_attempts,
                );
                match self.run_attempt(&mut history, cancel).await {
                    Ok(res) => return Ok(res),
                    Err(err) => {
                        warn!("Probe failed: {}", err);
                        if history.attempts == total_attempts {
                            error!("Probe failed after {} attempts: {}", history.attempts, err);
                            return Err(err);
                        }
                    }
                }
            }

            Ok(())
        })
        .await;

        Span::current().record("probe.attempts", history.attempts);

        let result = match result {
            Ok(Ok(_)) => {
                history.pass = true;
                history.message = "Probe completed successfully.".to_owned();
                Ok(())
            }
            Ok(Err(e)) => {
                history.pass = false;
                history.message = e.to_string();
                Err(e)
            }
            Err(_) => {
                history.pass = false;
                history.message = format!(
                    "Probe timed out after {} milliseconds.",
                    self.policy.timeout.as_millis()
                );

                Err(history.message.clone().into())
            }
        };

        self.history.write().unwrap().push_back(history);
        result
    }


    #[tracing::instrument(name = "probe.attempt", skip(self), err(Debug), fields(otel.kind=?OpenTelemetrySpanKind::Internal))]
    async fn run_attempt(
        &self,
        history: &mut ProbeResult,
        cancel: &AtomicBool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sample = self.target.run(cancel).await?;
        debug!(?sample, "Probe sample collected successfully.");
        for (path, validator) in &self.validators {
            let name = format!("{} {}", path, validator);
            let span = info_span!(
                "probe.validate",
                otel.name=name,
                field=%path,
                validator=%validator,
                otel.status_code=?OpenTelemetryStatus::Unset,
                otel.status_message=EmptyField
            )
            .entered();

            match validator.validate(path, sample.get(path)) {
                Ok(_) => {
                    span.record("otel.status_code", "Ok");
                    history
                        .validations
                        .insert(path.to_owned(), ValidationResult::pass(validator));
                }
                Err(err) => {
                    span.record("otel.status_code", "Error")
                        .record("otel.status_message", &err.to_string());
                    error!(error = err, "{}", err);
                    history
                        .validations
                        .insert(path.to_owned(), ValidationResult::fail(validator, &err));
                    return Err(err);
                }
            }
        }

        Ok(())
    }

    pub fn availability(&self) -> f64 {
        if let Ok(history) = self.history.read() {
            let total = history.len();
            let passed = history.iter().filter(|r| r.pass).count();
            100.0 * passed as f64 / total as f64
        } else {
            0.0
        }
    }
}
