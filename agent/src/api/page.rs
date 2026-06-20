use actix_web::{HttpRequest, HttpResponse, Result, web};
use grey_ui::{App, AppProps};
use yew::ServerRenderer;

use super::{ASSETS_DIR, AppState};
use crate::state::{CronStore, IncidentStore, ProbeStore};

pub async fn index(req: HttpRequest, data: web::Data<AppState>) -> Result<HttpResponse> {
    // Resolve the viewer's auth context so the server-rendered snapshot honours each entity's
    // `visible` filter. A normal browser navigation carries no `Authorization` header, so this is
    // anonymous in practice — admin-only probes/crons are therefore never embedded in the delivered
    // HTML and instead appear after sign-in, when the SPA's authenticated polls fetch them. A request
    // that somehow carries an invalid token can't renew it mid-render, so it falls back to the
    // anonymous view rather than failing the whole page (the public read endpoints return the 401).
    let ctx = super::auth::resolve_auth_context(&req, &data)
        .await
        .unwrap_or_default();
    let config = data.state.get_config();

    let probe_histories = data.state.get_probe_states().await?;
    let mut probes: Vec<grey_api::Probe> = probe_histories.into_values().collect();
    super::auth::retain_visible_probes(&config, &ctx, &mut probes);
    probes.sort_by_key(|p| p.name.clone());

    let mut crons: Vec<grey_api::Cron> = data
        .state
        .get_cron_states()
        .await
        .unwrap_or_default()
        .into_values()
        .collect();
    super::auth::retain_visible_crons(&config, &ctx, &mut crons);
    crons.sort_by_key(|c| c.name.clone());

    // Only the first page of publicly visible incidents is server-rendered for unauthenticated
    // viewers; the client paginates for older ones.
    let incidents = data
        .state
        .list_incidents(false, crate::state::DEFAULT_INCIDENT_PAGE, None)
        .await
        .map(|page| page.incidents)
        .unwrap_or_default();

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
        probes,
        crons,
        // Cluster topology is operator-only: it is never part of the server-rendered payload and is
        // fetched client-side once an administrator has signed in, so it can't leak to anonymous
        // viewers via the page's hydration data.
        incidents,
        url: req.uri().path().to_string(),
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
        let req = actix_web::test::TestRequest::default().to_http_request();
        let resp = index(req, web::Data::new(app_state)).await;

        let resp = resp.expect("Failed to render index");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("text/html"));
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        println!("{body}");
        assert!(body.trim().to_ascii_lowercase().starts_with("<!doctype html>"), "Body did not start with the HTML doctype");
        assert!(body.contains("<title>Grey</title>"), "Failed to find title in HTML body");
        assert!(body.contains(r#"data-probes="[{&quot;"#), "Failed to find probes data in HTML body");
        assert!(body.contains(r#"data-config="{&quot;"#), "Failed to find config data in HTML body");
        assert!(body.trim().ends_with("</html>"), "Body did not end with the HTML closing tag");
    }

    /// The server-rendered page must never embed operator-only cluster topology: `data-peers` is not
    /// part of the hydration payload, so an unauthenticated viewer can't read it from the HTML.
    #[actix_web::test]
    async fn test_index_omits_peers() {
        let temp_dir = tempdir().unwrap();
        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let req = actix_web::test::TestRequest::default().to_http_request();
        let resp = index(req, web::Data::new(app_state))
            .await
            .expect("Failed to render index");

        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        assert!(
            !body.contains("data-peers"),
            "the server-rendered page must not embed operator-only peer data"
        );
    }

    /// A deep link to the `/incidents` route must server-render the incidents page (the router is
    /// seeded from the request path) including any visible incidents.
    #[actix_web::test]
    async fn test_index_renders_incidents_route() {
        let temp_dir = tempdir().unwrap();
        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;

        app_state
            .state
            .create_incident(
                "Database outage".into(),
                grey_api::Impact::Offline,
                "Investigating".into(),
            )
            .await
            .unwrap();

        let req = actix_web::test::TestRequest::default()
            .uri("/incidents")
            .to_http_request();
        let resp = index(req, web::Data::new(app_state))
            .await
            .expect("Failed to render /incidents");

        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        assert!(
            body.contains("Database outage"),
            "the /incidents route should server-render the seeded incident"
        );
    }
}