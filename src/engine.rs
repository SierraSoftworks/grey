use std::sync::{atomic::AtomicBool, Arc};

use futures::TryFutureExt;
use tokio::time::Instant;
use tracing_batteries::prelude::opentelemetry::trace::{
    SpanKind as OpenTelemetrySpanKind, Status as OpenTelemetryStatus,
};
use tracing_batteries::prelude::*;

use crate::config::ConfigProvider;
use crate::Probe;

pub struct Engine {
    config: ConfigProvider,
    probes: Vec<Arc<Probe>>,
}

const NO_PARENT: Option<tracing::Id> = None;

impl Engine {
    pub fn new(config: ConfigProvider) -> Self {
        let probes = config.probes();

        Self { config, probes }
    }

    #[tracing::instrument(name = "engine", skip(self), fields(otel.kind=?OpenTelemetrySpanKind::Internal), err(Debug))]
    pub async fn run(&self, cancel: &AtomicBool) -> Result<(), Box<dyn std::error::Error>> {
        let probe_future = futures::future::try_join_all(
            self.probes
                .iter()
                .cloned()
                .map(|probe| self.schedule(probe, cancel)),
        );

        if self.config.ui().enabled {
            eprintln!(
                "Starting web UI on http://{}",
                self.config.ui().listen.as_str()
            );

            let ui_future =
                crate::ui::start_server(self.config.clone(), self.probes.iter().cloned().collect());

            let config = self.config.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    if let Err(err) = config.reload().await {
                        error!("Failed to reload config: {}", err);
                    }
                }
            });

            let ui_future = Box::pin(ui_future.map_err(|e| Box::<dyn std::error::Error>::from(e)));
            let probe_future = Box::pin(probe_future);

            match futures::future::try_select(ui_future, probe_future).await {
                Ok(_) => {}
                Err(e) => {
                    let (e, _) = e.factor_first();
                    return Err(e);
                }
            }
        } else {
            probe_future.await?;
        }

        Ok(())
    }

    #[tracing::instrument(name = "engine.schedule", skip(self, probe), err(Debug), fields(
        otel.kind=?OpenTelemetrySpanKind::Producer,
        probe.name=probe.name,
        otel.status_code=?OpenTelemetryStatus::Unset,
        error=EmptyField
    ))]
    async fn schedule(
        &self,
        probe: Arc<Probe>,
        cancel: &AtomicBool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Calculate a random delay between 0 and the probe interval
        let start_delay = rand::random::<u128>() % probe.policy.interval.as_millis();
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

            next_run_time += probe.policy.interval;

            let probe_span = span!(parent: NO_PARENT, tracing::Level::INFO, "engine.probe",
                %probe.name,
                otel.name=probe.name,
                otel.status_code=?OpenTelemetryStatus::Unset,
                otel.kind=?OpenTelemetrySpanKind::Consumer,
            );

            probe_span.follows_from(&parent_span);

            debug!("Starting next probing session...");
            let run_result = probe.run(cancel).instrument(probe_span.clone()).await;
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
