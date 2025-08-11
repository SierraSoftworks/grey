use yew::prelude::*;
use super::{History};

#[derive(Properties, PartialEq)]
pub struct ProbeProps {
    pub probe: grey_api::Probe,
    pub history: Vec<grey_api::ProbeResult>,
}

#[function_component(Probe)]
pub fn probe(props: &ProbeProps) -> Html {
    let recent_availability = props.history.iter().filter(|h| h.pass).count() as f64 / props.history.len() as f64;
    let probe_class = match props.history.last() {
        Some(h) if h.pass => "ok",
        Some(h) if !h.pass && recent_availability > 0.8 => "warn",
        Some(h) if !h.pass && recent_availability <= 0.8 => "error",
        _ => "ok",
    };

    let policy = format!("interval: {}, timeout: {}, retries: {}",
        humantime::format_duration(props.probe.policy.interval),
        humantime::format_duration(props.probe.policy.timeout),
        props.probe.policy.retries.unwrap_or(0));

    html! {
        <div class={format!("section probe {}", probe_class)}>
            <h3>
                <span>
                    {&props.probe.name}

                    if !props.probe.tags.is_empty() {
                        <div class="probe-tags">
                            {for props.probe.tags.iter().map(|(name, value)| {
                                html! {
                                    <div class="probe-tag">
                                        <span class="tag-name">{name}{":"}</span>
                                        <strong class="tag-value">{value}</strong>
                                    </div>
                                }
                            })}
                        </div>
                    }
                </span>
                <span class="availability">{format!("{:.3}%", props.probe.availability)}</span>
            </h3>
            <div class="probe-config probe-target">
                <span>{&props.probe.target}</span>
                <span>{policy}</span>
            </div>
            <History samples={props.history.clone()} />
        </div>
    }
}
