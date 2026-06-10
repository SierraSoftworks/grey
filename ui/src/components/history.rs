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

    /// The probe's cluster-converged streak record, used to render the most recent
    /// segment (and its tooltip) from the live state rather than the bucket's average.
    #[prop_or_default]
    pub streak: Option<grey_api::Streak>,
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
                // The most recent segment is rendered from the probe's current state — a
                // segment that is failing right now is an error regardless of how well it
                // performed on average, while one that has recovered is at worst degraded.
                // Older segments only have their averages to go on.
                let is_current = index + 1 == props.samples.len();
                let current_passing = props.streak.as_ref().filter(|_| is_current).map(|s| s.passing());
                let sample_class = match (current_passing, sample.max_availability()) {
                    (Some(false), _) => "error",
                    (Some(true), sli) if sli > 99.9 => "ok",
                    (Some(true), _) => "warn",
                    (None, sli) if sli > 99.9 => "ok",
                    (None, sli) if sli < 90.0 => "error",
                    (None, _) => "warn",
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
                                {render_tooltip(probe_result, props.streak.as_ref().filter(|_| is_current))}
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

fn render_tooltip(probe_result: &ProbeHistoryBucket, streak: Option<&grey_api::Streak>) -> Html {
    let (status_text, status_class) = match streak {
        Some(streak) if streak.passing() => ("Passing", "ok"),
        Some(_) => ("Failing", "error"),
        None => (
            if probe_result.max_availability() == 100.0 { "Passed" } else { "Failed" },
            if probe_result.pass { "ok" } else { "error" },
        ),
    };

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

    let mut relevant_observations = probe_result.observations.iter().collect::<Vec<_>>();
    relevant_observations.sort_by(|a, b| a.1.success_rate().partial_cmp(&b.1.success_rate()).unwrap_or(std::cmp::Ordering::Equal)); // (|(_, obs)| obs.success_rate());
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
                if let Some(streak) = streak {
                    if let Some(since) = streak.since() {
                        <div class="tooltip-row">
                            <span class="tooltip-label">{if streak.passing() { "Passing since:" } else { "Failing since:" }}</span>
                            <span>{since.format("%Y-%m-%d %H:%M:%S UTC").to_string()}</span>
                        </div>
                    }
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
                                let validation_class = if observation.success_rate() > 99.0 { "ok" } else { "error" };
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
