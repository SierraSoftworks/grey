use grey_api::ProbeHistoryBucket;
use yew::prelude::*;
use chrono::Utc;

use crate::components::{Popover, StatusDot};
use crate::formatters::{availability, compact_duration};
use crate::styles::{pass_class, sample_class};

#[cfg(feature = "wasm")]
use {
    wasm_bindgen::JsCast,
    web_sys::{HtmlElement, MouseEvent},
};

#[cfg(feature = "wasm")]
use gloo_console as console;

/// The probe's current, quorum-derived health (see [`grey_api::Probe::passing`]) and how long it has
/// held it, used to render the most recent history segment (and its tooltip) from the live state
/// rather than the bucket's average.
#[derive(Clone, PartialEq, Debug)]
pub struct LiveStatus {
    pub healthy: bool,
    pub since: Option<chrono::DateTime<Utc>>,
}

impl LiveStatus {
    /// The live status of `probe`, or `None` for records (from older agents) that carry no streak at
    /// all and so only have their bucket averages to go on.
    pub fn of(probe: &grey_api::Probe) -> Option<Self> {
        (!probe.streak.is_empty() || !probe.observers.is_empty()).then(|| Self {
            healthy: probe.passing(),
            since: probe.since(),
        })
    }
}

#[derive(Properties, PartialEq)]
pub struct ProbeHistoryProps {
    pub samples: Vec<ProbeHistoryBucket>,

    /// The probe's live status, when known.
    #[prop_or_default]
    pub live: Option<LiveStatus>,
}

#[derive(Clone, Default, PartialEq)]
struct TooltipData {
    pub visible: bool,
    pub element_index: usize,
    pub probe_result: Option<ProbeHistoryBucket>,
}

#[function_component(ProbeHistory)]
pub fn probe_history(props: &ProbeHistoryProps) -> Html {
    let auth_data = use_context::<crate::contexts::Store>().expect("Store not found");
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
        <div class="probe-history">
            {for props.samples.iter().enumerate().map(|(index, sample)| {
                // The most recent segment is rendered from the probe's current state — a
                // segment that is failing right now is an error regardless of how well it
                // performed on average, while one that has recovered is at worst degraded.
                // Older segments only have their averages to go on.
                let is_current = index + 1 == props.samples.len();
                let current_live = if is_current { props.live.as_ref() } else { None };
                let current_passing = current_live.map(|live| live.healthy);
                let sample_class = sample_class(current_passing, sample.max_availability());

                // Serialize the entire ProbeResult to JSON
                let probe_result_json = serde_json::to_string(sample).unwrap_or_default();

                let is_tooltip_target = tooltip_data.visible && tooltip_data.element_index == index;

                html! {
                    <span
                        class={format!("probe-history__sample {} {}", sample_class, if is_tooltip_target { "tooltip-target" } else { "" })}
                        data-probe-result={probe_result_json}
                        data-index={index.to_string()}
                        onmouseenter={on_mouse_enter.clone()}
                        onmouseleave={on_mouse_leave.clone()}
                    >
                        if is_tooltip_target {
                            if let Some(probe_result) = &tooltip_data.probe_result {
                                {render_tooltip(probe_result, current_live, auth_data.is_authenticated())}
                            } else {
                                // Fallback for SSR or when probe_result is None
                                <Popover status_class="unknown" status="Loading...">
                                    <div class="tooltip__details">
                                        <div class="tooltip__row">
                                            <span class="tooltip__label">{"Status:"}</span>
                                            <span>{"Details loading..."}</span>
                                        </div>
                                    </div>
                                </Popover>
                            }
                        }
                    </span>
                }
            })}
        </div>
    }
}

