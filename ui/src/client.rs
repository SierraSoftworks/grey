use std::collections::HashMap;

use chrono::Datelike;
use grey_api::UiConfig;
use yew::prelude::*;

use crate::status::StatusLevel;

use super::components::{Banner, BannerKind, Header, Notice, ProbeList};

#[cfg(feature = "wasm")]
pub enum ClientMsg {
    UpdateConfig(UiConfig),
    UpdateNotices(Vec<grey_api::UiNotice>),
    UpdateProbes(Vec<grey_api::Probe>),
    UpdateProbeHistory(String, Vec<grey_api::ProbeResult>),
    Error(String),
}

pub struct App {
    config: Option<UiConfig>,
    notices: Vec<grey_api::UiNotice>,
    probes: Vec<grey_api::Probe>,
    probe_histories: std::collections::HashMap<String, Vec<grey_api::ProbeResult>>,
    has_error: bool,
}

// SSR-compatible version that just takes props
#[derive(Properties, PartialEq)]
pub struct AppProps {
    pub config: grey_api::UiConfig,
    pub notices: Vec<grey_api::UiNotice>,
    pub probes: Vec<grey_api::Probe>,
    pub histories: HashMap<String, Vec<grey_api::ProbeResult>>,
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
        // Initial data fetch
        ctx.link().send_future(async move {
            match Self::fetch_probes().await {
                Ok(probes) => ClientMsg::UpdateProbes(probes),
                Err(err) => ClientMsg::Error(format!("Failed to fetch probes: {}", err)),
            }
        });

        ctx.link().send_future(async move {
            match Self::fetch_ui_config().await {
                Ok(config) => ClientMsg::UpdateConfig(config),
                Err(err) => ClientMsg::Error(format!("Failed to fetch UI config: {}", err)),
            }
        });

        ctx.link().send_future(async move {
            match Self::fetch_notices().await {
                Ok(notices) => ClientMsg::UpdateNotices(notices),
                Err(err) => ClientMsg::Error(format!("Failed to fetch notices: {}", err)),
            }
        });

        Self {
            config: None,
            notices: vec![],
            probes: vec![],
            probe_histories: std::collections::HashMap::new(),
            has_error: false,
        }
    }
    
    #[cfg(not(feature = "wasm"))]
    fn create(ctx: &Context<Self>) -> Self {
        let AppProps {
            config,
            notices,
            probes,
            histories,
        } = ctx.props();

        Self {
            config: Some(config.clone()),
            notices: notices.clone(),
            probes: probes.clone(),
            probe_histories: histories.clone(),
            has_error: false,
        }
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

                ctx.link().send_future(async move {
                    use std::time::Duration;

                    gloo::timers::future::sleep(Duration::from_secs(120)).await;

                    match Self::fetch_probes().await {
                        Ok(probes) => ClientMsg::UpdateProbes(probes),
                        Err(err) => ClientMsg::Error(format!("Failed to fetch probes: {}", err)),
                    }
                });

                true
            }
            ClientMsg::UpdateProbeHistory(probe_name, history) => {
                self.probe_histories.insert(probe_name, history);
                self.has_error = false;
                true
            }
            ClientMsg::UpdateConfig(config) => {
                self.config = Some(config);
                true
            }
            ClientMsg::UpdateNotices(notices) => {
                self.notices = notices;

                ctx.link().send_future(async move {
                    use std::time::Duration;

                    gloo::timers::future::sleep(Duration::from_secs(300)).await;

                    match Self::fetch_notices().await {
                        Ok(notices) => ClientMsg::UpdateNotices(notices),
                        Err(err) => ClientMsg::Error(format!("Failed to fetch notices: {}", err)),
                    }
                });
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
        let healthy_probes = self.probe_histories.iter()
            .filter_map(|(_, history)| history.last())
            .filter(|entry| entry.pass)
            .count();

        let banner_kind = match healthy_probes {
            p if p == self.probes.len() => BannerKind::Ok,
            p if p >= self.probes.len() / 2 => BannerKind::Warning,
            _ => BannerKind::Error,
        };

        let status_text = match banner_kind {
            BannerKind::Ok => "All services operating normally",
            BannerKind::Warning => "Partial degradation in service",
            BannerKind::Error => "Major outage affecting multiple services",
        };

        let config = self.config.clone().unwrap_or_default();

        html! {
            <div id="app">
                <Header config={config.clone()} status={if self.has_error { StatusLevel::Error } else { StatusLevel::Good }} status_text={if self.has_error { "Error" } else { "OK" }} />

                <div class="content">
                    <Banner kind={banner_kind} text={status_text.to_string()} />

                    {for self.notices.iter().map(|notice| {
                        html! {
                            <Notice notice={notice.clone()} />
                        }
                    })}

                    <ProbeList probes={self.probes.clone()} probe_histories={self.probe_histories.clone()} />
                </div>

                <footer>
                    <p>{format!("Copyright © {} Sierra Softworks", chrono::Utc::now().year())}</p>
                </footer>
            </div>
        }
    }
}

impl App {
    

    #[cfg(feature = "wasm")]
    async fn fetch_ui_config() -> Result<UiConfig, Box<dyn std::error::Error>> {
        let response = gloo::net::http::Request::get("/api/v1/user-interface")
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let config: UiConfig = response
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
    ) -> Result<Vec<grey_api::ProbeResult>, Box<dyn std::error::Error>> {
        let url = format!("/api/v1/probes/{}/history", probe_name);
        let response = gloo::net::http::Request::get(&url)
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let history: Vec<grey_api::ProbeResult> = response
            .json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(history)
    }
}
