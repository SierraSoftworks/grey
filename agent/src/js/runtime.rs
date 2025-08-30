use std::collections::HashMap;

use super::ReqwestFetcher;
use boa_engine::{
    Context, IntoJsFunctionCopied, JsObject, JsValue, js_string, object::builtins::JsArray,
    property::Attribute,
};
use tracing_batteries::prelude::*;

pub fn setup_runtime(
    context: &mut Context,
    args: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    boa_runtime::register(
        (
            boa_runtime::extensions::ConsoleExtension(super::TraceLogger),
            boa_runtime::extensions::FetchExtension(ReqwestFetcher::default()),
        ),
        None,
        context,
    )?;

    context.register_global_property(
        js_string!("output"),
        JsObject::with_null_proto(),
        Attribute::READONLY | Attribute::ENUMERABLE,
    )?;

    let args = JsArray::from_iter(args.into_iter().map(|v| js_string!(v).into()), context);
    context.register_global_property(
        js_string!("arguments"),
        args,
        Attribute::READONLY | Attribute::ENUMERABLE,
    )?;

    let get_trace_headers_ = get_trace_headers.into_js_function_copied(context);
    context.register_global_builtin_callable(
        js_string!("getTraceHeaders"),
        0,
        get_trace_headers_,
    )?;

    let get_trace_id_ = get_trace_id.into_js_function_copied(context);
    context.register_global_builtin_callable(js_string!("getTraceId"), 0, get_trace_id_)?;

    Ok(())
}

fn get_trace_headers() -> JsValue {
    let mut headers = HashMap::new();

    tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
        p.inject_context(&Span::current().context(), &mut headers)
    });

    let object = JsObject::with_null_proto();
    for (key, value) in headers.into_iter() {
        object
            .set(
                js_string!(key),
                js_string!(value),
                false,
                &mut Context::default(),
            )
            .unwrap();
    }

    object.into()
}

fn get_trace_id() -> JsValue {
    let mut headers = HashMap::new();

    tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
        p.inject_context(&Span::current().context(), &mut headers)
    });

    let trace_id = headers.get("traceparent").cloned();

    if let Some(trace_id) = trace_id {
        js_string!(trace_id).into()
    } else {
        JsValue::null()
    }
}
