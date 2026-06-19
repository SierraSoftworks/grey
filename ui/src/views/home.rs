use chrono::Utc;
use grey_api::Impact;
use yew::prelude::*;

use crate::components::incidents::worst_impact;
use crate::components::{Banner, BannerKind, IncidentsSection, ServiceList};
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
        (BannerKind::Ok, "All services operating normally")
    } else if healthy * 2 >= total {
        (BannerKind::Warning, "Partial degradation in service")
    } else {
        (BannerKind::Error, "Major outage affecting multiple services")
    };

    // An active incident takes over the top-line status.
    let (banner_kind, status_text) = match worst_impact(store.incidents()) {
        Some(Impact::Offline) => (BannerKind::Error, "Major incident affecting service"),
        Some(Impact::Degraded) => (BannerKind::Warning, "Active incident affecting service"),
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
            <div class="content">
                <Banner kind={banner_kind} text={status_text.to_string()} />
                <ServiceList />
            </div>

            <IncidentsSection incidents={recent_incidents} />
        </>
    }
}
