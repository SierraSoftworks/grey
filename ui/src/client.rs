use chrono::Datelike;
#[cfg(feature = "wasm")]
use grey_api::UiConfig;
use yew::prelude::*;

use crate::components::status::StatusLevel;
use crate::contexts::{
    AuthProvider, IncidentsProvider, NoticesProvider, PeersProvider, ProbesProvider,
    UiConfigProvider,
};
use crate::routes::Route;
use crate::views::{HomeView, IncidentDetail, IncidentsList, NewIncident};

use super::components::Header;

#[cfg(feature = "wasm")]
pub enum ClientMsg {
    UpdateNotices(Vec<grey_api::UiNotice>),
    UpdateProbes(Vec<grey_api::Probe>),
    UpdatePeers(Vec<grey_api::Peer>),
    UpdateIncidents(Vec<grey_api::Incident>),
    /// Optimistically insert/replace a single incident (after an admin create or edit), without
    /// waiting for the next poll.
    UpsertIncident(grey_api::Incident),
    /// Optimistically remove a single incident (after an admin delete).
    RemoveIncident(grey_api::Identifier),
    Error(String),
}

/// Sorts incidents most-recently-updated first (those with no updates sort last), mirroring the
/// server's ordering so optimistic insertions land in the right place.
fn sort_incidents(incidents: &mut [grey_api::Incident]) {
    incidents.sort_by(|a, b| b.last_updated().cmp(&a.last_updated()));
}

/// Inserts or replaces an incident in the shared (public) list. The list mirrors the unauthenticated
/// view, so an incident that is now hidden is dropped rather than shown.
#[cfg(feature = "wasm")]
fn apply_incident_upsert(incidents: &mut Vec<grey_api::Incident>, incident: grey_api::Incident) {
    incidents.retain(|i| i.id != incident.id);
    if incident.is_public() {
        incidents.push(incident);
    }
    sort_incidents(incidents);
}

// Main App component that provides all contexts
pub struct App {
    notices: Vec<grey_api::UiNotice>,
    probes: Vec<grey_api::Probe>,
    peers: Vec<grey_api::Peer>,
    incidents: Vec<grey_api::Incident>,
    has_error: bool,
    // Stable callbacks handed to the IncidentsProvider so descendants can mutate the in-memory list.
    upsert_incident: Callback<grey_api::Incident>,
    remove_incident: Callback<grey_api::Identifier>,
}

// SSR-compatible version that just takes props
#[derive(Default, Properties, PartialEq)]
pub struct AppProps {
    pub config: grey_api::UiConfig,
    pub notices: Vec<grey_api::UiNotice>,
    pub probes: Vec<grey_api::Probe>,
    pub peers: Vec<grey_api::Peer>,
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
            peers: Vec::new(),
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
        // Peers and incidents are optional: a node may not advertise them, and older server renders
        // omit them.
        let peers_data = app_element.get_attribute("data-peers");
        let incidents_data = app_element.get_attribute("data-incidents");

        let config: UiConfig = serde_json::from_str(&config_data)?;
        let notices: Vec<grey_api::UiNotice> = serde_json::from_str(&notices_data)?;
        let probes: Vec<grey_api::Probe> = serde_json::from_str(&probes_data)?;
        let peers: Vec<grey_api::Peer> = peers_data
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();
        let incidents: Vec<grey_api::Incident> = incidents_data
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();

        Ok(Self {
            config,
            notices,
            probes,
            peers,
            incidents,
            url: String::new(),
        })
    }
}

impl Component for App {
    #[cfg(feature = "wasm")]
    type Message = ClientMsg;
    #[cfg(not(feature = "wasm"))]
    type Message = ();

    type Properties = AppProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mut incidents = ctx.props().incidents.clone();
        sort_incidents(&mut incidents);

        // Wire the optimistic-update callbacks to messages on the client; SSR gets inert no-ops.
        #[cfg(feature = "wasm")]
        let upsert_incident = ctx.link().callback(ClientMsg::UpsertIncident);
        #[cfg(feature = "wasm")]
        let remove_incident = ctx.link().callback(ClientMsg::RemoveIncident);
        #[cfg(not(feature = "wasm"))]
        let upsert_incident = Callback::<grey_api::Incident>::noop();
        #[cfg(not(feature = "wasm"))]
        let remove_incident = Callback::<grey_api::Identifier>::noop();

