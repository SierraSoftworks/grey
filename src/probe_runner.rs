use chrono::TimeDelta;
use std::{
    sync::{atomic::AtomicBool, Arc, RwLock},
    time::Instant,
};
use tracing_batteries::prelude::{opentelemetry::trace::Status as OpenTelemetryStatus, *};

use crate::{
    history::ProbeHistory,
    result::{ProbeResult, ValidationResult},
    Probe, Validator,
};

const NO_PARENT: Option<tracing::Id> = None;

pub struct ProbeRunner<const N: usize> {
    probe_name: Arc<String>,
    config: Arc<RwLock<Probe>>,
    history: Arc<ProbeHistory<N>>,
    cancel: Arc<AtomicBool>,
}

impl<const N: usize> ProbeRunner<N> {
    pub fn new(config: Probe) -> Self {
        Self {
            probe_name: Arc::new(config.name.clone()),
            config: Arc::new(RwLock::new(config)),
            history: Arc::new(ProbeHistory::default()),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_snapshot_history<P: Into<std::path::PathBuf>>(
        config: Probe,
        snapshot_path: P,
    ) -> std::io::Result<Self> {
        let history = ProbeHistory::new()
            .with_max_state_age(TimeDelta::hours(1))
            .with_snapshot_interval(TimeDelta::seconds(60))
            .with_snapshot_file(snapshot_path)?;
        Ok(Self {
            probe_name: Arc::new(config.name.clone()),
            config: Arc::new(RwLock::new(config)),
            history: Arc::new(history),
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn name(&self) -> Arc<String> {
        self.probe_name.clone()
    }

    pub fn history(&self) -> Arc<ProbeHistory<N>> {
        self.history.clone()
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
            .record("probe.validators", debug(&probe.validators))
            .record("probe.tags", debug(&probe.tags));

        let result = async {
            while sample.attempts < total_attempts
                && !self.cancel.load(std::sync::atomic::Ordering::Relaxed)
            {
                sample.start_time = chrono::Utc::now();
                sample.attempts += 1;
                debug!(
                    "Running probe attempt {}/{}...",
                    sample.attempts, total_attempts,
                );
                match tokio::time::timeout(
                    probe.policy.timeout,
                    self.run_attempt(&probe, &mut sample),
                )
                .await
                {
                    Ok(Ok(res)) => return Ok(res),
                    Ok(Err(err)) => {
                        debug!("Probe failed: {}", err);
                        if sample.attempts == total_attempts {
                            warn!("Probe failed after {} attempts: {}", sample.attempts, err);
                            sample.message = err.to_string();
                            return Err(err);
                        }
                    }
                    Err(_) => {
                        debug!("Probe timed out");
                        if sample.attempts == total_attempts {
                            warn!("Probe timed out after {} attempts", sample.attempts);
                            sample.message = format!(
                                "Probe timed out after {} milliseconds.",
                                probe.policy.timeout.as_millis()
                            );
                            return Err(sample.message.clone().into());
                        }
                    }
                }
            }

            Ok(())
        }
        .await;

        sample.duration = chrono::Utc::now() - sample.start_time;

        Span::current().record("probe.attempts", sample.attempts);

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

        self.history.add_sample(sample);
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
        for (path, validator) in &probe.validators {
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