fn render_tooltip(probe_result: &ProbeHistoryBucket, live: Option<&LiveStatus>, include_observers: bool) -> Html {
    let (status_text, status_class) = match live {
        Some(live) => {
            let now = Utc::now();
            let since = live
                .since
                .map(|t| format!(" for {}", compact_duration(now - t)))
                .unwrap_or_default();
            let label = if live.healthy { "Passing" } else { "Failing" };
            (format!("{label}{since}"), pass_class(live.healthy))
        }
        _ => (
            (if probe_result.max_availability() == 100.0 { "Passed" } else { "Failed" }).to_string(),
            pass_class(probe_result.pass),
        ),
    };

    // Format the bucket's timestamp — shown in the popover's footer.
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

    let mut relevant_observations = probe_result.observations.iter().collect::<Vec<_>>();
    relevant_observations.sort_by(|a, b| a.1.success_rate().partial_cmp(&b.1.success_rate()).unwrap_or(std::cmp::Ordering::Equal)); // (|(_, obs)| obs.success_rate());
    relevant_observations.truncate(probe_result.validations.len().max(3));


    html! {
        <Popover
            class="popover--history"
            status_class={status_class}
            status={status_text}
            timestamp={timestamp}
        >
            <div class="tooltip__details">
                if !probe_result.message.is_empty() {
                    <div class="tooltip__row">
                        <span>{&probe_result.message}</span>
                    </div>
                }
                <div class="tooltip__row">
                    <span class="tooltip__label">{"Latency:"}</span>
                    <span>{duration_text}</span>
                </div>
                <div class="tooltip__row">
                    <span class="tooltip__label">{"Availability:"}</span>
                    <span>{format!("{} ± {:.1}%", availability(overall_stats.success_rate()), overall_stats.success_rate_error_margin())}</span>
                </div>
                
                if overall_stats.total_retries > 0 {
                    <div class="tooltip__row">
                        <span class="tooltip__label">{"Retry Rate:"}</span>
                        <span>{format!("{:.1}%", overall_stats.retry_rate())}</span>
                    </div>
                }
            </div>

            if !probe_result.validations.is_empty() || (probe_result.observations.len() > 1 && include_observers) {
                <div class="tooltip__context">
                    if include_observers && probe_result.observations.len() > 1 {
                        <div class="tooltip__section">
                            <div class="tooltip__section-title">{"Observers"}</div>
                            {for relevant_observations.iter().map(|(name, observation)| {
                                let validation_class = pass_class(observation.success_rate() > 99.0);
                                html! {
                                    <div class="tooltip__section-entry">
                                        <div class="tooltip__section-entry-header">
                                            <StatusDot class={validation_class} />
                                            <span class="tooltip__section-entry-name">{availability(observation.success_rate())}</span>
                                            <span class="tooltip__section-entry-message">{*name}</span>
                                        </div>
                                    </div>
                                }
                            })}

                            if probe_result.observations.len() > relevant_observations.len() {
                                <div class="tooltip__section-entry">
                                    <span class="tooltip__section-entry-extra">{format!("and {} more...", probe_result.observations.len() - relevant_observations.len())}</span>
                                </div>
                            }
                        </div>
                    }

                    if !probe_result.validations.is_empty() {
                        <div class="tooltip__section">
                            <div class="tooltip__section-title">{"Checks"}</div>
                            {for probe_result.validations.iter().map(|(name, validation)| {
                                let validation_class = pass_class(validation.pass);
                                html! {
                                    <div class="tooltip__section-entry">
                                        <div class="tooltip__section-entry-header">
                                            <StatusDot class={validation_class} />
                                            <span class="tooltip__section-entry-name">{name}</span>
                                        </div>
                                        if let Some(ref msg) = validation.message {
                                            <div class="tooltip__section-entry-details">
                                                <div class="tooltip__section-entry-extra">{msg}</div>
                                            </div>
                                        }
                                    </div>
                                }
                            })}
                        </div>
                    }
                </div>
            }
        </Popover>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Properties, PartialEq)]
    struct HarnessProps {
        bucket: ProbeHistoryBucket,
        live: Option<LiveStatus>,
    }

    /// Renders the tooltip directly — in the app it only appears on hover, which SSR can't reach.
    #[function_component(Harness)]
    fn harness(props: &HarnessProps) -> Html {
        render_tooltip(&props.bucket, props.live.as_ref(), true)
    }

    async fn render(live: Option<LiveStatus>) -> String {
        let bucket = ProbeHistoryBucket {
            start_time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            pass: true,
            message: String::new(),
            validations: Default::default(),
            observations: Default::default(),
        };
        yew::ServerRenderer::<Harness>::with_props(move || HarnessProps { bucket, live })
            .render()
            .await
    }

    #[tokio::test]
    async fn test_tooltip_shows_bucket_footer_and_streak_since() {
        let live = LiveStatus { healthy: true, since: Some(chrono::Utc::now() - chrono::Duration::days(5)) };

        let html = render(Some(live)).await;
        assert!(html.contains("popover__time"), "expected the bucket timestamp footer, got: {html}");
        assert!(html.contains("2023-11-14"), "expected the bucket timestamp value in the footer, got: {html}");
        assert!(html.contains("Passing for 5d"), "expected the live status, got: {html}");
    }

    #[tokio::test]
    async fn test_tooltip_omits_streak_row_for_legacy_records() {
        let html = render(None).await;
        assert!(html.contains("popover__time"), "expected the bucket timestamp footer, got: {html}");
        assert!(!html.contains("Passing"), "expected no live status, got: {html}");
    }

    #[test]
    fn live_status_is_absent_for_legacy_records() {
        let mut probe = grey_api::Probe {
            name: "p".into(),
            tags: Default::default(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: Default::default(),
            streak: Default::default(),
            debounce: None,
            retired: false,
            observers: Default::default(),
            quorum: None,
        };
        assert!(LiveStatus::of(&probe).is_none());
        probe.observers.insert("a".into(), grey_api::ObserverState::default());
        assert!(LiveStatus::of(&probe).is_some());
    }
}
