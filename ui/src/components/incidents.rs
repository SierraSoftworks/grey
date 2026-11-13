//! Shared, read-only incident display:
//! - [`IncidentSummary`] — a compact entry (title + horizontal timeline with hover popovers) used on
//!   the landing page.
//! - [`IncidentBlock`] — the full vertical event timeline used on the incidents list.
//! - [`IncidentsSection`] — the landing-page wrapper over the summaries.
//! Editing lives on the per-incident page (see `views::incident_detail`), not here.

use crate::components::incident_timeline::{HorizontalTimeline, VerticalTimeline};
use crate::formatters::{date_format, incident_status};
use crate::routes::Route;
use chrono::Utc;
use grey_api::{Impact, IncidentView};
use yew::prelude::*;
use yewi::prelude::*;

/// The worst impact among the incidents (used for the landing-page top-line status).
pub fn worst_impact(incidents: &[IncidentView]) -> Option<Impact> {
    incidents
        .iter()
        .filter(|i| i.is_active())
        .map(|i| i.current_impact())
        .max_by_key(|impact| impact.rank())
}

/// The landing-page incidents section: a list of compact summaries under a heading. Renders nothing
/// when there are no incidents to show.
#[derive(Properties, PartialEq)]
pub struct IncidentsSectionProps {
    pub incidents: Vec<IncidentView>,
}

#[function_component(IncidentsSection)]
pub fn incidents_section(props: &IncidentsSectionProps) -> Html {
    if props.incidents.is_empty() {
        return html! {};
    }

    html! {
        <section class="incidents-section">
            <div class="incidents-section__heading">
                <Heading level={HeadingLevel::H2} size={TextSize::Lg}>{ "Recent incidents" }</Heading>
                <Link href={NavTarget::route(Route::Incidents)} variant={LinkVariant::Subtle}>{ "All incidents" }</Link>
            </div>
            { for props.incidents.iter().map(|incident| html! {
                <IncidentSummary key={incident.id().to_string()} incident={incident.clone()} />
            }) }
        </section>
    }
}

/// The heading shared by both incident layouts: when it started, its title (a link to its page) and
/// where it stands now.
fn incident_header(incident: &IncidentView) -> Html {
    let (status_text, tone) = incident_status(incident);

    html! {
        <div class="incident__header">
            <h3 class="incident__title">
                <span class="incident__date">{ date_format(incident.started_at().unwrap_or_else(Utc::now)) }</span>
                <Link
                    href={NavTarget::route(Route::Incident { id: incident.id().to_string() })}
                    variant={LinkVariant::Subtle}
                    underline={LinkUnderline::Hover}
                >
                    { incident.title() }
                </Link>
            </h3>
            <Pill {tone} active={incident.is_active()}>{ status_text }</Pill>
        </div>
    }
}

/// A compact incident summary: title, current status, and a full-width horizontal timeline of
/// impact-coloured markers, each revealing its update in a popover on hover.
#[derive(Properties, PartialEq)]
pub struct IncidentSummaryProps {
    pub incident: IncidentView,
}

#[function_component(IncidentSummary)]
pub fn incident_summary(props: &IncidentSummaryProps) -> Html {
    let incident = &props.incident;
    // The most recent update's message, shown as a one-glance summary of where the incident stands.
    let latest_message = incident.sorted_updates().last().map(|u| u.message.clone());

    html! {
        <article class="incident incident--summary">
            { incident_header(incident) }
            if let Some(message) = latest_message {
                <Markdown class="incident__message" source={message} top_heading={HeadingLevel::H4} />
            }
            <HorizontalTimeline incident={incident.clone()} />
        </article>
    }
}

/// A single incident in full: its title, status, and the vertical timeline of its updates.
#[derive(Properties, PartialEq)]
pub struct IncidentBlockProps {
    pub incident: IncidentView,
}

#[function_component(IncidentBlock)]
pub fn incident_block(props: &IncidentBlockProps) -> Html {
    let incident = &props.incident;

    html! {
        <article class="incident">
            { incident_header(incident) }
            <VerticalTimeline updates={incident.updates.clone()} />
        </article>
    }
}

/// A placeholder standing in for one [`IncidentBlock`] while it loads: a header over a few update
/// cards, so the page reserves its eventual layout.
#[function_component(IncidentBlockSkeleton)]
pub fn incident_block_skeleton() -> Html {
    html! {
        <div class="incident incident--skeleton" aria-hidden="true">
            <div class="incident__header">
                <Skeleton width="18rem" />
                <Skeleton width="5rem" />
            </div>
            <Stack gap={Space::Lg}>
                { for (0..3).map(|i| html! {
                    <Stack key={i} gap={Space::Xs}>
                        <Skeleton width="7rem" />
                        <Skeleton lines=2 />
                    </Stack>
                }) }
            </Stack>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use grey_api::{Identifier, Incident, IncidentUpdate, IncidentUpdateId};

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn update(impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate {
            id: IncidentUpdateId::compose(Identifier::from(1_234_567u64), secs as u64),
            impact,
            timestamp: ts(secs),
            message: format!("update at {secs}"),
            version: (secs * 1000) as u64,
            deleted: false,
        }
    }

    fn incident(updates: Vec<IncidentUpdate>) -> IncidentView {
        IncidentView::new(
            Incident { id: Identifier::from(1_234_567u64), title: "DB outage".into(), version: 1, deleted: false },
            updates,
        )
    }

    async fn render_block(incident: IncidentView) -> String {
        #[function_component(Harness)]
        fn harness(props: &IncidentBlockProps) -> Html {
            use yew_router::Router;
            use yew_router::history::{AnyHistory, MemoryHistory};
            let history = AnyHistory::from(MemoryHistory::new());
            html! { <Router history={history}><IncidentBlock incident={props.incident.clone()} /></Router> }
        }
        yew::ServerRenderer::<Harness>::with_props(move || IncidentBlockProps { incident })
            .render()
            .await
    }

    #[tokio::test]
    async fn block_renders_timeline_and_link() {
        let html = render_block(incident(vec![
            update(Impact::Offline, 100),
            update(Impact::None, 200),
        ]))
        .await;
        assert!(html.contains("update at 100") && html.contains("update at 200"), "every update: {html}");
        assert!(html.find("update at 200") < html.find("update at 100"), "newest update first: {html}");
        assert!(html.contains("Offline"), "each update names its impact: {html}");
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
