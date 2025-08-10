use actix_web::{web, HttpResponse, Result};
use yew::ServerRenderer;
use grey_ui::{App, AppProps};

use super::{AppState, ASSETS_DIR};

pub async fn index(data: web::Data<AppState>) -> Result<HttpResponse> {
    let config: grey_api::UiConfig = (&data.ui_config).into();
    let notices = data.ui_config.notices.clone();
    let probes = data
        .probes
        .iter()
        .map(|(_name, probe)| probe.as_ref().into())
        .collect::<Vec<grey_api::Probe>>();
    let histories = data
        .probes
        .iter()
        .map(|(name, probe)| {
            let history = if let Ok(history) = probe.history.read() {
                history.iter().map(|sample| sample.into()).collect()
            } else {
                Vec::new()
            };
            (name.clone(), history)
        })
        .collect::<std::collections::HashMap<_, Vec<grey_api::ProbeResult>>>();


    // Read the embedded HTML template
    let html_template = ASSETS_DIR
        .get_file("index.html")
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("HTML template not found in embedded assets"))?
        .contents_utf8()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("HTML template is not valid UTF-8"))?;

    // Render the ServerApp component for SSR
    let app_props = AppProps { 
        config,
        notices,
        probes,
        histories,
    };
    let renderer = ServerRenderer::<App>::with_props(move || app_props).hydratable(true);
    let ssr_content = renderer.render().await;

    let (index_html_before, index_html_after) = html_template.split_once("<body>").unwrap();
    let mut index_html_before = index_html_before.to_owned();
    index_html_before.push_str("<body>");
    
    Ok(HttpResponse::Ok().content_type("text/html").body(format!(
        "{index_html_before}{ssr_content}{index_html_after}"
    )))
}
