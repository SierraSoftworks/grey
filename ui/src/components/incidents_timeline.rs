//! Shared incident display: impact helpers, the per-incident block, and the incidents section (a
//! colour-coded header over a stack of blocks) that mirrors the probe layout. Each block carries an
//! Element-Plus-style timeline of its updates and key dates.

use crate::components::markdown::render_markdown;
use crate::formatters::compact_duration;
use chrono::{DateTime, Utc};
use grey_api::{Impact, Incident, IncidentUpdate};
use yew::prelude::*;

/// The colour class for an impact. Matches the shared `.section.{ok|warn|error}` classes; `hidden`
/// uses the muted draft treatment.
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

/// The header status text shown on the right of an incident block (like a probe's streak): the
/// current impact and how long it has held, e.g. "Offline for 1h". Returns `(text, class)`.
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

/// A colour-coded header plus a block per incident, mirroring the probe section. Empty renders
/// nothing unless an `empty_message` is supplied (the dedicated incidents page).
#[derive(Properties, PartialEq)]
pub struct IncidentsSectionProps {
    pub incidents: Vec<Incident>,
    #[prop_or_default]
    pub empty_message: Option<AttrValue>,
}

#[function_component(IncidentsSection)]
pub fn incidents_section(props: &IncidentsSectionProps) -> Html {
    if props.incidents.is_empty() {
        return match &props.empty_message {
            Some(message) => html! {
                <div class="content incidents-section">
                    <div class="section fill ok"><span class="status ok">{"No active incidents"}</span></div>
                    <div class="section"><p class="incidents-empty">{message.clone()}</p></div>
                </div>
            },
            None => html! {},
        };
    }

    let (class, text) = active_summary(&props.incidents);

    html! {
        <div class="content incidents-section">
            <div class={classes!("section", "fill", class)}>
                <span class={classes!("status", class)}>{text}</span>
            </div>
            { for props.incidents.iter().map(|incident| html! {
                <IncidentBlock key={incident.id.clone()} incident={incident.clone()} />
            }) }
        </div>
    }
}

/// A single incident rendered as a status-bordered block (a `.section`, like a probe service group),
/// with its event timeline. `controls` grafts admin buttons onto the foot of the block.
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
        <div class={classes!("section", "incident-block", class)}>
            <div class="incident-block-header">
                <h3 class="incident-block-title">{&incident.title}</h3>
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
        </div>
    }
}

/// The incident's event timeline: a "Started" key date, the updates as cards anchored at their time,
/// and (if resolved) an "Ended" key date. Each circle is coloured by its impact, and the connecting
/// line keeps the preceding circle's colour until the next one.
fn render_timeline(incident: &Incident) -> Html {
    let mut updates = incident.updates.clone();
    updates.sort_by_key(|u| u.timestamp);

    // Build (circle class, body) rows in chronological order: a neutral "Started" marker, the
    // updates, and an "Ended" marker once resolved.
    let mut rows: Vec<(&'static str, Html)> = Vec::new();
    rows.push(("start", key_date_body("Started", incident.start_time)));
    for update in &updates {
        rows.push((impact_class(update.impact), update_body(update)));
    }
    if let Some(end) = incident.end_time {
        rows.push(("end", key_date_body("Ended", end)));
    }

    // The last row draws no connecting line; every other row's line carries its own colour down to
    // the next circle.
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
            <div class="timeline-time">{time.format("%Y-%m-%d %H:%M UTC").to_string()}</div>
        </>
    }
}

fn update_body(update: &IncidentUpdate) -> Html {
    let class = impact_class(update.impact);
    html! {
        <>
            <div class="timeline-time">{update.timestamp.format("%Y-%m-%d %H:%M UTC").to_string()}</div>
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
            id: "i".into(),
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

    async fn render(incident: Incident) -> String {
        yew::ServerRenderer::<IncidentBlock>::with_props(move || IncidentBlockProps {
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

        // Key dates appear as text; updates carry impact-coloured circles.
        assert!(html.contains("Started"), "missing Started key date: {html}");
        assert!(html.contains("Ended"), "missing Ended key date");
        assert!(html.contains("timeline-circle error"), "offline update needs an error circle");
        assert!(html.contains("timeline-circle ok"), "the resolving update needs an ok circle");
        // The line after the offline circle stays error-coloured until the next circle.
        assert!(html.contains("timeline-tail error"), "line after offline must be error-coloured");
        // Update messages render as cards with status pills.
        assert!(html.contains("incident-status-pill error"));
    }

    #[tokio::test]
    async fn header_reports_active_duration_resolution_and_draft() {
        // Ongoing offline -> "Offline for …".
        let active = render(incident(None, vec![update(Impact::Offline, 1_700_000_100)])).await;
        assert!(active.contains("Offline for"), "active offline header: {active}");

        // Ended -> "Resolved".
        let resolved =
            render(incident(Some(1_700_010_000), vec![update(Impact::Offline, 1_700_000_100)])).await;
        assert!(resolved.contains("Resolved"), "resolved header: {resolved}");

        // No updates -> hidden draft.
        let draft = render(incident(None, vec![])).await;
        assert!(draft.contains("Draft"), "draft header: {draft}");
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
