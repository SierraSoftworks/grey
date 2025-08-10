use yew::prelude::*;
use serde::{Deserialize, Serialize};
use super::components::{Header, Banner, Notice, Probe as ProbeComponent, BannerKind, ProbeData, UiConfig};

#[cfg(feature = "wasm")]
use gloo_timers::callback::Interval;
#[cfg(feature = "wasm")]
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AppData {
    pub config: UiConfig,
    pub availability: f64,
    pub probes: Vec<ProbeData>,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_update: chrono::DateTime<chrono::Utc>,
}

#[cfg(feature = "wasm")]
pub enum ClientMsg {
    UpdateData(AppData),
    Error(String),
    Tick,
}

#[cfg(feature = "wasm")]
pub struct ClientApp {
    data: Option<AppData>,
    _interval: Option<Interval>,
}

#[cfg(not(feature = "wasm"))]
pub struct ClientApp;

#[cfg(feature = "wasm")]
impl Component for ClientApp {
    type Message = ClientMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        // Set up interval to fetch data every 30 seconds
        let link = ctx.link().clone();
        let interval = Interval::new(1000, move || {
            link.send_message(ClientMsg::Tick);
        });

        // Initial data fetch
        ctx.link().send_message(ClientMsg::Tick);

        Self {
            data: None,
            _interval: Some(interval),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ClientMsg::UpdateData(new_data) => {
                gloo::console::log!("Updated data");
                self.data = Some(new_data);
                true
            }
            ClientMsg::Tick => {
                gloo::console::log!("Refreshing data");
                let link = ctx.link().clone();
                spawn_local(async move {
                    match fetch_app_data().await {
                        Ok(response) => link.send_message(ClientMsg::UpdateData(response)),
                        Err(err) => link.send_message(ClientMsg::Error(format!("Failed to fetch app data: {}", err))),
                    }
                });
                false
            },
            ClientMsg::Error(err) => {
                gloo::console::error!("{}", err);
                false
            }
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        if let Some(data) = &self.data {
            view_app_with_data(data)
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
    pub data: AppData,
}

#[function_component(ServerApp)]
pub fn server_app(props: &ServerAppProps) -> Html {
    view_app_with_data(&props.data)
}

// Shared view logic for both SSR and CSR
fn view_app_with_data(data: &AppData) -> Html {
    let banner_kind = if data.availability == 100.0 {
        BannerKind::Ok
    } else if data.availability >= 90.0 {
        BannerKind::Warning
    } else {
        BannerKind::Error
    };

    let status_text = if data.availability == 100.0 {
        "All services operating normally"
    } else if data.availability >= 90.0 {
        "Partial degradation in service"
    } else {
        "Major outage affecting multiple services"
    };

    html! {
        <div id="app">
            <Header config={data.config.clone()} last_update={data.last_update} />
            
            <div class="content">
                <Banner kind={banner_kind} text={status_text.to_string()} />

                {for data.config.notices.iter().map(|notice| {
                    html! {
                        <Notice notice={notice.clone()} />
                    }
                })}

                {for data.probes.iter().map(|probe_data| {
                    html! {
                        <ProbeComponent probe={probe_data.clone()} />
                    }
                })}
            </div>

            <footer>
                <p>{"Copyright © Sierra Softworks"}</p>
            </footer>
        </div>
    }
}

#[cfg(feature = "wasm")]
async fn fetch_app_data() -> Result<AppData, Box<dyn std::error::Error>> {
    let response = gloo::net::http::Request::get("/api/v1/app-data")
        .send()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    
    let app_data: AppData = response.json().await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    Ok(app_data)
}
