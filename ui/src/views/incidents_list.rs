use grey_api::IncidentView;
use yew::prelude::*;
use yewi::prelude::*;

use crate::components::IncidentBlock;
use crate::components::filter::use_filter;
use crate::contexts::use_store;
// `Route` is only used by the wasm-only admin list below, so it is unused in the SSR build.
#[cfg(not(feature = "ssr"))]
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
        <>
            <PageHeader title="Incidents" subtitle={html! { { SUBTITLE } }} />
            <IncidentList incidents={store.incidents().to_vec()} />
        </>
    }
}

const SUBTITLE: &str = "Every reported incident and the updates posted as it was worked on.";

#[derive(Properties, PartialEq)]
struct IncidentListProps {
    incidents: Vec<IncidentView>,
}

/// The incidents, narrowed by the app bar's filter (matched against titles and update messages).
#[function_component(IncidentList)]
fn incident_list(props: &IncidentListProps) -> Html {
    let filter = use_filter();

    if props.incidents.is_empty() {
        return html! {
            <EmptyState title="No incidents reported" icon={icons::CHECK} tone={Tone::Ok}>
                {"Everything has been operating normally. Incidents will appear here if a problem is reported."}
            </EmptyState>
        };
    }

    let shown: Vec<&IncidentView> = props
        .incidents
        .iter()
        .filter(|incident| {
            filter.matches(
                std::iter::once(incident.title()).chain(incident.updates.iter().map(|u| u.message.as_str())),
            )
        })
        .collect();
    if shown.is_empty() {
        return html! {
            <EmptyState title="Nothing matches the filter" icon={icons::SEARCH}>
                {"No incident has a title or an update that matches."}
            </EmptyState>
        };
    }

    html! {
        <div class="incidents">
            { for shown.into_iter().map(|incident| html! {
                <IncidentBlock key={incident.id().to_string()} incident={incident.clone()} />
            }) }
        </div>
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
    // True until the admin fetch resolves; lets us defer the empty state until hidden drafts (which
    // aren't in the SSR seed) have had a chance to load.
    let loading = use_state(|| store.incidents().is_empty());

    {
        let client = store.client().clone();
        let incidents = incidents.clone();
        let loading = loading.clone();
        let store = store.clone();
        use_effect_with(props.token.clone(), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                match client.admin_incidents().await {
                    Ok(list) => incidents.set(list),
                    Err(e) => store.set_error(e),
                }
                loading.set(false);
            });
            || ()
        });
    }

    let declare = html! {
        <Button
            variant={ButtonVariant::Primary}
            icon={icons::TRIANGLE_ALERT}
            href={NavTarget::route(Route::NewIncident)}
        >
            {"Declare incident"}
        </Button>
    };

    html! {
        <>
            <PageHeader title="Incidents" subtitle={html! { { SUBTITLE } }} actions={declare} />
            if *loading && incidents.is_empty() {
                // Seed was empty and the admin fetch (which adds hidden drafts) is still in flight;
                // wait for it before deciding between the list and the empty state, so a drafts-only
                // system doesn't briefly flash "no incidents".
                <Loading layout={LoadingLayout::Block} />
            } else {
                <IncidentList incidents={(*incidents).clone()} />
            }
        </>
    }
}
