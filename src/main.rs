#[macro_use]
extern crate lazy_static;
extern crate tracing_batteries;

use clap::Parser;

mod config;
mod engine;
mod history;
#[macro_use]
mod macros;
mod policy;
mod probe;
mod result;
mod sample;
mod targets;
mod ui;
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

    let telemetry = tracing_batteries::Session::new("grey", version!("v"))
        .with_battery(tracing_batteries::OpenTelemetry::new(""))
        .with_battery(tracing_batteries::Medama::new("https://analytics.sierrasoftworks.com"));

    let config = config::load_config(&args.config).await?;

    println!("Starting Grey with {} probes...", config.probes.len());

    let engine = Engine::new(config);
    engine.run().await?;

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
