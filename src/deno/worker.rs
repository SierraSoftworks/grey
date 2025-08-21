use std::{rc::Rc, sync::Arc, cell::RefCell};

use deno_core::{error::AnyError, *};
use deno_runtime::deno_permissions::{Permissions, PermissionsContainer, UnaryPermission};
use deno_runtime::permissions::RuntimePermissionDescriptorParser;
use deno_runtime::worker::{MainWorker, WorkerOptions, WorkerServiceOptions};
use deno_runtime::BootstrapOptions;
use sys_traits::impls::RealSys;

use crate::{
    deno::{
        module_loader::{MemoryModuleLoader, MEMORY_SCRIPT_SPECIFIER},
        runtime_snapshot,
    },
    version, Sample,
};

pub async fn run_probe_script(code: &str, args: Vec<String>) -> Result<Sample, AnyError> {
    let module = resolve_url(MEMORY_SCRIPT_SPECIFIER).unwrap();
    let sample = Rc::new(RefCell::new(Sample::default()));

    let mut worker = MainWorker::bootstrap_from_options(
        &module,
        WorkerServiceOptions::<
            deno_resolver::npm::DenoInNpmPackageChecker,
            deno_resolver::npm::managed::ManagedNpmResolver<RealSys>,
            RealSys,
        > {
            module_loader: Rc::new(MemoryModuleLoader::new(code)),
            blob_store: Default::default(),
            broadcast_channel: Default::default(),
            permissions: PermissionsContainer::new(Arc::new(RuntimePermissionDescriptorParser::new(RealSys::default())), get_permissions()),
            feature_checker: Default::default(),
            node_services: Default::default(),
            npm_process_state_provider: Default::default(),
            root_cert_store_provider: Default::default(),
            fetch_dns_resolver: Default::default(),
            shared_array_buffer_store: Default::default(),
            compiled_wasm_module_store: Default::default(),
            v8_code_cache: Default::default(),
            deno_rt_native_addon_loader: Default::default(),
            fs: Arc::new(crate::deno::fake_fs::NoOpFs),
        },
        WorkerOptions {
            bootstrap: BootstrapOptions {
                user_agent: version!("SierraSoftworks/grey@v"),
                args,
                ..Default::default()
            },
            startup_snapshot: Some(runtime_snapshot()),
            extensions: vec![crate::deno::grey_extension::init(sample.clone())],
            ..Default::default()
        }
    );

    worker.execute_main_module(&module).await?;
    worker.run_event_loop(false).await?;

    let sample: Sample = sample.borrow().clone().with("exit_code", worker.exit_code());

    Ok(sample)
}

fn get_permissions() -> Permissions {
    let mut perms = Permissions::none_without_prompt();
    perms.net = UnaryPermission::allow_all();
    perms
}

#[cfg(test)]
mod tests {
    use crate::SampleValue;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn test_set_output() {
        let output = run_probe_script(
            r#"
            setOutput('x', 'y')
            "#,
            Vec::new(),
        )
        .await
        .expect("no issues running");

        assert_eq!(
            output.get("x"),
            &SampleValue::String("y".into()),
            "the output value should be set"
        );
    }

    // #[tokio::test(flavor = "current_thread")]
    // async fn test_get_trace_id() {
    //     init_otel();

    //     test_with_trace_info("setOutput('trace_id', getTraceId())", |output, trace_id| {
    //         assert_eq!(
    //             output.get("trace_id"),
    //             &SampleValue::String(trace_id),
    //             "the trace ID should be set correctly"
    //         );
    //     }).await;
    // }

    // #[tokio::test(flavor = "current_thread")]
    // async fn test_get_trace_headers() {
    //     init_otel();

    //     test_with_trace_info("setOutput('traceparent', getTraceHeaders().traceparent); setOutput('tracestate', getTraceHeaders().tracestate)", |output, trace_id| {
    //         assert_eq!(
    //             output.get("traceparent"),
    //             &SampleValue::String(trace_id),
    //             "the trace ID should be set correctly"
    //         );

    //         assert_eq!(output.get("tracestate"), &SampleValue::String("".into()), "the trace state should be set correctly");
    //     }).await;
    // }

    // #[tokio::test(flavor = "current_thread")]
    // async fn test_trace_headers_varadic() {
    //     init_otel();

    //     test_with_trace_info("setOutput('traceheaders', JSON.stringify({...getTraceHeaders()}))", |output, trace_id| {
    //         assert_eq!(
    //             output.get("traceheaders"),
    //             &SampleValue::String(format!("{{\"traceparent\":\"{}\",\"tracestate\":\"\"}}", trace_id)),
    //             "the trace headers should be set correctly"
    //         );
    //     }).await;
    // }

    // fn init_otel() {
    //     tracing_subscriber::registry()
    //         .with(
    //             tracing_opentelemetry::subscriber().with_tracer(
    //                 opentelemetry::sdk::export::trace::stdout::new_pipeline()
    //                     .with_writer(std::io::sink())
    //                     .install_simple(),
    //             ),
    //         )
    //         .try_init().unwrap_or_default();

    //     opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());
    // }

    // async fn test_with_trace_info<F: FnOnce(Sample, String)>(code: &str, test: F) {
    //     let root = info_span!("root");

    //     let propagator = TraceContextPropagator::new();

    //     let _root = root.enter();

    //     let output = Arc::new(RefCell::new(Sample::default()));
    //     let mut worker = Worker::new_for_code(
    //         code,
    //         WorkerContext {
    //             args: Vec::new(),
    //             output: output.clone(),
    //         },
    //     )
    //     .expect("no issues initializing");

    //     let exit_code = worker
    //         .run()
    //         .instrument(root.clone())
    //         .await
    //         .expect("no runtime errors");
    //     assert_eq!(0, exit_code, "the exit code should be 0");

    //     let trace_id = {
    //         let mut headers = HashMap::new();
    //         propagator.inject_context(&root.context(), &mut headers);

    //         println!("{:?}", &root.context());

    //         headers.get("traceparent").expect("traceparent should be propagated").to_owned()
    //     };

    //     test(output.take(), trace_id);
    // }
}
