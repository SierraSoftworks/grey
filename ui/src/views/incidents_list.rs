use grey_api::Incident;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::components::IncidentBlock;
use crate::contexts::{use_auth, use_incidents};
use crate::routes::Route;

/// The `/incidents` page: the full, read-only history of incidents and their updates. Editing and
/// posting updates happen on each incident's own page, not here. Administrators additionally see
/// hidden (draft) incidents and a button to start a new one.
#[function_component(IncidentsList)]
pub fn incidents_list() -> Html {
    let auth = use_auth();
    let incidents_ctx = use_incidents();

    #[cfg(feature = "wasm")]
    if auth.is_authenticated() {
        if let Some(token) = auth.token.clone() {
            return html! { <AdminIncidentsList token={token} /> };
        }
    }
    #[cfg(not(feature = "wasm"))]
    let _ = &auth;

    html! {
        <div class="page">
            <h1>{"Incidents"}</h1>
            { incident_list_body(&incidents_ctx.incidents) }
        </div>
    }
}

fn incident_list_body(incidents: &[Incident]) -> Html {
    if incidents.is_empty() {
        return html! { <p class="empty-state">{"No incidents have been reported."}</p> };
    }
    html! {
        { for incidents.iter().map(|incident| html! {
            <IncidentBlock key={incident.id.to_string()} incident={incident.clone()} />
        }) }
    }
}

/// The admin variant fetches every incident (including hidden) and offers a "New incident" action.
#[cfg(feature = "wasm")]
#[derive(Properties, PartialEq)]
struct AdminIncidentsListProps {
    token: String,
}

#[cfg(feature = "wasm")]
#[function_component(AdminIncidentsList)]
fn admin_incidents_list(props: &AdminIncidentsListProps) -> Html {
    // Seed from the shared in-memory list so the page shows immediately (reflecting any just-made
    // create/edit/delete), then fetch the full admin list in the background to include hidden drafts.
    let ctx = use_incidents();
    let incidents = use_state(|| ctx.incidents.clone());
    let error = use_state(|| Option::<String>::None);

    {
        let client = use_auth().client;
        let incidents = incidents.clone();
        let error = error.clone();
        use_effect_with(props.token.clone(), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                match client.admin_incidents().await {
                    Ok(list) => incidents.set(list),
                    Err(e) => error.set(Some(e.to_string())),
                }
            });
            || ()
        });
    }

    html! {
        <div class="page">
            <div class="incidents-list-header">
                <h1>{"Incidents"}</h1>
                <Link<Route> to={Route::NewIncident} classes="declare-incident">
                    { crate::components::icons::warning_icon() }
                    <span>{"Declare Incident"}</span>
                </Link<Route>>
            </div>
            if let Some(err) = (*error).clone() {
                <p class="error-text">{err}</p>
            }
            { incident_list_body(&incidents) }
        </div>
    }
}
