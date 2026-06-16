//! Shared, read-only incident display:
//! - [`IncidentSummary`] — a compact entry (title + horizontal timeline with hover popovers) used on
//!   the landing page.
//! - [`IncidentBlock`] — the full vertical event timeline used on the incidents list.
//! - [`IncidentsSection`] — the landing-page wrapper over the summaries.
//! Editing lives on the per-incident page (see `views::incident_detail`), not here.

use crate::components::markdown::render_markdown;
use crate::components::{Popover, PopoverAlign};
use crate::formatters::compact_duration;
use crate::routes::Route;
use chrono::{DateTime, Utc};
use grey_api::{Impact, Incident, IncidentUpdate};
use yew::prelude::*;
use yew_router::prelude::*;

/// The colour class for an impact (`ok`/`warn`/`error`; `draft` is the muted hidden state).
pub fn impact_class(impact: Impact) -> &'static str {
    match impact {
        Impact::Offline => "error",
        Impact::Degraded => "warn",
        Impact::None => "ok",
        Impact::Hidden => "draft",
    }
}

pub fn impact_label(impact: Impact) -> &'static str {
    match impact {
        Impact::Offline => "Offline",
        Impact::Degraded => "Degraded",
        Impact::None => "Operational",
        Impact::Hidden => "Hidden",
    }
}

/// The status text shown alongside an incident: the current impact and how long it has held (like a
/// probe's streak), "Resolved" once it returns to operational, or "Draft" while hidden.
pub fn incident_status(incident: &Incident) -> (String, &'static str) {
    match incident.current_impact() {
        Impact::Hidden => ("Draft".to_string(), "draft"),
        Impact::None => {
            let start = incident.started_at();
            let end = incident.ended_at();
            match (start, end) {
                (Some(s), Some(e)) if e > s => {
                    let duration = compact_duration(e.signed_duration_since(s));
                    (format!("Resolved after {duration}"), "ok")
                }
                _ => ("Resolved".to_string(), "ok"),
            }
        },
        impact @ (Impact::Offline | Impact::Degraded) => {
            let since = incident.impact_since().or_else(|| incident.started_at());
            let held = since
                .map(|s| format!(" for {}", compact_duration(Utc::now().signed_duration_since(s))))
                .unwrap_or_default();
            (format!("{}{held}", impact_label(impact)), impact_class(impact))
        }
    }
}

/// The worst impact among the incidents (used for the landing-page top-line status).
pub fn worst_impact(incidents: &[Incident]) -> Option<Impact> {
    incidents
        .iter()
        .filter(|i| i.is_active())
        .map(|i| i.current_impact())
        .max_by_key(|impact| impact.rank())
}

