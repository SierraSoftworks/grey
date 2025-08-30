use super::History;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ProbeProps {
    pub probe: grey_api::Probe,
}

#[function_component(Probe)]
pub fn probe(props: &ProbeProps) -> Html {
    let (successful, total) = props
        .probe
        .history
        .iter()
        .map(|history| (history.successful_samples, history.sample_count))
        .fold((0, 0), |acc, x| (acc.0 + x.0, acc.1 + x.1));
    let recent_availability = if total == 0 {
        100.0
    } else {
        100.0 * successful as f64 / total as f64
    };

    let probe_class = match props.probe.history.last() {
        Some(h) if h.pass => "ok",
        Some(h) if !h.pass && recent_availability > 0.8 => "warn",
        Some(h) if !h.pass && recent_availability <= 0.8 => "error",
        _ => "ok",
    };

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
                <div class="probe-observers">
                    {format!("{}", props.probe.observers)}
                </div>
                <div class="availability">{format!("{:.3}%", props.probe.availability())}</div>
            </div>
            <History samples={props.probe.history.clone()} />
        </div>
    }
}
