//! Shared, read-only incident display:
//! - [`IncidentSummary`] — a compact entry (title + horizontal timeline with hover popovers) used on
//!   the landing page.
//! - [`IncidentBlock`] — the full vertical event timeline used on the incidents list.
//! - [`IncidentsSection`] — the landing-page wrapper over the summaries.
//! Editing lives on the per-incident page (see `views::incident_detail`), not here.

use crate::components::incident_timeline::{HorizontalTimeline, VerticalTimeline};
use crate::components::markdown::render_markdown;
use crate::formatters::{date_format, incident_status};
use crate::routes::Route;
use chrono::Utc;
use grey_api::{Impact, Incident};
use yew::prelude::*;
use yew_router::prelude::*;

/// The worst impact among the incidents (used for the landing-page top-line status).
pub fn worst_impact(incidents: &[Incident]) -> Option<Impact> {
    incidents
        .iter()
        .filter(|i| i.is_active())
        .map(|i| i.current_impact())
        .max_by_key(|impact| impact.rank())
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
            <div class="incidents-section__summaries">
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
            <div class="incident-summary__header">
                <h3 class="incident-summary__title">
                    <span class="incident-summary__timestamp">{date_format(incident.started_at().unwrap_or_else(Utc::now))}</span>

                    <Link<Route> to={Route::Incident { id: incident.id.to_string() }} classes="incident-link">
                        {&incident.title}
                    </Link<Route>>
                </h3>
                <span class={classes!("incident-streak", status_class)}>{status_text}</span>
            </div>
            if let Some(message) = latest_message {
                <div class="incident-summary__message markdown">{ render_markdown(&message) }</div>
            }
            <HorizontalTimeline incident={incident.clone()} />
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
            <div class="incident-block__header">
                <div class="incident-block__heading">
                    <h3 class="incident-block__title">
                        <span class="incident-summary__timestamp">{date_format(incident.started_at().unwrap_or_else(Utc::now))}</span>

                        <Link<Route> to={Route::Incident { id: incident.id.to_string() }} classes="incident-link">
                            {&incident.title}
                        </Link<Route>>
                    </h3>
                    // <span class="incident-id">{format!("{}", incident.id)}</span>
                </div>
                <span class={classes!("incident-streak", status_class)}>{status_text}</span>
            </div>
            <VerticalTimeline updates={incident.updates.clone()} />
        </article>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use grey_api::{Identifier, IncidentUpdate};

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
        assert!(html.contains("incident-timeline__circle error"), "offline circle: {html}");
        assert!(html.contains("incident-timeline__circle ok"), "resolving circle");
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
