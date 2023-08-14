use opentelemetry::trace::{SpanKind, Status};
use tokio::time::Instant;
use tracing::{instrument, Span, Level, Id, field, Instrument};

use crate::{Config, Probe};

pub struct Engine {
    pub config: Config,
}

const NO_PARENT: Option<Id> = None;

impl Engine {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    #[instrument(name = "engine", skip(self), fields(otel.kind=?SpanKind::Internal), err(Debug))]
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        futures::future::join_all(self.config.probes.iter().map(|probe| self.schedule(probe)))
            .await
            .into_iter()
            .collect::<Result<Vec<()>, Box<dyn std::error::Error>>>()?;

        Ok(())
    }

    #[instrument(name = "engine.schedule", skip(self, probe), err(Debug), fields(
        otel.kind=?SpanKind::Producer,
        probe.name=probe.name,
        otel.status_code=?Status::Unset,
        error=field::Empty
    ))]
    async fn schedule(&self, probe: &Probe) -> Result<(), Box<dyn std::error::Error>> {
        // Calculate a random delay between 0 and the probe interval
        let start_delay = rand::random::<u128>() % probe.policy.interval.as_millis();
        let mut next_run_time = Instant::now() + std::time::Duration::from_millis(start_delay as u64);

        let parent_span = Span::current();

        loop {
            let now = Instant::now();
            if now < next_run_time {
                tokio::time::sleep(next_run_time - now).await;
            }

            next_run_time += probe.policy.interval;

            let probe_span = span!(parent: NO_PARENT, Level::INFO, "engine.probe",
                %probe.name,
                otel.name=probe.name,
                otel.status_code=?Status::Unset,
                otel.kind=?SpanKind::Consumer,
            );

            probe_span.follows_from(&parent_span);

            info!("Starting next probing session...");
            match probe.run().instrument(probe_span.clone()).await {
                Ok(_) => {
                    probe_span
                        .record("otel.status_code", "Ok");
                },
                Err(err) => {
                    probe_span
                        .record("otel.status_code", "Error")
                        .record("error", field::debug(&err));
                }
            }
        }
    }
}
