#[macro_use]
extern crate lazy_static;
extern crate tracing_batteries;

use std::sync::atomic::AtomicBool;

use clap::Parser;

mod config;
mod engine;
mod history;
mod js;
#[macro_use]
mod macros;
mod policy;
mod probe;
mod probe_runner;
mod result;
mod sample;
mod serializers;
mod state;
mod targets;
mod ui;
mod utils;
mod validators;

pub use config::Config;
pub use engine::Engine;
pub use policy::Policy;
pub use probe::Probe;
pub use sample::{Sample, SampleValue};
pub use targets::Target;
pub use validators::Validator;

pub const HISTORY_SIZE: usize = 24;

static CANCEL: AtomicBool = AtomicBool::new(false);

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ctrlc::set_handler(|| {
    //     CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
    // })?;

    let args = Args::parse();

    let telemetry = tracing_batteries::Session::new("grey", version!("v"))
        .with_battery(tracing_batteries::OpenTelemetry::new(""))
        .with_battery(tracing_batteries::Medama::new(
            "https://analytics.sierrasoftworks.com",
        ));

    let state = state::State::new(&args.config).await?;

    println!("Starting Grey with {} probes...", state.get_config().probes.len());

    let engine = Engine::new(state);
    let local_set = &mut tokio::task::LocalSet::new();
    local_set.run_until(engine.run(&CANCEL)).await?;

    telemetry.shutdown();

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// The path to the configuration file which defines the probes to run.
    #[clap(short, long, value_parser)]
    config: String,
}
