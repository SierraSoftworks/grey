use super::History;
use crate::formatters::availability;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ProbeProps {
    pub probe: grey_api::Probe,
}

#[function_component(Probe)]
pub fn probe(props: &ProbeProps) -> Html {
    let recent_availability = props.probe.recent(2).success_rate();

    let probe_class = match props.probe.history.last() {
        Some(h) if h.pass => "ok",
        Some(h) if !h.pass && recent_availability > 80.0 => "warn",
        Some(h) if !h.pass && recent_availability <= 80.0 => "error",
        _ => "ok",
    };

    let active_observers = props.probe.history.last().map(|h| h.observations.len()).unwrap_or(props.probe.observations.len());

    html! {
        <div class="probe">
            <div class="probe-title">
                <div class="probe-name-section">
                    <div class={format!("status-dot {}", probe_class)}></div>
                    <h3 class="probe-name">{&props.probe.name}</h3>

                    if !props.probe.tags.is_empty() {
                        <div class="probe-tags">
                            {for props.probe.tags.iter().filter(|(name, _)| *name != "service").map(|(name, value)| {
                                html! {
                                    <div class="probe-tag">
                                        <span class="tag-name">{name}{":"}</span>
                                        <strong class="tag-value">{value}</strong>
                                    </div>
                                }
                            })}
                        </div>
                    }
                </div>
                <div class="probe-observers" tooltip="The number of agents which have contributed to this status report.">
                    <span class="icon-eye"></span>
                    {format!("{}", active_observers)}
                </div>
                <div class="availability">{availability(props.probe.availability())}</div>
            </div>
            <History samples={props.probe.history.clone()} />
        </div>
    }
}
