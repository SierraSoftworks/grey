#[macro_use]
extern crate lazy_static;
extern crate tracing_batteries;

use std::sync::atomic::AtomicBool;

use clap::Parser;

mod checks;
mod cluster;
mod config;
mod cron;
mod cron_monitor;
mod engine;
mod js;
#[macro_use]
mod macros;
mod notify;
mod policy;
mod probe;
mod probe_runner;
mod result;
mod sample;
mod serializers;
mod state;
mod targets;
mod api;
mod telemetry;
mod utils;

pub use config::Config;
pub use engine::Engine;
pub use policy::Policy;
pub use probe::Probe;
pub use sample::{Sample, SampleValue};
pub use targets::Target;

pub const HISTORY_SIZE: usize = 24;

static CANCEL: AtomicBool = AtomicBool::new(false);

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ctrlc::set_handler(|| {
    //     CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
    // })?;

    let args = Args::parse();

    let telemetry = tracing_batteries::Session::new("grey", version!("v"))
        .with_battery(
            tracing_batteries::OpenTelemetry::new("")
                .with_metrics()
                .with_logs(),
        )
        .with_battery(tracing_batteries::Analytics::new(
            "https://analytics.sierrasoftworks.com",
        ));

    let state = state::State::new(&args.config).await?;

    tracing::info!(
        name: "startup",
        {
            version = version!("v"),
            node.id = %state.node_id(),
            probes = state.get_config().probes.len(),
            crons = state.get_config().crons.len(),
            cluster.enabled = state.get_config().cluster.enabled,
            ui.enabled = state.get_config().ui.enabled,
        },
        "Starting Grey with {} probes...",
        state.get_config().probes.len()
    );

    let engine = Engine::new(state);
    let local_set = &mut tokio::task::LocalSet::new();
    let result = local_set.run_until(engine.run(&CANCEL)).await;

    tracing::info!(name: "shutdown", "Grey is shutting down.");
    telemetry.shutdown();
    result?;

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// The path to the configuration file which defines the probes to run.
    #[clap(short, long, value_parser)]
    config: String,
}
