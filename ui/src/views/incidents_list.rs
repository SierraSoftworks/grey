use grey_api::Incident;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::components::IncidentBlock;
use crate::contexts::use_store;
use crate::routes::Route;

/// The `/incidents` page: the full, read-only history of incidents and their updates. Editing and
/// posting updates happen on each incident's own page, not here. Administrators additionally see
/// hidden (draft) incidents and a button to start a new one.
#[function_component(IncidentsList)]
pub fn incidents_list() -> Html {
    let store = use_store();

    #[cfg(feature = "wasm")]
    if store.is_authenticated() {
        if let Some(token) = store.token() {
            return html! { <AdminIncidentsList token={token} /> };
        }
    }
    #[cfg(not(feature = "wasm"))]
    let _ = &store;

    html! {
        <div class="page">
            <h1>{"Incidents"}</h1>
            { incident_list_body(store.incidents()) }
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
    let store = use_store();
    let incidents = use_state(|| store.incidents().to_vec());

    {
        let client = store.client().clone();
        let incidents = incidents.clone();
        let store = store.clone();
        use_effect_with(props.token.clone(), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                match client.admin_incidents().await {
                    Ok(list) => incidents.set(list),
                    Err(e) => store.set_error(e),
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
            { incident_list_body(&incidents) }
        </div>
    }
}
