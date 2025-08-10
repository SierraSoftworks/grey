use yew::prelude::*;
use super::{History};

#[derive(Properties, PartialEq)]
pub struct ProbeProps {
    pub probe: grey_api::Probe,
    pub history: Vec<grey_api::ProbeResult>,
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
                <span>{format!("interval: {}ms", props.probe.policy.interval.as_millis())}</span>
            </div>
            <History samples={props.history.clone()} />
        </div>
    }
}
