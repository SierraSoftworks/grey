//! Page-level views, one per route. Reusable building blocks live in [`crate::components`].

mod auth_callback;
mod auth_logout;
#[cfg(debug_assertions)]
mod controls;
mod home;
mod incident_detail;
mod incidents_list;
mod new_incident;

pub use auth_callback::AuthCallback;
pub use auth_logout::AuthLogout;
#[cfg(debug_assertions)]
pub use controls::ControlsView;
pub use home::HomeView;
pub use incident_detail::IncidentDetail;
pub use incidents_list::IncidentsList;
pub use new_incident::NewIncident;
