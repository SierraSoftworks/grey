use crate::components::IncidentBlock;
use crate::contexts::{use_auth, use_incidents};
use yew::prelude::*;

/// The full incident list. Signed-in administrators get the management view; everyone else sees a
/// lightweight list of incident cards (each linking to its own page).
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
                    <IncidentBlock key={incident.id.clone()} incident={incident.clone()} />
                }) }
            }
        </div>
    }
}
