use grey_api::ProbeHistoryBucket;
use yew::prelude::*;

use crate::formatters::{availability, si_magnitude};

#[cfg(feature = "wasm")]
use {
    wasm_bindgen::JsCast,
    web_sys::{HtmlElement, MouseEvent},
};

#[cfg(feature = "wasm")]
use gloo_console as console;

#[derive(Properties, PartialEq)]
pub struct HistoryProps {
    pub samples: Vec<ProbeHistoryBucket>,
}

#[derive(Clone, Default, PartialEq)]
struct TooltipData {
    pub visible: bool,
    pub element_index: usize,
    pub probe_result: Option<ProbeHistoryBucket>,
}

#[function_component(History)]
pub fn history(props: &HistoryProps) -> Html {
    let tooltip_data = use_state(TooltipData::default);

    #[cfg(feature = "wasm")]
    let on_mouse_enter = {
        let tooltip_data = tooltip_data.clone();
        Callback::from(move |e: MouseEvent| {
            // Safely get the target and convert it to HtmlElement
            if let Some(target) = e.target() {
                if let Ok(element) = target.dyn_into::<HtmlElement>() {
                    // Get the JSON data from the element
                    if let Some(json_data) = element.get_attribute("data-probe-result") {
                        if let Ok(probe_result) =
                            serde_json::from_str::<ProbeHistoryBucket>(&json_data)
                        {
                            let element_index = element
                                .get_attribute("data-index")
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(0);

                            tooltip_data.set(TooltipData {
                                visible: true,
                                element_index,
                                probe_result: Some(probe_result),
                            });
                        } else {
                            console::warn!("Failed to parse probe result JSON");
                        }
                    } else {
                        console::warn!("No probe result data found");
                    }
                } else {
                    console::warn!("Failed to convert target to HtmlElement");
                }
            } else {
                console::warn!("No target found in mouse event");
            }
        })
    };

    #[cfg(not(feature = "wasm"))]
    let on_mouse_enter = {
        let tooltip_data = tooltip_data.clone();
        Callback::from(move |_: MouseEvent| {
            // For SSR, we can't access DOM elements, so just show a basic tooltip
            // This won't actually be interactive but prevents compilation issues
            tooltip_data.set(TooltipData {
                visible: true,
                element_index: 0,
                probe_result: None, // No probe result available in SSR
            });
        })
    };

    let on_mouse_leave = {
        let tooltip_data = tooltip_data.clone();
        Callback::from(move |_: MouseEvent| {
            tooltip_data.set(TooltipData {
                visible: false,
                ..(*tooltip_data).clone()
            });
        })
    };

    html! {
        <div class="history">
            {for props.samples.iter().enumerate().map(|(index, sample)| {
                // A segment that is currently failing is an error regardless of how well it
                // performed on average, while one that has recovered is at worst degraded —
                // this makes both failures and recoveries visible immediately.
                let sample_class = match (sample.passing(), sample.max_availability()) {
                    (false, _) => "error",
                    (true, sli) if sli > 99.9 => "ok",
                    (true, _) => "warn",
                };

                // Serialize the entire ProbeResult to JSON
                let probe_result_json = serde_json::to_string(sample).unwrap_or_default();

                let is_tooltip_target = tooltip_data.visible && tooltip_data.element_index == index;

                html! {
                    <span
                        class={format!("history-sample {} {}", sample_class, if is_tooltip_target { "tooltip-target" } else { "" })}
                        data-probe-result={probe_result_json}
                        data-index={index.to_string()}
                        onmouseenter={on_mouse_enter.clone()}
                        onmouseleave={on_mouse_leave.clone()}
                    >
                        if is_tooltip_target {
                            if let Some(probe_result) = &tooltip_data.probe_result {
                                {render_tooltip(probe_result)}
                            } else {
                                // Fallback for SSR or when probe_result is None
                                <div class="tooltip visible">
                                    <div class="tooltip-header">
                                        <div class="tooltip-status-dot unknown"></div>
                                        {"Loading..."}
                                    </div>
                                    <div class="tooltip-details">
                                        <div class="tooltip-row">
                                            <span class="tooltip-label">{"Status:"}</span>
                                            <span>{"Details loading..."}</span>
                                        </div>
                                    </div>
                                </div>
                            }
                        }
                    </span>
                }
            })}
        </div>
    }
}

