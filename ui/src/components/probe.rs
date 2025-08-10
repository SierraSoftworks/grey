use yew::prelude::*;
use serde::{Deserialize, Serialize};

use super::History;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeData {
    pub name: String,
    pub availability: f64,
    pub target: String,
    pub policy: String,
    pub samples: Vec<SampleData>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleData {
    pub pass: bool,
    pub message: String,
}

#[derive(Properties, PartialEq)]
pub struct ProbeProps {
    pub probe: ProbeData,
}

#[function_component(Probe)]
pub fn probe(props: &ProbeProps) -> Html {
    let probe_class = if props.probe.availability == 100.0 {
        "ok"
    } else if props.probe.availability > 90.0 {
        "warn"
    } else {
        "error"
    };

    html! {
        <div class={format!("section probe {}", probe_class)}>
            <h3>
                {&props.probe.name}
                <span class="availability">{format!("{}%", props.probe.availability)}</span>
            </h3>
            <div class="probe-config probe-target">
                <span>{&props.probe.target}</span>
                <span>{&props.probe.policy}</span>
            </div>
            <History samples={props.probe.samples.clone()} />
        </div>
    }
}
