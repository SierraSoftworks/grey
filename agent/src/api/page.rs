use actix_web::{HttpResponse, Result, web};
use grey_ui::{App, AppProps};
use yew::ServerRenderer;

use super::{ASSETS_DIR, AppState};

pub async fn index(data: web::Data<AppState>) -> Result<HttpResponse> {
    let probe_histories = data.state.get_probe_states().await?;

    let config = data.state.get_config();
    let mut probes: Vec<grey_api::Probe> = probe_histories.into_values().collect();
    probes.sort_by_key(|p| p.name.clone());

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

#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use tempfile::tempdir;
    use actix_web::http::StatusCode;

    use super::*;

    #[actix_web::test]
    async fn test_index() {
        let temp_dir = tempdir().unwrap();

        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let resp = index(web::Data::new(app_state)).await;

        let resp = resp.expect("Failed to render index");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("text/html"));
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        println!("{body}");
        assert!(body.trim().starts_with("<!DOCTYPE html>"), "Body did not start with the HTML doctype");
        assert!(body.contains("<title>Grey</title>"), "Failed to find title in HTML body");
        assert!(body.contains(r#"data-probes="[{&quot;"#), "Failed to find probes data in HTML body");
        assert!(body.contains(r#"data-config="{&quot;"#), "Failed to find config data in HTML body");
        assert!(body.trim().ends_with("</html>"), "Body did not end with the HTML closing tag");
    }
}