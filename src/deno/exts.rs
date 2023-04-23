use std::{cell::RefCell, sync::Arc, collections::HashMap};

use deno_core::{*, serde_v8::AnyValue};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::{Sample, SampleValue};

extension!(
    grey,
    ops = [
        op_set_output,
        op_get_trace_headers,
    ],
    esm = [dir "js", "40_output.js"],
    options = {
        sample: Arc<RefCell<Sample>>,
    },
    state = |state, config| {
        state.put(config.sample);
    },
    customizer = |ext: &mut deno_core::ExtensionBuilder| {
        ext.force_op_registration();
    }
);

#[op]
fn op_set_output(state: &mut OpState, name: String, value: Option<AnyValue>) {
    let sample_ref: &mut Arc<RefCell<Sample>> = state.borrow_mut();

    let value = match value {
        None => SampleValue::None,
        Some(AnyValue::Bool(val)) => val.into(),
        Some(AnyValue::Number(val)) if val.floor() == val => SampleValue::Int(val as i64),
        Some(AnyValue::Number(val)) => SampleValue::Double(val),
        Some(AnyValue::String(val)) => val.into(),
        _ => SampleValue::None
    };

    sample_ref.replace_with(|old| old.clone().with(name, value));
}

#[op]
fn op_get_trace_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();

    opentelemetry::global::get_text_map_propagator(|p| {
        p.inject_context(&Span::current().context(), &mut headers)
    });

    headers
}