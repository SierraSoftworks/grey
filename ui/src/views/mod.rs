//! Page-level views, one per route. Reusable building blocks live in [`crate::components`].

mod home;
mod incident_detail;
mod incidents_list;
mod new_incident;

pub use home::HomeView;
pub use incident_detail::IncidentDetail;
pub use incidents_list::IncidentsList;
pub use new_incident::NewIncident;