        let app = Self {
            notices: ctx.props().notices.clone(),
            probes: ctx.props().probes.clone(),
            peers: ctx.props().peers.clone(),
            incidents,
            has_error: false,
            upsert_incident,
            remove_incident,
        };

        #[cfg(feature = "wasm")]
        {
            if app.probes.is_empty() {
                // We might not have loaded the un-hydrated context correctly, so let's trigger an immediate refresh
                ctx.link()
                    .send_future(async move { Self::fetch_probes_as_client_msg().await });

                ctx.link()
                    .send_future(async move { Self::fetch_notices_as_client_msg().await });

                ctx.link()
                    .send_future(async move { Self::fetch_incidents_as_client_msg().await });
            } else {
                Self::schedule_next_probes_poll(ctx);
                Self::schedule_next_notices_poll(ctx);
                Self::schedule_next_incidents_poll(ctx);
            }

            // Peers change frequently and are cheap to fetch, so always refresh them on mount; the
            // polling loop is then driven from the update handler.
            ctx.link()
                .send_future(async move { Self::fetch_peers_as_client_msg().await });
        }

        app
    }

    #[cfg(feature = "wasm")]
    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ClientMsg::UpdateProbes(probes) => {
                self.probes = probes;
                // Setup next polling cycle
                Self::schedule_next_probes_poll(ctx);
                true
            }
            ClientMsg::UpdateNotices(notices) => {
                self.notices = notices;
                Self::schedule_next_notices_poll(ctx);
                true
            }
            ClientMsg::UpdatePeers(peers) => {
                let changed = self.peers != peers;
                self.peers = peers;
                Self::schedule_next_peers_poll(ctx);
                changed
            }
            ClientMsg::UpdateIncidents(mut incidents) => {
                sort_incidents(&mut incidents);
                let changed = self.incidents != incidents;
                self.incidents = incidents;
                Self::schedule_next_incidents_poll(ctx);
                changed
            }
            ClientMsg::UpsertIncident(incident) => {
                apply_incident_upsert(&mut self.incidents, incident);
                true
            }
            ClientMsg::RemoveIncident(id) => {
                let before = self.incidents.len();
                self.incidents.retain(|incident| incident.id != id);
                self.incidents.len() != before
            }
            ClientMsg::Error(err) => {
                gloo::console::error!("{}", err);
                self.has_error = true;
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        // Serialize to initial JSON for data attributes
        let config_json = serde_json::to_string(&ctx.props().config).unwrap_or_default();
        let notices_json = serde_json::to_string(&ctx.props().notices).unwrap_or_default();
        let probes_json = serde_json::to_string(&ctx.props().probes).unwrap_or_default();
        let peers_json = serde_json::to_string(&ctx.props().peers).unwrap_or_default();
        let incidents_json = serde_json::to_string(&ctx.props().incidents).unwrap_or_default();

        html! {
            <div id="app"
                data-config={config_json}
                data-notices={notices_json}
                data-probes={probes_json}
                data-peers={peers_json}
                data-incidents={incidents_json}
            >
                <UiConfigProvider config={ctx.props().config.clone()}>
                    <AuthProvider>
                        <NoticesProvider notices={self.notices.clone()}>
                            <ProbesProvider probes={self.probes.clone()}>
                                <PeersProvider peers={self.peers.clone()}>
                                    <IncidentsProvider
                                        incidents={self.incidents.clone()}
                                        upsert={self.upsert_incident.clone()}
                                        remove={self.remove_incident.clone()}
                                    >
                                        { self.render_router(ctx) }
                                    </IncidentsProvider>
                                </PeersProvider>
                            </ProbesProvider>
                        </NoticesProvider>
                    </AuthProvider>
                </UiConfigProvider>
            </div>
        }
    }
}

impl App {
    /// On the client the browser's history drives the router.
    #[cfg(feature = "wasm")]
    fn render_router(&self, _ctx: &Context<Self>) -> Html {
        use yew_router::prelude::*;

        html! {
            <BrowserRouter>
                <AppContent has_error={self.has_error} />
            </BrowserRouter>
        }
    }

