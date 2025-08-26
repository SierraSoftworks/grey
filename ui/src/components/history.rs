use grey_api::ProbeHistory;
use yew::prelude::*;

#[cfg(feature = "wasm")]
use {
    wasm_bindgen::JsCast,
    web_sys::{HtmlElement, MouseEvent},
};

#[cfg(feature = "wasm")]
use gloo_console as console;

#[derive(Properties, PartialEq)]
pub struct HistoryProps {
    pub samples: Vec<ProbeHistory>,
}

#[derive(Clone, PartialEq)]
struct TooltipData {
    pub visible: bool,
    pub element_index: usize,
    pub probe_result: Option<grey_api::ProbeHistory>,
}

impl Default for TooltipData {
    fn default() -> Self {
        Self {
            visible: false,
            element_index: 0,
            probe_result: None,
        }
    }
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
                            serde_json::from_str::<grey_api::ProbeHistory>(&json_data)
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
                let sample_class = match sample.availability() {
                    sli if sli > 99.9 => "ok",
                    sli if sli < 90.0 => "error",
                    _ => "warn",
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

fn render_tooltip(probe_result: &grey_api::ProbeHistory) -> Html {
    let status_text = if probe_result.pass {
        "Passed"
    } else {
        "Failed"
    };
    let status_class = if probe_result.pass { "ok" } else { "error" };

    // Format the timestamp
    let timestamp = probe_result
        .start_time
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    // Format duration
    let duration_text = format!("{}", humantime::format_duration(probe_result.latency));

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
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Latency:"}</span>
                    <span>{duration_text}</span>
                </div>
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Availability:"}</span>
                    <span>{format!("{:.1}%", probe_result.availability())}</span>
                </div>
                <div class="tooltip-row">
                    <span class="tooltip-label">{"Retry Rate:"}</span>
                    <span>{format!("{:.1}%", probe_result.retry_rate())}</span>
                </div>
                if !probe_result.message.is_empty() {
                    <div class="tooltip-row">
                        <span class="tooltip-label">{"Message:"}</span>
                        <span>{&probe_result.message}</span>
                    </div>
                }
                if !probe_result.validations.is_empty() {
                    <div class="tooltip-section">
                        <div class="tooltip-label tooltip-section-title">{"Validations:"}</div>
                        {for probe_result.validations.iter().map(|(name, validation)| {
                            let validation_class = if validation.pass { "ok" } else { "error" };
                            html! {
                                <div class="tooltip-validation">
                                    <div class="tooltip-validation-header">
                                        <div class={format!("tooltip-status-dot {}", validation_class)}></div>
                                        <span class="tooltip-validation-name">{name}</span>
                                        <span class="tooltip-validation-message">{&validation.condition}</span>
                                    </div>
                                    if let Some(ref msg) = validation.message {
                                        <div class="tooltip-validation-details">
                                            <div class="tooltip-validation-extra">{msg}</div>
                                        </div>
                                    }
                                </div>
                            }
                        })}
                    </div>
                }
            </div>
        </div>
    }
}
