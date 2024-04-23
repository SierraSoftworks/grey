#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate tracing;
#[macro_use]
extern crate tracing_attributes;

use clap::Parser;

mod config;
mod engine;
#[macro_use]
mod macros;
mod policy;
mod probe;
mod sample;
mod targets;
mod telemetry;
mod validators;

pub use config::Config;
pub use engine::Engine;
pub use policy::Policy;
pub use probe::Probe;
pub use sample::{Sample, SampleValue};
pub use targets::Target;
pub use validators::Validator;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    telemetry::setup();

    let config = config::load_config(&args.config).await?;

    println!("Starting Grey with {} probes...", config.probes.len());

    let engine = Engine::new(config);
    engine.run().await?;

    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// The path to the configuration file which defines the probes to run.
    #[clap(short, long, value_parser)]
    config: String,
}