fn render_tooltip(probe_result: &ProbeHistoryBucket) -> Html {
    let passing = probe_result.passing();
    let status_text = match (passing, probe_result.max_availability()) {
        (true, sli) if sli == 100.0 => "Passing",
        (true, _) => "Recovered",
        (false, _) => "Failing",
    };
    let status_class = if passing { "ok" } else { "error" };

    // Format the timestamp
    let timestamp = probe_result
        .start_time
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    let overall_stats = probe_result.total();

    // Format duration
    let duration_text = format!(
        "{}",
        humantime::format_duration(overall_stats.average_latency())
    );

    let samples = si_magnitude(overall_stats.total_samples as f64, "");

    // Surface currently-failing observers first, then order by how poorly they performed.
    let mut relevant_observations = probe_result.observations.iter().collect::<Vec<_>>();
    relevant_observations.sort_by(|a, b| {
        let currently_failing = |obs: &grey_api::Observation| obs.has_state() && !obs.passing;
        currently_failing(b.1)
            .cmp(&currently_failing(a.1))
            .then(a.1.success_rate().partial_cmp(&b.1.success_rate()).unwrap_or(std::cmp::Ordering::Equal))
    });
    relevant_observations.truncate(probe_result.validations.len().max(3));


    html! {
        <div class="tooltip visible">
            <div class="tooltip-header">
                <div class={format!("tooltip-status-dot {}", status_class)}></div>
                {status_text}
            </div>
            <div class="tooltip-details">
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Start:"}</span>
                    <span>{timestamp}</span>
                </div>
                if let Some(since) = probe_result.since() {
                    <div class="tooltip-row">
                        <span class="tooltip-label">{if passing { "Passing since:" } else { "Failing since:" }}</span>
                        <span>{since.format("%Y-%m-%d %H:%M:%S UTC").to_string()}</span>
                    </div>
                }
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Latency:"}</span>
                    <span>{duration_text}</span>
                </div>
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Availability:"}</span>
                    <span>{availability(overall_stats.success_rate())}</span>
                </div>
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Retry Rate:"}</span>
                    <span>{format!("{:.1}%", overall_stats.retry_rate())}</span>
                </div>
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Samples:"}</span>
                    <span>{samples}</span>
                </div>
                if !probe_result.message.is_empty() {
                    <div class="tooltip-row">
                        <span class="tooltip-label">{"Message:"}</span>
                        <span>{&probe_result.message}</span>
                    </div>
                }
            </div>

            if !probe_result.validations.is_empty() || probe_result.observations.len() > 1 {
                <div class="tooltip-context">
                    if probe_result.observations.len() > 1 {
                        <div class="tooltip-section">
                            <div class="tooltip-section-title">{"Observers"}</div>
                            {for relevant_observations.iter().map(|(name, observation)| {
                                // Prefer the observer's most recent state; older agents only
                                // report aggregate counters, so fall back to the average there.
                                let observer_ok = if observation.has_state() {
                                    observation.passing
                                } else {
                                    observation.success_rate() > 99.0
                                };
                                let validation_class = if observer_ok { "ok" } else { "error" };
                                html! {
                                    <div class="tooltip-section-entry">
                                        <div class="tooltip-section-entry-header">
                                            <div class={format!("tooltip-status-dot {}", validation_class)}></div>
                                            <span class="tooltip-section-entry-name">{availability(observation.success_rate())}</span>
                                            <span class="tooltip-section-entry-message">{*name}</span>
                                        </div>
                                    </div>
                                }
                            })}

                            if probe_result.observations.len() > relevant_observations.len() {
                                <div class="tooltip-section-entry">
                                    <span class="tooltip-section-entry-extra">{format!("and {} more...", probe_result.observations.len() - relevant_observations.len())}</span>
                                </div>
                            }
                        </div>
                    }

                    if !probe_result.validations.is_empty() {
                        <div class="tooltip-section">
                            <div class="tooltip-section-title">{"Validations"}</div>
                            {for probe_result.validations.iter().map(|(name, validation)| {
                                let validation_class = if validation.pass { "ok" } else { "error" };
                                html! {
                                    <div class="tooltip-section-entry">
                                        <div class="tooltip-section-entry-header">
                                            <div class={format!("tooltip-status-dot {}", validation_class)}></div>
                                            <span class="tooltip-section-entry-name">{name}</span>
                                            <span class="tooltip-section-entry-message">{&validation.condition}</span>
                                        </div>
                                        if let Some(ref msg) = validation.message {
                                            <div class="tooltip-section-entry-details">
                                                <div class="tooltip-section-entry-extra">{msg}</div>
                                            </div>
                                        }
                                    </div>
                                }
                            })}
                        </div>
                    }
                </div>
            }
        </div>
    }
}
