use std::collections::HashMap;

use chrono::Datelike;
#[cfg(feature = "wasm")]
use grey_api::UiConfig;
use yew::prelude::*;

use crate::components::status::StatusLevel;
use crate::contexts::{
    UiConfigProvider, NoticesProvider, ProbesProvider, ProbeHistoryProvider,
    use_probes, use_probe_history
};
use crate::app_state::AppState;

use super::components::{Banner, BannerKind, Header, Timeline, ProbeList};

#[cfg(feature = "wasm")]
pub enum ClientMsg {
    UpdateNotices(Vec<grey_api::UiNotice>),
    UpdateProbes(Vec<grey_api::Probe>),
    UpdateProbeHistory(String, Vec<grey_api::ProbeHistory>),
    Error(String),
}

// Main App component that provides all contexts
pub struct App {
    #[cfg(feature = "wasm")]
    config: UiConfig,
    #[cfg(feature = "wasm")]
    notices: Vec<grey_api::UiNotice>,
    #[cfg(feature = "wasm")]
    probes: Vec<grey_api::Probe>,
    #[cfg(feature = "wasm")]
    probe_histories: std::collections::HashMap<String, Vec<grey_api::ProbeHistory>>,
    #[cfg(feature = "wasm")]
    has_error: bool,
}

// SSR-compatible version that just takes props
#[derive(Properties, PartialEq)]
pub struct AppProps {
    pub config: grey_api::UiConfig,
    pub notices: Vec<grey_api::UiNotice>,
    pub probes: Vec<grey_api::Probe>,
    pub histories: HashMap<String, Vec<grey_api::ProbeHistory>>,
}

impl Component for App {
    #[cfg(feature = "wasm")]
    type Message = ClientMsg;
    #[cfg(feature = "wasm")]
    type Properties = ();

    #[cfg(not(feature = "wasm"))]
    type Message = ();
    #[cfg(not(feature = "wasm"))]
    type Properties = AppProps;

    #[cfg(feature = "wasm")]
    fn create(ctx: &Context<Self>) -> Self {
        // Try to load initial state from DOM data attributes
        let initial_state = AppState::from_dom().unwrap_or_default();

        let app = Self {
            config: initial_state.config,
            notices: initial_state.notices,
            probes: initial_state.probes,
            probe_histories: initial_state.probe_histories,
            has_error: false,
        };

        Self::setup_polling(ctx);

        app
    }
    
    #[cfg(not(feature = "wasm"))]
    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
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
                Self::setup_probes_polling(ctx);
                true
            }
            ClientMsg::UpdateProbeHistory(probe_name, history) => {
                self.probe_histories.insert(probe_name, history);
                self.has_error = false;
                true
            }
            ClientMsg::UpdateNotices(notices) => {
                self.notices = notices;
                Self::setup_notices_polling(ctx);
                true
            }
            ClientMsg::Error(err) => {
                gloo::console::error!("{}", err);
                self.has_error = true;
                false
            }
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        #[cfg(feature = "wasm")]
        let (config, notices, probes, probe_histories) = (
            self.config.clone(),
            self.notices.clone(),
            self.probes.clone(),
            self.probe_histories.clone(),
        );

        #[cfg(not(feature = "wasm"))]
        let AppProps {
            config,
            notices,
            probes,
            histories: probe_histories,
        } = _ctx.props();

        #[cfg(not(feature = "wasm"))]
        let (config, notices, probes, probe_histories) = (
            config.clone(),
            notices.clone(),
            probes.clone(),
            probe_histories.clone(),
        );

        // Create app state for data attributes
        let app_state = AppState {
            config: config.clone(),
            notices: notices.clone(),
            probes: probes.clone(),
            probe_histories: probe_histories.clone(),
        };

        // Serialize to JSON for data attributes
        let config_json = serde_json::to_string(&app_state.config).unwrap_or_default();
        let notices_json = serde_json::to_string(&app_state.notices).unwrap_or_default();
        let probes_json = serde_json::to_string(&app_state.probes).unwrap_or_default();
        let histories_json = serde_json::to_string(&app_state.probe_histories).unwrap_or_default();

        html! {
            <div id="app" 
                data-config={config_json}
                data-notices={notices_json}
                data-probes={probes_json}
                data-probe-histories={histories_json}
            >
                <UiConfigProvider config={config.clone()}>
                    <NoticesProvider notices={notices.clone()}>
                        <ProbesProvider probes={probes.clone()}>
                            <ProbeHistoryProvider probe_histories={probe_histories.clone()}>
                                <AppContent />
                            </ProbeHistoryProvider>
                        </ProbesProvider>
                    </NoticesProvider>
                </UiConfigProvider>
            </div>
        }
    }
}

// Main content component that uses contexts
#[function_component(AppContent)]
fn app_content() -> Html {
    let probes_ctx = use_probes();
    let history_ctx = use_probe_history();

    #[cfg(feature = "wasm")]
    let has_error = false; // We can add error state to context later if needed
    #[cfg(not(feature = "wasm"))]
    let has_error = false;

    let healthy_probes = history_ctx.probe_histories.iter()
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
                status={if has_error { StatusLevel::Error } else { StatusLevel::Good }} 
                status_text={if has_error { "Error" } else { "OK" }} 
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

impl App {
    #[cfg(feature = "wasm")]
    fn setup_polling(ctx: &Context<Self>) {
        Self::setup_probes_polling(ctx);
        Self::setup_notices_polling(ctx);
    }

    #[cfg(feature = "wasm")]
    fn setup_probes_polling(ctx: &Context<Self>) {
        ctx.link().send_future(async move {
            use std::time::Duration;
            gloo::timers::future::sleep(Duration::from_secs(120)).await;
            match Self::fetch_probes().await {
                Ok(probes) => ClientMsg::UpdateProbes(probes),
                Err(err) => ClientMsg::Error(format!("Failed to fetch probes: {}", err)),
            }
        });
    }

    #[cfg(feature = "wasm")]
    fn setup_notices_polling(ctx: &Context<Self>) {
        ctx.link().send_future(async move {
            use std::time::Duration;
            gloo::timers::future::sleep(Duration::from_secs(300)).await;
            match Self::fetch_notices().await {
                Ok(notices) => ClientMsg::UpdateNotices(notices),
                Err(err) => ClientMsg::Error(format!("Failed to fetch notices: {}", err)),
            }
        });
    }

    #[cfg(feature = "wasm")]
    async fn fetch_ui_config() -> Result<grey_api::UiConfig, Box<dyn std::error::Error>> {
        let response = gloo::net::http::Request::get("/api/v1/user-interface")
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let config: grey_api::UiConfig = response
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(config)
    }

    #[cfg(feature = "wasm")]
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

    #[cfg(feature = "wasm")]
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

    #[cfg(feature = "wasm")]
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
