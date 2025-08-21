use std::{collections::HashMap, rc::Rc, cell::RefCell};

use deno_core::{serde_v8::AnyValue, *};
use tracing::Span;
use tracing_batteries::prelude::*;

use crate::{Sample, SampleValue};

extension!(
    grey_extension,
    ops = [
        op_set_output,
        op_get_trace_headers,
    ],
    options = {
        sample: Rc<RefCell<Sample>>,
    },
    state = |state, config| {
        state.put(config.sample);
    },
);

#[op2]
fn op_set_output(state: &mut OpState, #[string] name: String, #[serde] value: Option<AnyValue>) {
    let sample_ref: &Rc<RefCell<Sample>> = state.borrow();

    let value = match value {
        Some(AnyValue::Bool(val)) => val.into(),
        Some(AnyValue::Number(val)) if val.floor() == val => SampleValue::Int(val as i64),
        Some(AnyValue::Number(val)) => SampleValue::Double(val),
        Some(AnyValue::String(val)) => val.into(),
        _ => SampleValue::None,
    };

    // Now exchange the value within sample_ref with a new one
    let mut sample = sample_ref.borrow_mut();
    let new_sample = sample.clone().with(name, value);
    *sample = new_sample;
}

#[op2]
#[serde]
fn op_get_trace_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();

    tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
        p.inject_context(&Span::current().context(), &mut headers)
    });

    headers
}
