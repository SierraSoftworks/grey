use actix_web::{web, HttpResponse, Result};
use yew::ServerRenderer;
use grey_ui::{ServerApp, ServerAppProps};

use super::{AppState, ASSETS_DIR};

pub async fn index(data: web::Data<AppState>) -> Result<HttpResponse> {
    let config: grey_api::UiConfig = (&data.ui_config).into();
    
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
    let app_props = ServerAppProps { 
        config,
        probes,
        histories,
    };
    let renderer = ServerRenderer::<ServerApp>::with_props(move || app_props);
    let ssr_content = renderer.render().await;

    // Replace the content between hydration markers with the SSR content
    let hydration_start = "<!-- hydration:start -->";
    let hydration_end = "<!-- hydration:end -->";
    
    let html_with_ssr = if let (Some(start_pos), Some(end_pos)) = (
        html_template.find(hydration_start),
        html_template.find(hydration_end)
    ) {
        let start_pos = start_pos + hydration_start.len();
        format!(
            "{}{}{}{}",
            &html_template[..start_pos],
            "\n",
            ssr_content,
            &html_template[end_pos..]
        )
    } else {
        // Fallback: replace the entire app div if markers aren't found
        html_template.replace(
            r#"<div id="app"></div>"#,
            &format!(r#"<div id="app">{}</div>"#, ssr_content)
        )
    };
    
    Ok(HttpResponse::Ok().content_type("text/html").body(html_with_ssr))
}
