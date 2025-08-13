use std::{
    sync::{atomic::AtomicBool, Arc, RwLock},
    time::Instant,
};
use tracing_batteries::prelude::{opentelemetry::trace::Status as OpenTelemetryStatus, *};

use crate::{history::ProbeHistory, Probe};

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

    #[tracing::instrument(name = "probe_runner.schedule", skip(self), err(Debug), fields(
        otel.kind=?OpenTelemetrySpanKind::Producer,
        probe.name=self.probe_name.as_str(),
        otel.status_code=?OpenTelemetryStatus::Unset,
        error=EmptyField
    ))]
    pub async fn schedule(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config = self.config.clone();
        let history = self.history.clone();
        let cancel = self.cancel.clone();

        cancel.store(false, std::sync::atomic::Ordering::Relaxed);

        // Calculate a random delay between 0 and the probe interval
        let start_delay = rand::random::<u128>()
            % config
                .read()
                .map_err(|e| format!("Failed to read probe config: {}", e))?
                .policy
                .interval
                .as_millis();
        let mut next_run_time =
            Instant::now() + std::time::Duration::from_millis(start_delay as u64);

        let parent_span = Span::current();

        while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let now = Instant::now();
            let sleep_time = next_run_time - now;
            if sleep_time > tokio::time::Duration::from_secs(1) {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            } else if sleep_time > tokio::time::Duration::from_secs(0) {
                tokio::time::sleep(sleep_time).await;
            }

            let probe = config
                .read()
                .map_err(|e| format!("Failed to read probe config: {}", e))?
                .clone();

            next_run_time += probe.policy.interval;

            let probe_span = span!(parent: NO_PARENT, tracing::Level::INFO, "engine.probe",
                %probe.name,
                otel.name=probe.name,
                otel.status_code=?OpenTelemetryStatus::Unset,
                otel.kind=?OpenTelemetrySpanKind::Consumer,
            );

            probe_span.follows_from(&parent_span);

            debug!("Starting next probing session...");
            let run_result = probe
                .run(history.clone(), &cancel)
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
}
