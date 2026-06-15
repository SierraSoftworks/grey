use crate::components::incidents_timeline::{incident_status_class, incident_status_label};
use crate::components::markdown::render_markdown;
use crate::contexts::{use_auth, use_incidents};
use chrono::{DateTime, Utc};
use grey_api::{Incident, IncidentUpdate};
use yew::prelude::*;

/// The full incident list. Signed-in administrators get the management view (including hidden
/// incidents and editing controls); everyone else sees the read-only public list.
#[function_component(IncidentsPage)]
pub fn incidents_page() -> Html {
    let auth = use_auth();
    let incidents_ctx = use_incidents();

    #[cfg(feature = "wasm")]
    if auth.is_authenticated() {
        if let Some(token) = auth.token.clone() {
            return html! { <crate::components::incidents_admin::AdminIncidents token={token} /> };
        }
    }
    #[cfg(not(feature = "wasm"))]
    let _ = &auth;

    html! {
        <div class="content incidents-page">
            <h1>{"Incidents"}</h1>
            if incidents_ctx.incidents.is_empty() {
                <p class="incidents-empty">{"No incidents have been reported."}</p>
            } else {
                { for incidents_ctx.incidents.iter().map(|incident| html! {
                    <IncidentCard key={incident.id.clone()} incident={incident.clone()} />
                }) }
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct IncidentCardProps {
    pub incident: Incident,
    /// Optional admin controls rendered at the foot of the card.
    #[prop_or_default]
    pub controls: Html,
}

#[function_component(IncidentCard)]
pub fn incident_card(props: &IncidentCardProps) -> Html {
    let incident = &props.incident;
    let status = incident.current_status();
    let status_class = incident_status_class(status);

    html! {
        <article class={classes!("incident-card", status_class)}>
            <header class="incident-card-header">
                <h2>{&incident.title}</h2>
                <span class={classes!("incident-status-pill", status_class)}>
                    {incident_status_label(status)}
                </span>
            </header>

            <dl class="incident-times">
                { time_row("Started", Some(incident.start_time)) }
                { time_row("Detected", incident.detection_time) }
                { time_row("Mitigated", incident.mitigation_time) }
                { time_row("Resolved", incident.end_time) }
            </dl>

            if !incident.description.is_empty() {
                <div class="incident-description markdown">
                    { render_markdown(&incident.description) }
                </div>
            }

            if !incident.affected_services.is_empty() {
                <p class="incident-affected">
                    <strong>{"Affected services: "}</strong>{incident.affected_services.join(", ")}
                </p>
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
        </article>
    }
}

#[derive(Properties, PartialEq)]
struct IncidentUpdateItemProps {
    update: IncidentUpdate,
}

#[function_component(IncidentUpdateItem)]
fn incident_update_item(props: &IncidentUpdateItemProps) -> Html {
    let update = &props.update;
    let status_class = incident_status_class(update.status);

    html! {
        <li class={classes!("incident-update", status_class)}>
            <div class="incident-update-meta">
                <span class={classes!("incident-status-pill", status_class)}>
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
