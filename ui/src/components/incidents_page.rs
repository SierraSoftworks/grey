use crate::components::incidents_timeline::IncidentsSection;
use crate::contexts::{use_auth, use_incidents};
use yew::prelude::*;

/// The full incident list. Signed-in administrators get the management view; everyone else sees the
/// read-only public list rendered as status blocks under a colour-coded header.
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
        <IncidentsSection
            incidents={incidents_ctx.incidents.clone()}
            empty_message={AttrValue::from("No incidents have been reported.")}
        />
    }
}
