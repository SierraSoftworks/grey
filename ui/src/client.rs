use chrono::Datelike;
#[cfg(feature = "wasm")]
use grey_api::UiConfig;
use yew::prelude::*;

use crate::contexts::StoreProvider;
use crate::routes::Route;
use crate::views::{
    AuthCallback, AuthLogout, HomeView, IncidentDetail, IncidentsList, NewIncident,
};

use super::components::{ErrorBanner, Header};

/// The props that seed the app: the public config plus the server-rendered snapshot of the publicly
/// visible live entities. On the client these come from the `#app` element's data attributes (see
/// [`AppProps::from_dom`]); during SSR the agent supplies them directly.
///
/// Operator-only data (the cluster peers) is deliberately absent: it is never server-rendered, and is
/// fetched client-side only once an administrator is signed in, so it can never leak into the page
/// delivered to an unauthenticated viewer.
#[derive(Default, Properties, PartialEq)]
pub struct AppProps {
    pub config: grey_api::UiConfig,
    pub notices: Vec<grey_api::UiNotice>,
    pub probes: Vec<grey_api::Probe>,
    #[prop_or_default]
    pub crons: Vec<grey_api::Cron>,
    #[prop_or_default]
    pub incidents: Vec<grey_api::Incident>,
    /// The request path, used to seed the router during server-side rendering so a deep link to a
    /// non-home route renders the right page (and hydrates cleanly). Unused on the client, where the
    /// browser's location drives the router.
    #[prop_or_default]
    pub url: String,
}

impl AppProps {
    #[cfg(feature = "wasm")]
    pub fn from_dom_minimal() -> Result<Self, Box<dyn std::error::Error>> {
        use web_sys::window;

        let window = window().ok_or("No window found")?;
        let document = window.document().ok_or("No document found")?;
        let app_element = document.get_element_by_id("app").ok_or("#app not found")?;

        let config_data = app_element
            .get_attribute("data-config")
            .ok_or("#app[data-config] not found")?;
        let config: UiConfig = serde_json::from_str(&config_data)?;

        Ok(Self {
            config,
            notices: Vec::new(),
            probes: Vec::new(),
            crons: Vec::new(),
            incidents: Vec::new(),
            url: String::new(),
        })
    }

    #[cfg(feature = "wasm")]
    pub fn from_dom() -> Result<Self, Box<dyn std::error::Error>> {
        use web_sys::window;

        let window = window().ok_or("No window found")?;
        let document = window.document().ok_or("No document found")?;
        let app_element = document.get_element_by_id("app").ok_or("#app not found")?;

        let config_data = app_element
            .get_attribute("data-config")
            .ok_or("#app[data-config] not found")?;
        let notices_data = app_element
            .get_attribute("data-notices")
            .ok_or("#app[data-notices] not found")?;
        let probes_data = app_element
            .get_attribute("data-probes")
            .ok_or("#app[data-probes] not found")?;
        // Incidents and crons are optional: older server renders omit them.
        let incidents_data = app_element.get_attribute("data-incidents");
        let crons_data = app_element.get_attribute("data-crons");

        let config: UiConfig = serde_json::from_str(&config_data)?;
        let notices: Vec<grey_api::UiNotice> = serde_json::from_str(&notices_data)?;
        let probes: Vec<grey_api::Probe> = serde_json::from_str(&probes_data)?;
        let incidents: Vec<grey_api::Incident> = incidents_data
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();
        let crons: Vec<grey_api::Cron> = crons_data
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();

        Ok(Self {
            config,
            notices,
            probes,
            crons,
            incidents,
            url: String::new(),
        })
    }
}

/// The application root. It renders the `#app` element (carrying the SSR snapshot as data attributes
/// so the client can hydrate), then hands every piece of state to the [`StoreProvider`], which owns
/// it from there on (including the background polling and session bootstrap).
#[function_component(App)]
pub fn app(props: &AppProps) -> Html {
    let config_json = serde_json::to_string(&props.config).unwrap_or_default();
    let notices_json = serde_json::to_string(&props.notices).unwrap_or_default();
    let probes_json = serde_json::to_string(&props.probes).unwrap_or_default();
    let crons_json = serde_json::to_string(&props.crons).unwrap_or_default();
    let incidents_json = serde_json::to_string(&props.incidents).unwrap_or_default();

    html! {
        <div id="app"
            data-config={config_json}
            data-notices={notices_json}
            data-probes={probes_json}
            data-crons={crons_json}
            data-incidents={incidents_json}
        >
            <StoreProvider
                config={props.config.clone()}
                notices={props.notices.clone()}
                probes={props.probes.clone()}
                crons={props.crons.clone()}
                incidents={props.incidents.clone()}
            >
                { render_router(&props.url) }
            </StoreProvider>
        </div>
    }
}

/// On the client the browser's history drives the router.
#[cfg(feature = "wasm")]
fn render_router(_url: &str) -> Html {
    use yew_router::prelude::*;

    html! {
        <BrowserRouter>
            <AppContent />
        </BrowserRouter>
    }
}

/// During SSR there is no browser history, so seed an in-memory history from the request path so the
/// correct route is server-rendered (and the client hydrates without a mismatch).
#[cfg(not(feature = "wasm"))]
fn render_router(url: &str) -> Html {
    use yew_router::Router;
    use yew_router::history::{AnyHistory, History, MemoryHistory};

    let history = AnyHistory::from(MemoryHistory::new());
    history.push(if url.is_empty() { "/" } else { url });

    html! {
        <Router history={history}>
            <AppContent />
        </Router>
    }
}

// Layout shared by every route: the header, the routed page, and the footer.
#[function_component(AppContent)]
fn app_content() -> Html {
    use yew_router::prelude::*;

    html! {
        <>
            <Header/>

            <ErrorBanner/>

            <Switch<Route> render={switch} />

            <footer>
                <p>{format!("Copyright © {} Sierra Softworks", chrono::Utc::now().year())}</p>
            </footer>
        </>
    }
}

fn switch(route: Route) -> Html {
    match route {
        Route::Home => html! { <HomeView /> },
        Route::Incidents => html! { <IncidentsList /> },
        Route::NewIncident => html! { <NewIncident /> },
        Route::Incident { id } => html! { <IncidentDetail id={id} /> },
        Route::AuthCallback => html! { <AuthCallback /> },
        Route::AuthLogout => html! { <AuthLogout /> },
        Route::NotFound => html! { <yew_router::prelude::Redirect<Route> to={Route::Home} /> },
    }
}
