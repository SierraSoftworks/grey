//! Shared incident display:
//! - [`IncidentSummary`] — a compact card (title, description, horizontal timeline with hover
//!   popovers) used on the landing page.
//! - [`IncidentBlock`] — the full detail (vertical event timeline) used on the incidents page, the
//!   per-incident page, and the admin view.
//! - [`IncidentsSection`] — the landing-page section: a colour-coded header over the summaries.

use crate::components::markdown::render_markdown;
use crate::formatters::compact_duration;
use crate::routes::Route;
use chrono::{DateTime, Utc};
use grey_api::{Impact, Incident, IncidentUpdate};
use yew::prelude::*;
use yew_router::prelude::*;

/// The colour class for an impact. Matches the shared `.ok`/`.warn`/`.error` classes; `hidden` uses
/// the muted draft treatment.
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

/// The header status text shown on the right of an incident (like a probe's streak): the current
/// impact and how long it has held, e.g. "Offline for 1h". Returns `(text, class)`.
fn header_status(incident: &Incident) -> (String, &'static str) {
    let impact = incident.current_impact();
    if impact == Impact::Hidden {
        return ("Draft".to_string(), "draft");
    }
    if incident.end_time.is_some() {
        return ("Resolved".to_string(), "ok");
    }
    match impact {
        Impact::Offline | Impact::Degraded => {
            let since = incident.impact_since().unwrap_or(incident.start_time);
            let held_for = compact_duration(Utc::now().signed_duration_since(since));
            (format!("{} for {held_for}", impact_label(impact)), impact_class(impact))
        }
        Impact::None => ("Operational".to_string(), "ok"),
        Impact::Hidden => ("Draft".to_string(), "draft"),
    }
}

/// Summarises the section header: a colour class and label reflecting the worst active incident.
pub fn active_summary(incidents: &[Incident]) -> (&'static str, String) {
    let active = incidents.iter().filter(|i| i.is_active()).count();
    if active == 0 {
        return ("ok", "No active incidents".to_string());
    }
    let any_offline = incidents
        .iter()
        .any(|i| i.is_active() && i.current_impact() == Impact::Offline);
    let class = if any_offline { "error" } else { "warn" };
    let plural = if active == 1 { "" } else { "s" };
    (class, format!("{active} active incident{plural}"))
}

