use grey_api::ValidationResult;
use std::{
    sync::{Arc, RwLock, atomic::AtomicBool},
    time::Instant,
};
use tracing_batteries::prelude::{opentelemetry::trace::Status as OpenTelemetryStatus, *};

use crate::{
    Probe,
    checks::describe_failure,
    result::ProbeResult,
    state::{ProbeStore, State},
};

const NO_PARENT: Option<tracing::Id> = None;

pub struct ProbeRunner {
    probe_name: Arc<String>,
    config: Arc<RwLock<Probe>>,
    state: State,
    cancel: Arc<AtomicBool>,
}

impl ProbeRunner {
    pub fn new(config: Probe, state: State) -> Self {
        Self {
            probe_name: Arc::new(config.name.clone()),
            config: Arc::new(RwLock::new(config)),
            state,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn name(&self) -> Arc<String> {
        self.probe_name.clone()
    }

    pub fn update(&self, probe: Probe) {
        *self.config.write().unwrap() = probe;
    }

    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    #[tracing::instrument(name = "probe.schedule", skip(self), err(Debug), fields(
        otel.kind=?OpenTelemetrySpanKind::Producer,
        probe.name=self.probe_name.as_str(),
        otel.status_code=?OpenTelemetryStatus::Unset,
        error=EmptyField
    ))]
    pub async fn schedule(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let mut next_run_time = self
            .config
            .read()
            .map_err(|e| format!("Failed to read probe config: {}", e))?
            .next_start_time();

        let parent_span = Span::current();

        while !self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let now = Instant::now();
            let sleep_time = next_run_time - now;
            if sleep_time > tokio::time::Duration::from_secs(1) {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            } else if sleep_time > tokio::time::Duration::from_secs(0) {
                tokio::time::sleep(sleep_time).await;
            }

            let probe = self
                .config
                .read()
                .map_err(|e| format!("Failed to read probe config: {}", e))?
                .clone();

            next_run_time += probe.policy.interval;

            let probe_span = span!(parent: NO_PARENT, tracing::Level::INFO, "probe.schedule.run",
                %probe.name,
                otel.name=probe.name,
                otel.status_code=?OpenTelemetryStatus::Unset,
                otel.kind=?OpenTelemetrySpanKind::Consumer,
            );

            probe_span.follows_from(&parent_span);

            debug!("Starting next probing session...");
            let run_result = self
                .run_scheduled_execution()
                .instrument(probe_span.clone())
                .await;
            match run_result {
                Ok(_) => {
                    probe_span.record("otel.status_code", "Ok");
                }
                Err(err) => {
                    probe_span
                        .record("otel.status_code", "Error")
                        .record("error", debug(&err));
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(name = "probe.run", skip(self), err(Display), fields(
        otel.name=self.probe_name.as_str(),
        probe.name=self.probe_name.as_str(),
        probe.attempts=0,
    ))]
    async fn run_scheduled_execution(&self) -> Result<(), Box<dyn std::error::Error>> {
        let probe = self
            .config
            .read()
            .map_err(|e| format!("Failed to read probe config: {}", e))?
            .clone();

        let mut sample = ProbeResult::new();
        let total_attempts = probe.policy.retries.unwrap_or(2);

        // Update span with probe details
        Span::current()
            .record("probe.policy.interval", debug(&probe.policy.interval))
            .record("probe.policy.timeout", debug(&probe.policy.timeout))
            .record("probe.policy.retries", probe.policy.retries.unwrap_or(2))
            .record("probe.target", probe.target.to_string())
            .record("probe.checks", debug(&probe.checks))
            .record("probe.tags", debug(&probe.tags));

        let result = async {
            match tokio::time::timeout(
                probe.policy.timeout,
                async {
                    while !self.cancel.load(std::sync::atomic::Ordering::Relaxed)
                    {
                        sample.start_time = chrono::Utc::now();
                        debug!(
                            "Running probe attempt {}/{}...",
                            sample.retries + 1, total_attempts,
                        );
                        match self.run_attempt(&probe, &mut sample).await
                        {
                            Ok(res) => return Ok(res),
                            Err(err) => {
                                debug!("Probe failed: {}", err);
                                sample.retries += 1;
                                sample.message = err.to_string();
                                if sample.retries >= total_attempts {
                                    return Err(err);
                                }
                            }
                        }
                    }

                    Err("Probe was cancelled.".into())
            }).await {
                Ok(Ok(res)) => return Ok(res),
                Ok(Err(err)) => {
                    debug!("Probe failed: {}", err);
                    if sample.retries + 1 == total_attempts {
                        warn!("Probe failed after {} retries: {}", sample.retries, err);
                    }
                    return Err(err);
                }
                Err(err) => {
                    debug!("Probe timed out: {}", err);
                    if sample.retries == total_attempts {
                        warn!("Probe timed out after {} retries: {}", sample.retries, err);
                        if sample.message.is_empty() {
                            sample.message = format!("Probe timed out after {} retries .", sample.retries);
                        }
                        return Err(format!("Probe timed out after {} retries.", sample.retries).into());
                    }
                }
            }

            Ok(())
        }
        .await;


        Span::current().record("probe.attempts", sample.retries);

        let result = match result {
            Ok(_) => {
                sample.pass = true;
                sample.message = "Probe completed successfully.".to_owned();
                Ok(())
            }
            Err(e) => {
                sample.pass = false;
                Err(e)
            }
        };

        self.state
            .update_probe_state(self.name().as_str(), sample.finish())
            .await?;
        result
    }

    #[tracing::instrument(name = "probe.attempt", skip(self), err(Debug), fields(otel.kind=?OpenTelemetrySpanKind::Internal))]
    async fn run_attempt(
        &self,
        probe: &Probe,
        result: &mut ProbeResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sample = probe.target.run(&self.cancel).await?;
        debug!(?sample, "Probe sample collected successfully.");

        for check in &probe.checks {
            let name = format!("check {}", check);
            let span = info_span!(
                "probe.validate",
                otel.name=name,
                check=%check,
                otel.status_code=?OpenTelemetryStatus::Unset,
                otel.status_message=EmptyField
            )
            .entered();

            // On failure, `describe_failure` builds a `(probe-level message, per-check detail)`
            // pair: the probe-level message names the check for top-line context, while the detail
            // stored in the validation result omits it (the check expression is the map key) and
            // both append the sample fields the check consulted.
            let failure = match check.matches(&sample) {
                Ok(true) => None,
                Ok(false) => Some(describe_failure(check, &sample, "did not pass".to_string())),
                Err(e) => Some(describe_failure(
                    check,
                    &sample,
                    format!("could not be evaluated: {e}"),
                )),
            };

            match failure {
                None => {
                    span.record("otel.status_code", "Ok");
                    result
                        .validations
                        .insert(check.to_string(), ValidationResult::pass());
                }
                Some((probe_message, detail)) => {
                    span.record("otel.status_code", "Error")
                        .record("otel.status_message", detail.as_str());
                    let err: Box<dyn std::error::Error> = probe_message.into();
                    error!(error = err, "{}", err);
                    result
                        .validations
                        .insert(check.to_string(), ValidationResult::fail(detail));
                    return Err(err);
                }
            }
        }

        Ok(())
    }
}
