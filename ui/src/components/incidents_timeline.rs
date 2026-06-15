//! Shared incident display: status/state helpers, the per-incident block, and the incidents section
//! (a colour-coded header over a stack of incident blocks) that mirrors the probe layout. Used by
//! the home page, the public incidents page, and (via [`IncidentBlock`]) the admin management view.

use crate::components::markdown::render_markdown;
use chrono::{DateTime, Utc};
use grey_api::{Incident, IncidentState, IncidentStatus, IncidentUpdate};
use yew::prelude::*;

/// The status class used for an incident **update** pill. Matches the section/banner colour classes
/// (`ok`/`warn`/`error`/`unknown`) used elsewhere.
pub fn incident_status_class(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Healthy => "ok",
        IncidentStatus::Degraded => "warn",
        IncidentStatus::Offline => "error",
        IncidentStatus::Unknown => "unknown",
    }
}

pub fn incident_status_label(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Healthy => "Healthy",
        IncidentStatus::Degraded => "Degraded",
        IncidentStatus::Offline => "Offline",
        IncidentStatus::Unknown => "Unknown",
    }
}

/// The colour class for an incident's overall state. `draft` has its own muted styling.
pub fn incident_state_class(state: IncidentState) -> &'static str {
    match state {
        IncidentState::Draft => "draft",
        IncidentState::Healthy => "ok",
        IncidentState::Degraded => "warn",
        IncidentState::Offline => "error",
        IncidentState::Unknown => "unknown",
    }
}

pub fn incident_state_label(state: IncidentState) -> &'static str {
    match state {
        IncidentState::Draft => "Draft",
        IncidentState::Healthy => "Healthy",
        IncidentState::Degraded => "Degraded",
        IncidentState::Offline => "Offline",
        IncidentState::Unknown => "Unknown",
    }
}

/// Summarises the section header: a colour class and a label reflecting any active incidents.
pub fn active_summary(incidents: &[Incident]) -> (&'static str, String) {
    let active = incidents.iter().filter(|i| i.is_active()).count();
    if active == 0 {
        ("ok", "No active incidents".to_string())
    } else {
        let any_offline = incidents
            .iter()
            .any(|i| i.is_active() && i.state == IncidentState::Offline);
        let class = if any_offline { "error" } else { "warn" };
        let plural = if active == 1 { "" } else { "s" };
        (class, format!("{active} active incident{plural}"))
    }
}

/// A colour-coded header plus a block per incident, mirroring the probe section. When the list is
/// empty it renders nothing unless an `empty_message` is supplied (the dedicated incidents page).
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

/// A single incident rendered as a status-bordered block (a `.section`, like a probe service group).
/// `controls` lets the admin view graft management buttons onto the foot of the block.
#[derive(Properties, PartialEq)]
pub struct IncidentBlockProps {
    pub incident: Incident,
    #[prop_or_default]
    pub controls: Html,
}

#[function_component(IncidentBlock)]
pub fn incident_block(props: &IncidentBlockProps) -> Html {
    let incident = &props.incident;
    let class = incident_state_class(incident.state);

    html! {
        <div class={classes!("section", "incident-block", class)}>
            <div class="incident-block-header">
                <h3 class="incident-block-title">{&incident.title}</h3>
                <span class={classes!("incident-status-pill", class)}>
                    {incident_state_label(incident.state)}
                </span>
            </div>

            <dl class="incident-times">
                { time_row("Started", Some(incident.start_time)) }
                { time_row("Detected", incident.detection_time) }
                { time_row("Mitigated", incident.mitigation_time) }
                { time_row("Resolved", incident.end_time) }
            </dl>

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

            if !incident.updates.is_empty() {
                <ol class="incident-updates">
                    { for sorted_updates(incident).into_iter().map(|update| {
                        let key = update.id.clone();
                        html! { <IncidentUpdateItem key={key} update={update} /> }
                    }) }
                </ol>
            }

            { props.controls.clone() }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct IncidentUpdateItemProps {
    update: IncidentUpdate,
}

#[function_component(IncidentUpdateItem)]
fn incident_update_item(props: &IncidentUpdateItemProps) -> Html {
    let update = &props.update;
    let class = incident_status_class(update.status);

    html! {
        <li class={classes!("incident-update", class)}>
            <div class="incident-update-meta">
                <span class={classes!("incident-status-pill", class)}>
                    {incident_status_label(update.status)}
                </span>
                <span class="incident-update-time">
                    {update.timestamp.format("%Y-%m-%d %H:%M UTC").to_string()}
                </span>
            </div>
            <div class="incident-update-message markdown">
                { render_markdown(&update.message) }
            </div>
        </li>
    }
}

fn time_row(label: &str, time: Option<DateTime<Utc>>) -> Html {
    match time {
        Some(t) => html! {
            <>
                <dt>{label}</dt>
                <dd>{t.format("%Y-%m-%d %H:%M UTC").to_string()}</dd>
            </>
        },
        None => html! {},
    }
}

/// Updates newest-first for the detail view.
fn sorted_updates(incident: &Incident) -> Vec<IncidentUpdate> {
    let mut updates = incident.updates.clone();
    updates.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    updates
}