fn time_format(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// The landing-page incidents section: a colour-coded header over compact summaries. Renders nothing
/// when there are no incidents to show.
#[derive(Properties, PartialEq)]
pub struct IncidentsSectionProps {
    pub incidents: Vec<Incident>,
}

#[function_component(IncidentsSection)]
pub fn incidents_section(props: &IncidentsSectionProps) -> Html {
    if props.incidents.is_empty() {
        return html! {};
    }

    let (class, text) = active_summary(&props.incidents);

    html! {
        <div class="content incidents-section">
            <div class={classes!("section", "fill", class)}>
                <span class={classes!("status", class)}>{text}</span>
            </div>
            <div class="incident-summaries">
                { for props.incidents.iter().map(|incident| html! {
                    <IncidentSummary key={incident.id.clone()} incident={incident.clone()} />
                }) }
            </div>
        </div>
    }
}

/// A compact incident card: title, description, and a horizontal timeline of impact-coloured markers
/// whose details appear in a popover on hover.
#[derive(Properties, PartialEq)]
pub struct IncidentSummaryProps {
    pub incident: Incident,
}

#[function_component(IncidentSummary)]
pub fn incident_summary(props: &IncidentSummaryProps) -> Html {
    let incident = &props.incident;
    let active = use_state(|| Option::<usize>::None);
    let (status_text, status_class) = header_status(incident);

    let mut updates = incident.updates.clone();
    updates.sort_by_key(|u| u.timestamp);

    html! {
        <div class="incident-summary">
            <div class="incident-summary-header">
                <h3 class="incident-summary-title">
                    <Link<Route> to={Route::Incident { id: incident.id.clone() }} classes="incident-link">
                        {&incident.title}
                    </Link<Route>>
                </h3>
                <span class={classes!("incident-streak", status_class)}>{status_text}</span>
            </div>

            if !incident.description.is_empty() {
                <div class="incident-summary-description markdown">
                    { render_markdown(&incident.description) }
                </div>
            }

            <div class="incident-hbar">
                { for updates.iter().enumerate().map(|(i, update)| {
                    // The line leading into a marker carries the preceding marker's colour.
                    let line_class = if i == 0 { "start" } else { impact_class(updates[i - 1].impact) };
                    let dot_class = impact_class(update.impact);
                    let is_open = *active == Some(i);
                    let on_enter = { let active = active.clone(); Callback::from(move |_| active.set(Some(i))) };
                    let on_leave = { let active = active.clone(); Callback::from(move |_| active.set(None)) };
                    html! {
                        <div class="hstep">
                            <span class={classes!("hline", line_class)}></span>
                            <div
                                class={classes!("hdot-wrap", is_open.then_some("open"))}
                                onmouseenter={on_enter}
                                onmouseleave={on_leave}
                            >
                                <span class={classes!("hdot", dot_class)}></span>
                                if is_open {
                                    <div class="incident-popover">
                                        <div class="incident-popover-head">
                                            <span class={classes!("incident-status-pill", dot_class)}>
                                                {impact_label(update.impact)}
                                            </span>
                                            <span class="incident-popover-time">{time_format(update.timestamp)}</span>
                                        </div>
                                        <div class="incident-popover-body markdown">
                                            { render_markdown(&update.message) }
                                        </div>
                                    </div>
                                }
                            </div>
                        </div>
                    }
                }) }
            </div>
        </div>
    }
}

/// A single incident in full: a status-bordered card with its event timeline. `controls` grafts
/// admin buttons onto the foot of the card. Lightweight (no `.section` chrome).
#[derive(Properties, PartialEq)]
pub struct IncidentBlockProps {
    pub incident: Incident,
    #[prop_or_default]
    pub controls: Html,
}

#[function_component(IncidentBlock)]
pub fn incident_block(props: &IncidentBlockProps) -> Html {
    let incident = &props.incident;
    let class = impact_class(incident.current_impact());
    let (status_text, status_class) = header_status(incident);

    html! {
        <article class={classes!("incident-block", class)}>
            <div class="incident-block-header">
                <div class="incident-block-heading">
                    <h3 class="incident-block-title">
                        <Link<Route> to={Route::Incident { id: incident.id.clone() }} classes="incident-link">
                            {&incident.title}
                        </Link<Route>>
                    </h3>
                    <span class="incident-id">{format!("#{}", incident.id)}</span>
                </div>
                <span class={classes!("incident-streak", status_class)}>{status_text}</span>
            </div>

            if !incident.affected_services.is_empty() {
                <p class="incident-affected">
                    <strong>{"Affected services: "}</strong>{incident.affected_services.join(", ")}
                </p>
            }

            if !incident.description.is_empty() {
                <div class="incident-description markdown">
                    { render_markdown(&incident.description) }
                </div>
            }

            { render_timeline(incident) }

            { props.controls.clone() }
        </article>
    }
}

/// The vertical event timeline: a "Started" key date, the updates as cards anchored at their time,
/// and (if resolved) an "Ended" key date. Each circle is coloured by its impact; the connecting line
/// keeps the preceding circle's colour until the next one.
fn render_timeline(incident: &Incident) -> Html {
    let mut updates = incident.updates.clone();
    updates.sort_by_key(|u| u.timestamp);

    let mut rows: Vec<(&'static str, Html)> = Vec::new();
    rows.push(("start", key_date_body("Started", incident.start_time)));
    for update in &updates {
        rows.push((impact_class(update.impact), update_body(update)));
    }
    if let Some(end) = incident.end_time {
        rows.push(("end", key_date_body("Ended", end)));
    }

    let last = rows.len() - 1;

    html! {
        <ul class="incident-timeline">
            { for rows.into_iter().enumerate().map(|(i, (class, body))| timeline_row(class, i != last, body)) }
        </ul>
    }
}

fn timeline_row(circle_class: &'static str, has_tail: bool, body: Html) -> Html {
    html! {
        <li class="timeline-item">
            <div class="timeline-rail">
                <span class={classes!("timeline-circle", circle_class)}></span>
                if has_tail {
                    <span class={classes!("timeline-tail", circle_class)}></span>
                }
            </div>
            <div class="timeline-body">{ body }</div>
        </li>
    }
}

fn key_date_body(label: &str, time: DateTime<Utc>) -> Html {
    html! {
        <>
            <div class="timeline-keydate">{label}</div>
            <div class="timeline-time">{time_format(time)}</div>
        </>
    }
}

fn update_body(update: &IncidentUpdate) -> Html {
    let class = impact_class(update.impact);
    html! {
        <>
            <div class="timeline-time">{time_format(update.timestamp)}</div>
            <div class={classes!("timeline-card", class)}>
                <span class={classes!("incident-status-pill", class)}>{impact_label(update.impact)}</span>
                <div class="timeline-card-message markdown">{ render_markdown(&update.message) }</div>
            </div>
        </>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn incident(end: Option<i64>, updates: Vec<IncidentUpdate>) -> Incident {
        Incident {
            id: "abcd-ef12".into(),
            title: "DB outage".into(),
            description: String::new(),
            start_time: ts(1_700_000_000),
            end_time: end.map(ts),
            affected_services: vec![],
            updates,
            created_at: ts(1_700_000_000),
            updated_at: ts(1_700_000_000),
        }
    }

    fn update(impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate {
            id: format!("u{secs}"),
            impact,
            timestamp: ts(secs),
            message: format!("update at {secs}"),
        }
    }

    /// Renders an `IncidentBlock` inside a router (its title is a `<Link>`), as the app does.
    async fn render(incident: Incident) -> String {
        #[function_component(Harness)]
        fn harness(props: &IncidentBlockProps) -> Html {
            use yew_router::history::{AnyHistory, MemoryHistory};
            let history = AnyHistory::from(MemoryHistory::new());
            html! {
                <Router history={history}>
                    <IncidentBlock incident={props.incident.clone()} controls={Html::default()} />
                </Router>
            }
        }
        yew::ServerRenderer::<Harness>::with_props(move || IncidentBlockProps {
            incident,
            controls: Html::default(),
        })
        .render()
        .await
    }

    #[tokio::test]
    async fn timeline_renders_keydates_updates_and_preceding_colour() {
        let html = render(incident(
            Some(1_700_010_000),
            vec![
                update(Impact::Offline, 1_700_000_100),
                update(Impact::None, 1_700_009_000),
            ],
        ))
        .await;

        assert!(html.contains("Started"), "missing Started key date: {html}");
        assert!(html.contains("Ended"), "missing Ended key date");
        assert!(html.contains("timeline-circle error"), "offline update needs an error circle");
        assert!(html.contains("timeline-circle ok"), "the resolving update needs an ok circle");
        assert!(html.contains("timeline-tail error"), "line after offline must be error-coloured");
        // The nice id and a link to the detail page are present.
        assert!(html.contains("#abcd-ef12"), "missing nice id: {html}");
        assert!(html.contains("/incidents/abcd-ef12"), "missing detail link");
    }

    #[tokio::test]
    async fn header_reports_active_duration_resolution_and_draft() {
        let active = render(incident(None, vec![update(Impact::Offline, 1_700_000_100)])).await;
        assert!(active.contains("Offline for"), "active offline header: {active}");

        let resolved =
            render(incident(Some(1_700_010_000), vec![update(Impact::Offline, 1_700_000_100)])).await;
        assert!(resolved.contains("Resolved"), "resolved header: {resolved}");

        let draft = render(incident(None, vec![])).await;
        assert!(draft.contains("Draft"), "draft header: {draft}");
    }

    #[tokio::test]
    async fn summary_renders_horizontal_markers_with_preceding_colour() {
        #[function_component(SummaryHarness)]
        fn summary_harness(props: &IncidentSummaryProps) -> Html {
            use yew_router::history::{AnyHistory, MemoryHistory};
            let history = AnyHistory::from(MemoryHistory::new());
            html! {
                <Router history={history}>
                    <IncidentSummary incident={props.incident.clone()} />
                </Router>
            }
        }

        let inc = incident(
            None,
            vec![update(Impact::Offline, 100), update(Impact::Degraded, 200)],
        );
        let html = yew::ServerRenderer::<SummaryHarness>::with_props(move || {
            IncidentSummaryProps { incident: inc }
        })
        .render()
        .await;

        assert!(html.contains("incident-hbar"), "missing horizontal timeline: {html}");
        assert!(html.contains("hdot error"), "missing offline marker");
        assert!(html.contains("hdot warn"), "missing degraded marker");
        // The line leading into the degraded marker carries the preceding (offline) colour.
        assert!(html.contains("hline error"), "missing preceding-colour line");
        assert!(html.contains("/incidents/abcd-ef12"), "summary title should link to the detail page");
    }

    #[test]
    fn active_summary_picks_worst_active_incident() {
        let degraded = incident(None, vec![update(Impact::Degraded, 1)]);
        let offline = incident(None, vec![update(Impact::Offline, 1)]);
        let resolved = incident(Some(2), vec![update(Impact::Offline, 1)]);

        assert_eq!(active_summary(&[degraded.clone()]).0, "warn");
        assert_eq!(active_summary(&[degraded, offline]).0, "error");
        assert_eq!(active_summary(&[resolved]).0, "ok", "an ended incident is not active");
        assert_eq!(active_summary(&[]).0, "ok");
    }
}
