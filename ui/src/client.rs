use std::collections::HashMap;

use yew::prelude::*;
use crate::header::_HeaderProps::last_update;

use super::components::{Header, Banner, Notice, Probe as ProbeComponent, BannerKind};
use grey_api::UiConfig;

#[cfg(feature = "wasm")]
use gloo_timers::callback::Interval;
#[cfg(feature = "wasm")]
use wasm_bindgen_futures::spawn_local;

#[cfg(feature = "wasm")]
pub enum ClientMsg {
    UpdateConfig(UiConfig),
    UpdateProbes(Vec<grey_api::Probe>),
    UpdateProbeHistory(String, Vec<grey_api::ProbeResult>),
    Error(String),
}

#[cfg(feature = "wasm")]
pub struct ClientApp {
    config: Option<UiConfig>,
    probes: Vec<grey_api::Probe>,
    probe_histories: std::collections::HashMap<String, Vec<grey_api::ProbeResult>>,
    last_updated: chrono::DateTime<chrono::Utc>,
}

#[cfg(not(feature = "wasm"))]
pub struct ClientApp;

#[cfg(feature = "wasm")]
impl Component for ClientApp {
    type Message = ClientMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        // Initial data fetch
        ctx.link().send_future(async move {
            match fetch_probes().await {
                Ok(probes) => ClientMsg::UpdateProbes(probes),
                Err(err) => ClientMsg::Error(format!("Failed to fetch probes: {}", err)),
            }
        });

        ctx.link().send_future(async move {
            match fetch_ui_config().await {
                Ok(config) => ClientMsg::UpdateConfig(config),
                Err(err) => ClientMsg::Error(format!("Failed to fetch UI config: {}", err)),
            }
        });

        Self {
            config: None,
            probes: vec![],
            probe_histories: std::collections::HashMap::new(),
            last_updated: chrono::Utc::now(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ClientMsg::UpdateProbes(probes) => {
                gloo::console::log!("Updated probes");
                self.probes = probes;
                self.last_updated = chrono::Utc::now();
                for probe in self.probes.iter() {
                    let probe_name = probe.name.clone();
                    ctx.link().send_future(async move {
                        match fetch_probe_history(&probe_name).await {
                            Ok(history) => ClientMsg::UpdateProbeHistory(probe_name, history),
                            Err(err) => ClientMsg::Error(format!("Failed to fetch history for {}: {}", probe_name, err)),
                        }
                    });
                }

                ctx.link().send_future(async move {
                    use std::time::Duration;

                    gloo::timers::future::sleep(Duration::from_secs(1)).await;

                    match fetch_probes().await {
                        Ok(probes) => ClientMsg::UpdateProbes(probes),
                        Err(err) => ClientMsg::Error(format!("Failed to fetch probes: {}", err)),
                    }
                });

                true
            }
            ClientMsg::UpdateProbeHistory(probe_name, history) => {
                gloo::console::log!(format!("Updated history for {}", probe_name));
                self.probe_histories.insert(probe_name, history);
                true
            }
            ClientMsg::UpdateConfig(config) => {
                gloo::console::log!("Updated config");
                self.config = Some(config);
                true
            }
            ClientMsg::Error(err) => {
                gloo::console::error!("{}", err);
                false
            }
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        if let Some(config) = &self.config {
            view_app_with_data(&config, self.last_updated, &self.probes, &self.probe_histories)
        } else {
            html! {
                <div id="app">
                    <div class="loading">{"Loading..."}</div>
                </div>
            }
        }
    }
}

// SSR-compatible version that just takes props
#[derive(Properties, PartialEq)]
pub struct ServerAppProps {
    pub config: grey_api::UiConfig,
    pub probes: Vec<grey_api::Probe>,
    pub histories: HashMap<String, Vec<grey_api::ProbeResult>>,
}

#[function_component(ServerApp)]
pub fn server_app(props: &ServerAppProps) -> Html {
    view_app_with_data(&props.config, chrono::Utc::now(), &props.probes, &props.histories)
}

// Shared view logic for both SSR and CSR
fn view_app_with_data(config: &grey_api::UiConfig, last_updated: chrono::DateTime<chrono::Utc>, probes: &Vec<grey_api::Probe>, histories: &HashMap<String, Vec<grey_api::ProbeResult>>) -> Html {
    let availability = 100.0 * probes.iter().map(|p| p.availability).sum::<f64>() / probes.len() as f64;

    let banner_kind = if availability == 100.0 {
        BannerKind::Ok
    } else if availability >= 90.0 {
        BannerKind::Warning
    } else {
        BannerKind::Error
    };

    let status_text = if availability == 100.0 {
        "All services operating normally"
    } else if availability >= 90.0 {
        "Partial degradation in service"
    } else {
        "Major outage affecting multiple services"
    };

    html! {
        <div id="app">
            <Header config={config.clone()} last_update={last_updated} />
            
            <div class="content">
                <Banner kind={banner_kind} text={status_text.to_string()} />

                {for config.notices.iter().map(|notice| {
                    html! {
                        <Notice notice={notice.clone()} />
                    }
                })}

                {for probes.iter().map(|probe| {
                    html! {
                        <ProbeComponent probe={probe.clone()} history={histories.get(&probe.name).cloned().unwrap_or_default()} />
                    }
                })}
            </div>

            <footer>
                <p>{"Copyright © 2025 Sierra Softworks"}</p>
            </footer>
        </div>
    }
}

#[cfg(feature = "wasm")]
async fn fetch_ui_config() -> Result<UiConfig, Box<dyn std::error::Error>> {
    let response = gloo::net::http::Request::get("/api/v1/user-interface")
        .send()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    let config: UiConfig = response.json().await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    Ok(config)
}

#[cfg(feature = "wasm")]
async fn fetch_probes() -> Result<Vec<grey_api::Probe>, Box<dyn std::error::Error>> {
    let response = gloo::net::http::Request::get("/api/v1/probes")
        .send()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    
    let probes: Vec<grey_api::Probe> = response.json().await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    Ok(probes)
}

#[cfg(feature = "wasm")]
async fn fetch_probe_history(probe_name: &str) -> Result<Vec<grey_api::ProbeResult>, Box<dyn std::error::Error>> {
    let url = format!("/api/v1/probes/{}/history", probe_name);
    let response = gloo::net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    
    let history: Vec<grey_api::ProbeResult> = response.json().await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    Ok(history)
}
