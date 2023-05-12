use opentelemetry_otlp::WithExportConfig;
use tracing::Collect;
use tracing_subscriber::{prelude::*, registry::LookupSpan, Subscribe};

pub fn setup() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::DEBUG)
        .with(tracing_subscriber::filter::dynamic_filter_fn(
            |_metadata, ctx| {
                !ctx.lookup_current()
                    // Exclude the rustls session "Connection" events which don't have a parent span
                    .map(|s| s.parent().is_none() && s.name() == "Connection")
                    .unwrap_or_default()
            },
        ))
        .with(load_output_layer())
        .init();

    opentelemetry::global::set_text_map_propagator(opentelemetry::sdk::propagation::TraceContextPropagator::new());
}

fn load_otlp_headers() -> tonic::metadata::MetadataMap {
    let mut tracing_metadata = tonic::metadata::MetadataMap::new();

    #[cfg(debug_assertions)]
    tracing_metadata.insert(
        "x-honeycomb-team",
        "X6naTEMkzy10PMiuzJKifF".parse().unwrap(),
    );

    match std::env::var("OTEL_EXPORTER_OTLP_HEADERS").ok() {
        Some(headers) if !headers.is_empty() => {
            for header in headers.split_terminator(',') {
                if let Some((key, value)) = header.split_once('=') {
                    let key: &str = Box::leak(key.to_string().into_boxed_str());
                    let value = value.to_owned();
                    if let Ok(value) = value.parse() {
                        tracing_metadata.insert(key, value);
                    } else {
                        eprintln!("Could not parse value for header {}.", key);
                    }
                }
            }
        }
        _ => {}
    }

    tracing_metadata
}

fn load_output_layer<S>() -> Box<dyn Subscribe<S> + Send + Sync + 'static>
where
    S: Collect + Send + Sync,
    for<'a> S: LookupSpan<'a>,
{
    #[cfg(not(debug_assertions))]
    let tracing_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    #[cfg(debug_assertions)]
    let tracing_endpoint = Some("https://api.honeycomb.io:443".to_string());

    if let Some(endpoint) = tracing_endpoint {
        let metadata = load_otlp_headers();
        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(endpoint)
                    .with_metadata(metadata),
            )
            .with_trace_config(opentelemetry::sdk::trace::config().with_resource(
                opentelemetry::sdk::Resource::new(vec![
                    opentelemetry::KeyValue::new("service.name", "grey"),
                    opentelemetry::KeyValue::new("service.version", version!("v")),
                    opentelemetry::KeyValue::new("host.os", std::env::consts::OS),
                    opentelemetry::KeyValue::new("host.architecture", std::env::consts::ARCH),
                ]),
            ))
            .install_batch(opentelemetry::runtime::Tokio)
            .unwrap();

        tracing_opentelemetry::subscriber()
            .with_tracer(tracer)
            .boxed()
    } else {
        tracing_subscriber::fmt::subscriber().boxed()
    }
}
