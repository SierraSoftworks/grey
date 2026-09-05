use grey_api::ValidationResult;
use std::{
    sync::{Arc, RwLock, atomic::AtomicBool},
    time::Instant,
};
use tracing_batteries::prelude::{opentelemetry::trace::Status as OpenTelemetryStatus, *};

use crate::{
    Probe, checks,
    result::ProbeResult,
    state::{ProbeStore, State},
    telemetry::{ProbeStatus, metrics},
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

        let node_id = self.state.node_id();
        let probe_name = self.probe_name.as_str();

        let result = match tokio::time::timeout(
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
                            sample.retries += 1;
                            sample.message = err.to_string();
                            if sample.retries >= total_attempts {
                                return Err(err);
                            }

                            // The attempt failed but the policy allows another: a retry is a
                            // warning (the probe may yet pass), and an error only once the
                            // attempts are exhausted.
                            warn!(
                                name: "probe.retry",
                                {
                                    probe.name = probe_name,
                                    probe.target = %probe.target,
                                    probe.attempt = sample.retries,
                                    probe.attempts = total_attempts,
                                    exception = err.as_ref(),
                                },
                                "Probe '{probe_name}' failed attempt {}/{} and will be retried: {err}",
                                sample.retries, total_attempts,
                            );
                            metrics().record_probe_retry(probe_name, &node_id);
                        }
                    }
                }

                Err("Probe was cancelled.".into())
        }).await {
            Ok(Ok(res)) => Ok(res),
            Ok(Err(err)) => Err((ProbeStatus::Fail, err)),
            // The timeout bounds the whole retry loop, so when it elapses the in-flight attempt
            // was dropped before its checks could be evaluated (and the retry counter can never
            // have reached the attempt limit — exhaustion returns through the arm above). The
            // probe therefore always fails here, however many attempts had completed.
            Err(_) => {
                let message = format!(
                    "Probe timed out after {} (attempt {}/{}).",
                    humantime::format_duration(probe.policy.timeout),
                    sample.retries + 1,
                    total_attempts
                );
                sample.message = message.clone();
                Err((ProbeStatus::Timeout, message.into()))
            }
        };


        Span::current().record("probe.attempts", sample.retries);

        let (status, result) = match result {
            Ok(_) => {
                sample.pass = true;
                sample.message = "Probe completed successfully.".to_owned();
                (ProbeStatus::Pass, Ok(()))
            }
            Err((status, e)) => {
                sample.pass = false;
                (status, Err(e))
            }
        };

        let sample = sample.finish();
        let latency = sample.duration.to_std().unwrap_or_default();
        // `retries` counts attempts which completed and failed. A timeout additionally cut short
        // the attempt that was in flight, so it counts towards the attempts made; a cancellation
        // before any attempt ran legitimately reports zero.
        let attempts_made = match status {
            ProbeStatus::Timeout => sample.retries + 1,
            ProbeStatus::Pass | ProbeStatus::Fail => sample.retries,
        };

        match &result {
            Ok(()) => {
                debug!(
                    name: "probe.result",
                    {
                        probe.name = probe_name,
                        probe.target = %probe.target,
                        probe.status = status.as_str(),
                        probe.retries = sample.retries,
                        probe.latency_ms = latency.as_millis() as u64,
                    },
                    "Probe '{probe_name}' passed.",
                );
            }
            Err(err) => {
                let failed_checks: Vec<String> = sample
                    .validations
                    .iter()
                    .filter(|(_, v)| !v.pass)
                    .map(|(check, v)| {
                        format!("{check}: {}", v.message.as_deref().unwrap_or_default())
                    })
                    .collect();

                error!(
                    name: "probe.result",
                    {
                        probe.name = probe_name,
                        probe.target = %probe.target,
                        probe.status = status.as_str(),
                        probe.retries = sample.retries,
                        probe.attempts_made = attempts_made,
                        probe.attempts = total_attempts,
                        probe.latency_ms = latency.as_millis() as u64,
                        probe.failed_checks = ?failed_checks,
                        exception = err.as_ref(),
                    },
                    "Probe '{probe_name}' failed after {attempts_made} attempt(s): {err}",
                );
            }
        }

        metrics().record_probe(probe_name, &node_id, status, latency);

        self.state
            .update_probe_state(probe_name, sample)
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

            // The check expression is the validations map key and the UI shows the pass/fail state,
            // so the public failure message carries only what those can't: the sample fields the
            // check consulted. The raw evaluation error is operator-only — it can expose internal
            // detail and the message is served publicly — so it is kept to telemetry alone.
            let (failure, otel_detail) = match check.matches(&sample) {
                Ok(true) => (None, None),
                Ok(false) => (Some(checks::unmatched_message(check, &sample)), None),
                Err(e) => (
                    Some(checks::evaluation_error_message(check, &sample)),
                    Some(e.to_string()),
                ),
            };

            match failure {
                None => {
                    span.record("otel.status_code", "Ok");
                    result
                        .validations
                        .insert(check.to_string(), ValidationResult::pass());
                }
                Some(message) => {
                    // Telemetry gets the public message plus any operator-only detail (the raw
                    // evaluation error); the status page (validation result + probe error) gets the
                    // public message alone.
                    let otel_message = match otel_detail {
                        Some(detail) => format!("{message} ({detail})"),
                        None => message.clone(),
                    };
                    span.record("otel.status_code", "Error")
                        .record("otel.status_message", otel_message.as_str());
                    // The attempt's outcome is reported by the caller (a warning when it will be
                    // retried, an error once attempts are exhausted), so the individual check
                    // failure is kept to debug to avoid double-reporting.
                    debug!(check = %check, "{otel_message}");
                    result
                        .validations
                        .insert(check.to_string(), ValidationResult::fail(&message));
                    let err: Box<dyn std::error::Error> = message.into();
                    return Err(err);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::TargetType;

    /// A probe whose target stalls past the policy timeout must be recorded as a failure. The
    /// timeout arm used to fall through to the success path (the retry counter can never have
    /// reached the attempt limit once the deadline drops the in-flight attempt), so a stalled
    /// probe — e.g. a DNS lookup against an unresponsive resolver — was stored as passing even
    /// though none of its checks had been evaluated.
    #[tokio::test]
    async fn timed_out_probe_is_marked_failed() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        let mut probe = state.get_config().probes[0].clone();
        probe.policy.timeout = std::time::Duration::from_millis(50);
        probe.target = TargetType::Hang;

        let runner = ProbeRunner::new(probe.clone(), state.clone());
        let result = runner.run_scheduled_execution().await;
        let err = result.expect_err("a timed-out probe must report an error").to_string();
        assert!(err.contains("timed out"), "unexpected error: {err}");

        let states = state.get_probe_states().await.unwrap();
        let stored = states.get(&probe.name).expect("the probe state to be stored");
        let bucket = stored.history.last().expect("a history bucket to be recorded");
        assert!(!bucket.pass, "a timed-out probe must be recorded as failing");
        assert!(bucket.message.contains("timed out"), "unexpected message: {}", bucket.message);
        assert!(
            bucket.validations.is_empty(),
            "no checks ran before the deadline, so no validations should be recorded"
        );
    }
}
