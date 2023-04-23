use std::{cell::RefCell, rc::Rc, sync::Arc};

use deno_core::{error::AnyError, *};
use deno_runtime::*;

use super::{exts::grey, module_loader::MemoryModuleLoader};
use crate::{version, Sample};

pub struct WorkerContext {
    pub args: Vec<String>,
    pub output: Arc<RefCell<Sample>>,
}

pub struct Worker {
    main_module: ModuleSpecifier,
    worker: worker::MainWorker,
}

impl Worker {
    #[allow(unused)]
    pub fn new_for_url(module: &ModuleSpecifier, context: WorkerContext) -> Result<Self, AnyError> {
        let mod_loader = Rc::new(NoopModuleLoader {});

        Ok(Worker::new(module, mod_loader, context))
    }

    pub fn new_for_code(code: &str, context: WorkerContext) -> Result<Self, AnyError> {
        let mod_specifier = resolve_url("memory:probe.js")?;
        let mod_loader: Rc<dyn ModuleLoader> = Rc::new(MemoryModuleLoader::new(code));

        Ok(Worker::new(&mod_specifier, mod_loader, context))
    }

    pub async fn run(&mut self) -> Result<i32, AnyError> {
        self.worker.execute_main_module(&self.main_module).await?;
        Ok(self.worker.exit_code())
    }

    #[instrument("script.worker.init", skip(module, mod_loader, context), fields(module.url=%module))]
    fn new(
        module: &ModuleSpecifier,
        mod_loader: Rc<dyn ModuleLoader>,
        context: WorkerContext,
    ) -> Self {
        let permissions = permissions::Permissions::allow_all();

        let worker = deno_runtime::worker::MainWorker::bootstrap_from_options(
            module.clone(),
            permissions::PermissionsContainer::new(permissions),
            deno_runtime::worker::WorkerOptions {
                module_loader: mod_loader,
                startup_snapshot: Some(super::deno_isolate_init()),
                extensions: vec![grey::init_ops(context.output)],
                bootstrap: BootstrapOptions {
                    user_agent: version!("SierraSoftworks/grey@v"),
                    args: context.args,
                    ..Default::default()
                },
                ..Default::default()
            },
        );

        Self {
            worker,
            main_module: module.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use opentelemetry::{sdk::propagation::TraceContextPropagator, propagation::TextMapPropagator};
    use tracing::Instrument;
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    use tracing_subscriber::{subscribe::CollectExt, util::SubscriberInitExt};

    use crate::SampleValue;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn test_set_output() {
        let output = Arc::new(RefCell::new(Sample::default()));
        let mut worker = Worker::new_for_code(
            "setOutput('x', 'y')",
            WorkerContext {
                args: Vec::new(),
                output: output.clone(),
            },
        )
        .expect("no issues initializing");

        let exit_code = worker.run().await.expect("no runtime errors");
        assert_eq!(0, exit_code, "the exit code should be 0");

        assert_eq!(
            output.borrow().get("x"),
            &SampleValue::String("y".into()),
            "the output value should be set"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_get_trace_id() {
        init_otel();
        
        test_with_trace_info("setOutput('trace_id', getTraceId())", |output, trace_id| {
            assert_eq!(
                output.get("trace_id"),
                &SampleValue::String(trace_id),
                "the trace ID should be set correctly"
            );
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_get_trace_headers() {
        init_otel();
        
        test_with_trace_info("setOutput('traceparent', getTraceHeaders().traceparent); setOutput('tracestate', getTraceHeaders().tracestate)", |output, trace_id| {
            assert_eq!(
                output.get("traceparent"),
                &SampleValue::String(trace_id),
                "the trace ID should be set correctly"
            );

            assert_eq!(output.get("tracestate"), &SampleValue::String("".into()), "the trace state should be set correctly");
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_trace_headers_varadic() {
        init_otel();
        
        test_with_trace_info("setOutput('traceheaders', JSON.stringify({...getTraceHeaders()}))", |output, trace_id| {
            assert_eq!(
                output.get("traceheaders"),
                &SampleValue::String(format!("{{\"traceparent\":\"{}\",\"tracestate\":\"\"}}", trace_id)),
                "the trace headers should be set correctly"
            );
        }).await;
    }

    fn init_otel() {
        tracing_subscriber::registry()
            .with(
                tracing_opentelemetry::subscriber().with_tracer(
                    opentelemetry::sdk::export::trace::stdout::new_pipeline()
                        .with_writer(std::io::sink())
                        .install_simple(),
                ),
            )
            .try_init().unwrap_or_default();

        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());
    }

    async fn test_with_trace_info<F: FnOnce(Sample, String)>(code: &str, test: F) {
        let root = info_span!("root");

        let propagator = TraceContextPropagator::new();
        
        let _root = root.enter();

        let output = Arc::new(RefCell::new(Sample::default()));
        let mut worker = Worker::new_for_code(
            code,
            WorkerContext {
                args: Vec::new(),
                output: output.clone(),
            },
        )
        .expect("no issues initializing");

        let exit_code = worker
            .run()
            .instrument(root.clone())
            .await
            .expect("no runtime errors");
        assert_eq!(0, exit_code, "the exit code should be 0");

        let trace_id = {
            let mut headers = HashMap::new();
            propagator.inject_context(&root.context(), &mut headers);

            println!("{:?}", &root.context());

            headers.get("traceparent").expect("traceparent should be propagated").to_owned()
        };

        test(output.take(), trace_id);
    }
}
