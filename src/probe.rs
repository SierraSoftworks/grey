use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::AtomicBool};

use serde::{Deserialize, Serialize};
use tracing_batteries::prelude::opentelemetry::trace::{
    SpanKind as OpenTelemetrySpanKind, Status as OpenTelemetryStatus,
};
use tracing_batteries::prelude::*;

use crate::{
    history::ProbeHistory,
    result::{ProbeResult, ValidationResult},
    targets::TargetType,
    validators::ValidatorType,
    Policy, Target, Validator,
};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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
    pub async fn run<const N: usize>(
        &self,
        history: Arc<ProbeHistory<N>>,
        cancel: &AtomicBool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut sample = ProbeResult::new();
        let total_attempts = self.policy.retries.unwrap_or(2);

        let result = tokio::time::timeout(self.policy.timeout, async {
            while sample.attempts < total_attempts
                && !cancel.load(std::sync::atomic::Ordering::Relaxed)
            {
                sample.attempts += 1;
                debug!(
                    "Running probe attempt {}/{}...",
                    sample.attempts, total_attempts,
                );
                match self.run_attempt(&mut sample, cancel).await {
                    Ok(res) => return Ok(res),
                    Err(err) => {
                        warn!("Probe failed: {}", err);
                        if sample.attempts == total_attempts {
                            error!("Probe failed after {} attempts: {}", sample.attempts, err);
                            return Err(err);
                        }
                    }
                }
            }

            Ok(())
        })
        .await;

        sample.duration = chrono::Utc::now() - sample.start_time;

        Span::current().record("probe.attempts", sample.attempts);

        let result = match result {
            Ok(Ok(_)) => {
                sample.pass = true;
                sample.message = "Probe completed successfully.".to_owned();
                Ok(())
            }
            Ok(Err(e)) => {
                sample.pass = false;
                sample.message = e.to_string();
                Err(e)
            }
            Err(_) => {
                sample.pass = false;
                sample.message = format!(
                    "Probe timed out after {} milliseconds.",
                    self.policy.timeout.as_millis()
                );

                Err(sample.message.clone().into())
            }
        };

        history.add_sample(sample);
        result
    }

    #[tracing::instrument(name = "probe.attempt", skip(self), err(Debug), fields(otel.kind=?OpenTelemetrySpanKind::Internal))]
    async fn run_attempt(
        &self,
        result: &mut ProbeResult,
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
                    result
                        .validations
                        .insert(path.to_owned(), ValidationResult::pass(validator));
                }
                Err(err) => {
                    span.record("otel.status_code", "Error")
                        .record("otel.status_message", &err.to_string());
                    error!(error = err, "{}", err);
                    result
                        .validations
                        .insert(path.to_owned(), ValidationResult::fail(validator, &err));
                    return Err(err);
                }
            }
        }

        Ok(())
    }
}