    /// During SSR there is no browser history, so seed an in-memory history from the request path so
    /// the correct route is server-rendered (and the client hydrates without a mismatch).
    #[cfg(not(feature = "wasm"))]
    fn render_router(&self, ctx: &Context<Self>) -> Html {
        use yew_router::Router;
        use yew_router::history::{AnyHistory, History, MemoryHistory};

        let history = AnyHistory::from(MemoryHistory::new());
        let path = ctx.props().url.as_str();
        history.push(if path.is_empty() { "/" } else { path });

        html! {
            <Router history={history}>
                <AppContent has_error={self.has_error} />
            </Router>
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct AppContentProps {
    has_error: bool,
}

// Layout shared by every route: the header, the routed page, and the footer.
#[function_component(AppContent)]
fn app_content(props: &AppContentProps) -> Html {
    use yew_router::prelude::*;

    html! {
        <>
            <Header/>

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
        Route::NotFound => html! { <yew_router::prelude::Redirect<Route> to={Route::Home} /> },
    }
}

#[cfg(feature = "wasm")]
impl App {
    fn schedule_next_probes_poll(ctx: &Context<Self>) {
        let reload_interval = ctx.props().config.reload_interval;
        ctx.link().send_future(async move {
            gloo::timers::future::sleep(reload_interval).await;
            Self::fetch_probes_as_client_msg().await
        });
    }

    async fn fetch_probes_as_client_msg() -> ClientMsg {
        match Self::fetch_probes().await {
            Ok(probes) => ClientMsg::UpdateProbes(probes),
            Err(err) => ClientMsg::Error(format!("Failed to fetch probes: {}", err)),
        }
    }

    fn schedule_next_notices_poll(ctx: &Context<Self>) {
        let reload_interval = ctx.props().config.reload_interval;
        ctx.link().send_future(async move {
            gloo::timers::future::sleep(reload_interval).await;
            Self::fetch_notices_as_client_msg().await
        });
    }

    async fn fetch_notices_as_client_msg() -> ClientMsg {
        match Self::fetch_notices().await {
            Ok(notices) => ClientMsg::UpdateNotices(notices),
            Err(err) => ClientMsg::Error(format!("Failed to fetch notices: {}", err)),
        }
    }

    fn schedule_next_peers_poll(ctx: &Context<Self>) {
        let reload_interval = ctx.props().config.reload_interval;
        ctx.link().send_future(async move {
            gloo::timers::future::sleep(reload_interval).await;
            Self::fetch_peers_as_client_msg().await
        });
    }

    async fn fetch_peers_as_client_msg() -> ClientMsg {
        match Self::fetch_peers().await {
            Ok(peers) => ClientMsg::UpdatePeers(peers),
            Err(err) => ClientMsg::Error(format!("Failed to fetch peers: {}", err)),
        }
    }

    fn schedule_next_incidents_poll(ctx: &Context<Self>) {
        let reload_interval = ctx.props().config.reload_interval;
        ctx.link().send_future(async move {
            gloo::timers::future::sleep(reload_interval).await;
            Self::fetch_incidents_as_client_msg().await
        });
    }

    async fn fetch_incidents_as_client_msg() -> ClientMsg {
        match Self::fetch_incidents().await {
            Ok(incidents) => ClientMsg::UpdateIncidents(incidents),
            Err(err) => ClientMsg::Error(format!("Failed to fetch incidents: {}", err)),
        }
    }

    async fn fetch_peers() -> Result<Vec<grey_api::Peer>, Box<dyn std::error::Error>> {
        let response = gloo::net::http::Request::get("/api/v1/cluster/peers")
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let peers: Vec<grey_api::Peer> = response
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(peers)
    }

    async fn fetch_notices() -> Result<Vec<grey_api::UiNotice>, Box<dyn std::error::Error>> {
        let response = gloo::net::http::Request::get("/api/v1/notices")
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let notices: Vec<grey_api::UiNotice> = response
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(notices)
    }

    async fn fetch_probes() -> Result<Vec<grey_api::Probe>, Box<dyn std::error::Error>> {
        let response = gloo::net::http::Request::get("/api/v1/probes")
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let probes: Vec<grey_api::Probe> = response
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(probes)
    }

    async fn fetch_incidents() -> Result<Vec<grey_api::Incident>, Box<dyn std::error::Error>> {
        let response = gloo::net::http::Request::get("/api/v1/incidents")
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let incidents: Vec<grey_api::Incident> = response
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(incidents)
    }
}
