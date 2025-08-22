use std::collections::HashMap;

use chrono::Datelike;
#[cfg(feature = "wasm")]
use grey_api::UiConfig;
use yew::prelude::*;

use crate::components::status::StatusLevel;
use crate::contexts::{
    use_probe_history, use_probes, NoticesProvider, ProbeHistoryProvider, ProbesProvider,
    UiConfigProvider,
};

use super::components::{Banner, BannerKind, Header, ProbeList, Timeline};

#[cfg(feature = "wasm")]
pub enum ClientMsg {
    UpdateNotices(Vec<grey_api::UiNotice>),
    UpdateProbes(Vec<grey_api::Probe>),
    UpdateProbeHistory(String, Vec<grey_api::ProbeHistory>),
    Error(String),
}

// Main App component that provides all contexts
pub struct App {
    notices: Vec<grey_api::UiNotice>,
    probes: Vec<grey_api::Probe>,
    probe_histories: std::collections::HashMap<String, Vec<grey_api::ProbeHistory>>,
    has_error: bool,
}

// SSR-compatible version that just takes props
#[derive(Default, Properties, PartialEq)]
pub struct AppProps {
    pub config: grey_api::UiConfig,
    pub notices: Vec<grey_api::UiNotice>,
    pub probes: Vec<grey_api::Probe>,
    pub histories: HashMap<String, Vec<grey_api::ProbeHistory>>,
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
            histories: HashMap::new(),
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
        let histories_data = app_element
            .get_attribute("data-probe-histories")
            .ok_or("#app[data-probe-histories] not found")?;

        let config: UiConfig = serde_json::from_str(&config_data)?;
        let notices: Vec<grey_api::UiNotice> = serde_json::from_str(&notices_data)?;
        let probes: Vec<grey_api::Probe> = serde_json::from_str(&probes_data)?;
        let histories: HashMap<String, Vec<grey_api::ProbeHistory>> =
            serde_json::from_str(&histories_data)?;

        Ok(Self {
            config,
            notices,
            probes,
            histories,
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
        let app = Self {
            notices: ctx.props().notices.clone(),
            probes: ctx.props().probes.clone(),
            probe_histories: ctx.props().histories.clone(),
            has_error: false,
        };

        #[cfg(feature = "wasm")]
        if app.probes.is_empty() {
            // We might not have loaded the un-hydrated context correctly, so let's trigger an immediate refresh
            ctx.link()
                .send_future(async move { Self::fetch_probes_as_client_msg().await });

            ctx.link()
                .send_future(async move { Self::fetch_notices_as_client_msg().await });
        } else {
            Self::schedule_next_probes_poll(ctx);
            Self::schedule_next_notices_poll(ctx);
        }

        app
    }

    #[cfg(feature = "wasm")]
    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ClientMsg::UpdateProbes(probes) => {
                self.probes = probes;
                for probe in self.probes.iter() {
                    let probe_name = probe.name.clone();
                    ctx.link().send_future(async move {
                        match Self::fetch_probe_history(&probe_name).await {
                            Ok(history) => ClientMsg::UpdateProbeHistory(probe_name, history),
                            Err(err) => ClientMsg::Error(format!(
                                "Failed to fetch history for {}: {}",
                                probe_name, err
                            )),
                        }
                    });
                }

                // Setup next polling cycle
                Self::schedule_next_probes_poll(ctx);
                true
            }
            ClientMsg::UpdateProbeHistory(probe_name, history) => {
                self.probe_histories.insert(probe_name, history);
                self.has_error = false;
                true
            }
            ClientMsg::UpdateNotices(notices) => {
                self.notices = notices;
                Self::schedule_next_notices_poll(ctx);
                true
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
        let histories_json = serde_json::to_string(&ctx.props().histories).unwrap_or_default();

        html! {
            <div id="app"
                data-config={config_json}
                data-notices={notices_json}
                data-probes={probes_json}
                data-probe-histories={histories_json}
            >
                <UiConfigProvider config={ctx.props().config.clone()}>
                    <NoticesProvider notices={self.notices.clone()}>
                        <ProbesProvider probes={self.probes.clone()}>
                            <ProbeHistoryProvider probe_histories={self.probe_histories.clone()}>
                                <AppContent has_error={self.has_error} />
                            </ProbeHistoryProvider>
                        </ProbesProvider>
                    </NoticesProvider>
                </UiConfigProvider>
            </div>
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct AppContentProps {
    has_error: bool,
}

// Main content component that uses contexts
#[function_component(AppContent)]
fn app_content(props: &AppContentProps) -> Html {
    let probes_ctx = use_probes();
    let history_ctx = use_probe_history();

    let healthy_probes = history_ctx
        .probe_histories
        .iter()
        .filter_map(|(_, history)| history.last())
        .filter(|entry| entry.pass)
        .count();

    let banner_kind = match healthy_probes {
        p if p == probes_ctx.probes.len() => BannerKind::Ok,
        p if p >= probes_ctx.probes.len() / 2 => BannerKind::Warning,
        _ => BannerKind::Error,
    };

    let status_text = match banner_kind {
        BannerKind::Ok => "All services operating normally",
        BannerKind::Warning => "Partial degradation in service",
        BannerKind::Error => "Major outage affecting multiple services",
    };

    html! {
        <>
            <Header
                status={if props.has_error { StatusLevel::Error } else { StatusLevel::Good }}
                status_text={if props.has_error { "Error" } else { "OK" }}
            />

            <div class="content">
                <Banner kind={banner_kind} text={status_text.to_string()} />
                <ProbeList />
            </div>

            <Timeline />

            <footer>
                <p>{format!("Copyright © {} Sierra Softworks", chrono::Utc::now().year())}</p>
            </footer>
        </>
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

    async fn fetch_probe_history(
        probe_name: &str,
    ) -> Result<Vec<grey_api::ProbeHistory>, Box<dyn std::error::Error>> {
        let url = format!("/api/v1/probes/{}/history", probe_name);
        let response = gloo::net::http::Request::get(&url)
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let history: Vec<grey_api::ProbeHistory> = response
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(history)
    }
}
