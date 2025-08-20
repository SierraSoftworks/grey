use actix_web::{web, HttpResponse, Result};
use grey_api::Probe;
use grey_ui::{App, AppProps};
use yew::ServerRenderer;

use super::{AppState, ASSETS_DIR};

pub async fn index<const N: usize>(data: web::Data<AppState<N>>) -> Result<HttpResponse> {
    let config: grey_api::UiConfig = (&data.config.ui()).into();
    let notices = data.config.ui().notices;
    let probes = data
        .config
        .probes()
        .iter()
        .map(|probe| probe.into())
        .map(|mut probe: Probe| {
            probe.availability = data.history.get(&probe.name).map(|h| h.availability()).unwrap_or(100.0);
            probe
        })
        .collect::<Vec<grey_api::Probe>>();
    let histories = probes
        .iter()
        .filter_map(|probe| {
            data.history
                .get(&probe.name)
                .map(|history| (probe, history))
        })
        .map(|(probe, history)| {
            let history = history
                .get_state_buckets()
                .iter()
                .map(|bucket| bucket.into())
                .collect();
            (probe.name.clone(), history)
        })
        .collect::<std::collections::HashMap<_, Vec<grey_api::ProbeHistory>>>();

    // Read the embedded HTML template
    let html_template = ASSETS_DIR
        .get_file("index.html")
        .ok_or_else(|| {
            actix_web::error::ErrorInternalServerError("HTML template not found in embedded assets")
        })?
        .contents_utf8()
        .ok_or_else(|| {
            actix_web::error::ErrorInternalServerError("HTML template is not valid UTF-8")
        })?;

    // Render the ServerApp component for SSR
    let title = config.title.clone();
    let app_props = AppProps {
        config,
        notices,
        probes,
        histories,
    };
    let renderer = ServerRenderer::<App>::with_props(move || app_props).hydratable(true);
    let ssr_content = renderer.render().await;

    let (index_html_before, index_html_after) = html_template.split_once("<body>").unwrap();
    let mut index_html_before =
        index_html_before.replace("<title></title>", &format!("<title>{}</title>", title));
    index_html_before.push_str("<body>");

    Ok(HttpResponse::Ok().content_type("text/html").body(format!(
        "{index_html_before}{ssr_content}{index_html_after}"
    )))
}
