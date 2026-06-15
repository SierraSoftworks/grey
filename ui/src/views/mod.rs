//! Page-level views, one per route. Reusable building blocks live in [`crate::components`].

mod home;
mod incident_detail;
mod incidents_list;
mod new_incident;

pub use home::HomeView;
pub use incident_detail::IncidentDetail;
pub use incidents_list::IncidentsList;
pub use new_incident::NewIncident;

use grey_api::Impact;

/// Parses an impact from a `<select>` value.
pub fn parse_impact(value: &str) -> Impact {
    match value {
        "offline" => Impact::Offline,
        "degraded" => Impact::Degraded,
        "none" => Impact::None,
        _ => Impact::Hidden,
    }
}

/// The `<option>`/`<select>` value for an impact.
pub fn impact_value(impact: Impact) -> &'static str {
    match impact {
        Impact::Offline => "offline",
        Impact::Degraded => "degraded",
        Impact::None => "none",
        Impact::Hidden => "hidden",
    }
}
