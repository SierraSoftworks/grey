use actix_web::{web, HttpResponse, Result};
use yew::ServerRenderer;
use grey_ui::{ServerApp, ServerAppProps, AppData, ProbeData, SampleData};

use super::{AppState, ASSETS_DIR};

pub async fn index(data: web::Data<AppState>) -> Result<HttpResponse> {
    let current_time = chrono::Utc::now();

    let probes_data = data
        .probes
        .values()
        .map(|probe| {
            let samples = if let Ok(history) = probe.history.read() {
                history.iter().map(|sample| SampleData {
                    pass: sample.pass,
                    message: sample.message.clone(),
                }).collect()
            } else {
                Vec::new()
            };

            ProbeData {
                name: probe.name.clone(),
                availability: probe.availability(),
                target: format!("{}", probe.target),
                policy: format!("{}", probe.policy),
                samples,
            }
        })
        .collect::<Vec<_>>();

    let availability = if probes_data.is_empty() {
        100.0
    } else {
        probes_data.iter().map(|probe| probe.availability).sum::<f64>() / (probes_data.len() as f64)
    };

    let app_data = AppData {
        config: data.config.clone(),
        availability,
        probes: probes_data,
        last_update: current_time,
    };

    // Read the embedded HTML template
    let html_template = ASSETS_DIR
        .get_file("index.html")
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("HTML template not found in embedded assets"))?
        .contents_utf8()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("HTML template is not valid UTF-8"))?;

    // Render the ServerApp component for SSR
    let app_props = ServerAppProps { data: app_data };
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
