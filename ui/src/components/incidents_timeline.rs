use crate::contexts::use_incidents;
use crate::routes::Route;
use grey_api::{Incident, IncidentStatus};
use yew::prelude::*;
use yew_router::prelude::*;

/// The CSS status class used by timeline dots and status pills for an incident status.
pub fn incident_status_class(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Healthy => "ok",
        IncidentStatus::Degraded => "warning",
        IncidentStatus::Offline => "error",
        IncidentStatus::Unknown => "unknown",
    }
}

/// A human-readable label for an incident status.
pub fn incident_status_label(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Healthy => "Healthy",
        IncidentStatus::Degraded => "Degraded",
        IncidentStatus::Offline => "Offline",
        IncidentStatus::Unknown => "Unknown",
    }
}

/// Formats an incident's time span for compact display ("started → resolved", or "→ ongoing").
pub fn format_range(incident: &Incident) -> String {
    let start = incident.start_time.format("%Y-%m-%d %H:%M UTC");
    match incident.end_time {
        Some(end) => format!("{start} → {}", end.format("%Y-%m-%d %H:%M UTC")),
        None => format!("{start} → ongoing"),
    }
}

/// The incidents timeline shown beneath the probes on the status page. Reuses the notices-timeline
/// visual language so the two read consistently.
#[function_component(IncidentsTimeline)]
pub fn incidents_timeline() -> Html {
    let incidents_ctx = use_incidents();

    if incidents_ctx.incidents.is_empty() {
        return html! {};
    }

    html! {
        <div class="incidents-timeline notices-timeline">
            <div class="incidents-timeline-header">
                <h2>{"Incidents"}</h2>
                <Link<Route> to={Route::Incidents} classes="incidents-view-all">{"View all"}</Link<Route>>
            </div>
            <div class="timeline-line"></div>
            { for incidents_ctx.incidents.iter().map(|incident| html! {
                <IncidentTimelineItem key={incident.id.clone()} incident={incident.clone()} />
            }) }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct IncidentTimelineItemProps {
    incident: Incident,
}

#[function_component(IncidentTimelineItem)]
fn incident_timeline_item(props: &IncidentTimelineItemProps) -> Html {
    let incident = &props.incident;
    let status = incident.current_status();
    let status_class = incident_status_class(status);

    html! {
        <div class={classes!("timeline-item", status_class)}>
            <div class="timeline-dot-container">
                <div class={classes!("timeline-dot", status_class)}></div>
            </div>
            <div class="timeline-content">
                <div class="notice-header">
                    <h3>
                        <Link<Route> to={Route::Incidents} classes="incident-link">{&incident.title}</Link<Route>>
                    </h3>
                    <span class={classes!("incident-status-pill", status_class)}>
                        {incident_status_label(status)}
                    </span>
                </div>
                <span class="notice-timestamp">{format_range(incident)}</span>
            </div>
        </div>
    }
}