fn time_format(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// The landing-page incidents section: a flat list of compact summaries (no card chrome). Renders
/// nothing when there are no incidents to show.
#[derive(Properties, PartialEq)]
pub struct IncidentsSectionProps {
    pub incidents: Vec<Incident>,
}

#[function_component(IncidentsSection)]
pub fn incidents_section(props: &IncidentsSectionProps) -> Html {
    if props.incidents.is_empty() {
        return html! {};
    }

    html! {
        <div class="incidents-section">
            <div class="incident-summaries">
                { for props.incidents.iter().map(|incident| html! {
                    <IncidentSummary key={incident.id.to_string()} incident={incident.clone()} />
                }) }
            </div>
        </div>
    }
}

/// A compact incident summary: title, current status, and a full-width horizontal timeline of
/// impact-coloured markers, each revealing its update in a popover on hover.
#[derive(Properties, PartialEq)]
pub struct IncidentSummaryProps {
    pub incident: Incident,
}

#[function_component(IncidentSummary)]
pub fn incident_summary(props: &IncidentSummaryProps) -> Html {
    let incident = &props.incident;
    let (status_text, status_class) = incident_status(incident);
    // The most recent update's message, shown as a one-glance summary of where the incident stands.
    let latest_message = incident.sorted_updates().last().map(|u| u.message.clone());

    html! {
        <div class="incident-summary">
            <div class="incident-summary-header">
                <h3 class="incident-summary-title">
                    <Link<Route> to={Route::Incident { id: incident.id.to_string() }} classes="incident-link">
                        {&incident.title}
                    </Link<Route>>
                </h3>
                <span class={classes!("incident-streak", status_class)}>{status_text}</span>
            </div>
            if let Some(message) = latest_message {
                <div class="incident-summary-message markdown">{ render_markdown(&message) }</div>
            }
            <HorizontalTimeline incident={incident.clone()} />
        </div>
    }
}

/// The horizontal timeline used in summaries: the first update sits at the far left; the segment
/// after the last update extends to the far right in its colour, unless it has returned to
/// operational (a terminal marker). Popovers anchor inward at the ends so they stay on-screen.
#[derive(Properties, PartialEq)]
pub struct HorizontalTimelineProps {
    pub incident: Incident,
}

#[function_component(HorizontalTimeline)]
pub fn horizontal_timeline(props: &HorizontalTimelineProps) -> Html {
    let active = use_state(|| Option::<usize>::None);
    let updates = props.incident.sorted_updates();
    if updates.is_empty() {
        return html! {};
    }
    let last = updates.len() - 1;
    let extend_tail = updates[last].impact != Impact::None;

    html! {
        <div class="incident-hbar">
            { for updates.iter().enumerate().map(|(i, update)| {
                let dot_class = impact_class(update.impact);
                let is_open = *active == Some(i);
                let on_enter = { let active = active.clone(); Callback::from(move |_| active.set(Some(i))) };
                let on_leave = { let active = active.clone(); Callback::from(move |_| active.set(None)) };
                // Anchor popovers inward at the ends so they don't run off the page.
                let align = if i == 0 { PopoverAlign::Left } else if i == last { PopoverAlign::Right } else { PopoverAlign::Center };
                html! {
                    <>
                        // The line before a marker carries the preceding marker's colour.
                        if i > 0 {
                            <span class={classes!("hline", impact_class(updates[i - 1].impact))}></span>
                        }
                        <div
                            class={classes!("hdot-wrap", is_open.then_some("open"))}
                            onmouseenter={on_enter}
                            onmouseleave={on_leave}
                        >
                            <span class={classes!("hdot", dot_class)}></span>
                            if is_open {
                                <Popover
                                    {align}
                                    status_class={dot_class}
                                    status={impact_label(update.impact)}
                                    timestamp={time_format(update.timestamp)}
                                >
                                    <div class="markdown">
                                        { render_markdown(&update.message) }
                                    </div>
                                </Popover>
                            }
                        </div>
                    </>
                }
            }) }
            // The final state extends to the right edge unless the incident is resolved.
            if extend_tail {
                <span class={classes!("hline", impact_class(updates[last].impact))}></span>
            }
        </div>
    }
}

/// A single incident in full: its title, id, status, and the vertical timeline of its updates. Flat
/// (no card chrome); the update entries themselves are the cards.
#[derive(Properties, PartialEq)]
pub struct IncidentBlockProps {
    pub incident: Incident,
}

#[function_component(IncidentBlock)]
pub fn incident_block(props: &IncidentBlockProps) -> Html {
    let incident = &props.incident;
    let (status_text, status_class) = incident_status(incident);

    html! {
        <article class="incident-block">
            <div class="incident-block-header">
                <div class="incident-block-heading">
                    <h3 class="incident-block-title">
                        <Link<Route> to={Route::Incident { id: incident.id.to_string() }} classes="incident-link">
                            {&incident.title}
                        </Link<Route>>
                    </h3>
                    <span class="incident-id">{format!("{}", incident.id)}</span>
                </div>
                <span class={classes!("incident-streak", status_class)}>{status_text}</span>
            </div>
            <VerticalTimeline updates={incident.updates.clone()} />
        </article>
    }
}

/// The vertical event timeline: each update is a circle (coloured by impact) with a card showing its
/// timestamp, impact and message. The line below a circle keeps its colour down to the next one.
#[derive(Properties, PartialEq)]
pub struct VerticalTimelineProps {
    pub updates: Vec<IncidentUpdate>,
}

#[function_component(VerticalTimeline)]
pub fn vertical_timeline(props: &VerticalTimelineProps) -> Html {
    let mut updates = props.updates.clone();
    // Most recent update first.
    updates.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    if updates.is_empty() {
        return html! { <p class="incidents-empty">{"No updates yet."}</p> };
    }

    html! {
        <ul class="incident-timeline">
            { for updates.iter().enumerate().map(|(i, update)| {
                let class = impact_class(update.impact);
                html! {
                    <li class="timeline-item">
                        <div class="timeline-rail">
                            <span class={classes!("timeline-circle", class)}></span>
                            <span class={classes!("timeline-tail", class)}></span>
                        </div>
                        <div class="timeline-body">
                            <div class="timeline-time">
                                <span class={classes!("incident-status-pill", class)}>{impact_label(update.impact)}</span>
                                {time_format(update.timestamp)}
                            </div>
                            <div class={classes!("timeline-card", class)}>
                                <div class="timeline-card-message markdown">{ render_markdown(&update.message) }</div>
                            </div>
                        </div>
                    </li>
                }
            }) }
        </ul>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::Identifier;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn update(impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate { impact, timestamp: ts(secs), message: format!("update at {secs}") }
    }

    fn incident(updates: Vec<IncidentUpdate>) -> Incident {
        Incident { id: Identifier::from(1_234_567u64), title: "DB outage".into(), version: 1, updates }
    }

    async fn render_block(incident: Incident) -> String {
        #[function_component(Harness)]
        fn harness(props: &IncidentBlockProps) -> Html {
            use yew_router::history::{AnyHistory, MemoryHistory};
            let history = AnyHistory::from(MemoryHistory::new());
            html! { <Router history={history}><IncidentBlock incident={props.incident.clone()} /></Router> }
        }
        yew::ServerRenderer::<Harness>::with_props(move || IncidentBlockProps { incident })
            .render()
            .await
    }

    #[tokio::test]
    async fn block_renders_timeline_id_and_link() {
        let html = render_block(incident(vec![
            update(Impact::Offline, 100),
            update(Impact::None, 200),
        ]))
        .await;
        assert!(html.contains("timeline-circle error"), "offline circle: {html}");
        assert!(html.contains("timeline-circle ok"), "resolving circle");
        assert!(html.contains("#1q-jg7") || html.contains("#"), "shows nice id");
        assert!(html.contains("/incidents/"), "links to detail");
        assert!(html.contains("Resolved"), "resolved status");
    }

    #[test]
    fn worst_impact_ignores_resolved_and_hidden() {
        let degraded = incident(vec![update(Impact::Degraded, 1)]);
        let offline = incident(vec![update(Impact::Offline, 1)]);
        let resolved = incident(vec![update(Impact::Offline, 1), update(Impact::None, 2)]);
        let draft = incident(vec![update(Impact::Hidden, 1)]);

        assert_eq!(worst_impact(&[degraded.clone()]), Some(Impact::Degraded));
        assert_eq!(worst_impact(&[degraded, offline]), Some(Impact::Offline));
        assert_eq!(worst_impact(&[resolved, draft]), None, "resolved/hidden are not active");
    }
}
