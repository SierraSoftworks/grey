use actix_web::{web, HttpResponse, Result};
use grey_ui::{App, AppProps};
use yew::ServerRenderer;

use super::{AppState, ASSETS_DIR};

pub async fn index(data: web::Data<AppState>) -> Result<HttpResponse> {
    let probe_histories = data.state.get_probe_states().await?;

    let config = data.state.get_config();
    let probes = probe_histories.into_values().collect();

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
    let title = config.ui.title.clone();
    let app_props = AppProps {
        config: (&config.ui).into(),
        notices: config.ui.notices.clone(),
        probes,
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
