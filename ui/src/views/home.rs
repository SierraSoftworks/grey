use chrono::Utc;
use grey_api::Impact;
use yew::prelude::*;
use yewi::prelude::*;

use crate::components::incidents::worst_impact;
use crate::components::{IncidentsSection, ServiceList};
use crate::contexts::use_store;

/// The status page: a top-line banner, the probe list, and recent/active incidents. The top-line
/// status reflects the worst active incident when there is one, otherwise it is derived from probe
/// health.
#[function_component(HomeView)]
pub fn home_view() -> Html {
    let store = use_store();

    // Probe-derived status, used when there are no active incidents.
    let total = store.probes().len();
    let healthy = store.probes().iter().filter(|p| p.passing()).count();
    let probe_status = if total == 0 || healthy == total {
        (Tone::Ok, "All services operating normally")
    } else if healthy * 2 >= total {
        (Tone::Warn, "Partial degradation in service")
    } else {
        (Tone::Danger, "Major outage affecting multiple services")
    };

    // An active incident takes over the top-line status.
    let (tone, status_text) = match worst_impact(store.incidents()) {
        Some(Impact::Offline) => (Tone::Danger, "Major incident affecting service"),
        Some(Impact::Degraded) => (Tone::Warn, "Active incident affecting service"),
        _ => probe_status,
    };

    // Show incidents that are active or were updated recently (the probe-history window).
    let cutoff = Utc::now() - chrono::Duration::days(14);
    let recent_incidents: Vec<grey_api::IncidentView> = store
        .incidents()
        .iter()
        .filter(|incident| {
            incident.is_active() || incident.last_updated().map(|t| t >= cutoff).unwrap_or(false)
        })
        .cloned()
        .collect();

    html! {
        <>
            <Banner class="status-banner" {tone} bold=true size={Size::Large}>{ status_text }</Banner>
            <ServiceList />
            <IncidentsSection incidents={recent_incidents} />
        </>
    }
}
