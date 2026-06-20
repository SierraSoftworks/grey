use actix_web::{HttpRequest, HttpResponse, Result, web};

use super::AppState;
use super::auth::{resolve_auth_context, retain_visible_probes};
use crate::state::ProbeStore;

/// `GET /api/v1/probes` — the probes the requesting viewer may see, sorted by name. Public: an
/// anonymous viewer sees every probe whose `visible` filter permits it (the default permits
/// everyone), while a probe restricted with e.g. `visible: auth.admin` is returned only once a
/// matching bearer token is presented.
pub async fn get_probes(req: HttpRequest, data: web::Data<AppState>) -> Result<HttpResponse> {
    let ctx = match resolve_auth_context(&req, &data).await {
        Ok(ctx) => ctx,
        // A stale/invalid token is a 401 (not a silent public-only listing), so the SPA renews the
        // session and retries rather than quietly dropping the viewer's restricted probes.
        Err(err) => return Ok(err.into()),
    };
    let config = data.state.get_config();

    let api_probes = data.state.get_probe_states().await?;
    let mut probes: Vec<grey_api::Probe> = api_probes.into_values().collect();
    retain_visible_probes(&config, &ctx, &mut probes);
    probes.sort_by_key(|p| p.name.clone());

    Ok(HttpResponse::Ok().json(probes))
}

#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;
    use tempfile::tempdir;

    use super::*;
    use crate::state::State;

    #[actix_web::test]
    async fn test_get_probes() {
        let temp_dir = tempdir().unwrap();

        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let resp = get_probes(TestRequest::default().to_http_request(), web::Data::new(app_state)).await;

        let resp = resp.expect("Failed to get probes");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("application/json"));
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        let probes: Vec<grey_api::Probe> = serde_json::from_str(&body).unwrap();
        assert_eq!(probes.len(), 1);
    }

    /// A probe restricted with `visible: auth.admin` is omitted from the public listing for an
    /// anonymous viewer (no bearer token), while an unrestricted probe is returned.
    #[actix_web::test]
    async fn restricted_probes_are_hidden_from_anonymous_viewers() {
        let dir = tempdir().unwrap();
        let config = format!(
            "ui:\n  enabled: true\n  listen: 127.0.0.1:0\nprobes:\n  - name: public.probe\n    policy: {{ interval: 60s, timeout: 5s }}\n    target: !Http\n      url: https://example.com\n  - name: secret.probe\n    policy: {{ interval: 60s, timeout: 5s }}\n    target: !Http\n      url: https://example.com\n    visible: auth.admin\nstate: {}\n",
            dir.path().join("state.redb").display().to_string().replace('\\', "/")
        );
        let config_path = dir.path().join("config.yml");
        tokio::fs::write(&config_path, config).await.unwrap();
        let app_state = AppState::new(State::new(&config_path).await.unwrap());

        let resp = get_probes(TestRequest::default().to_http_request(), web::Data::new(app_state))
            .await
            .expect("Failed to get probes");
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let probes: Vec<grey_api::Probe> = serde_json::from_slice(&body_bytes).unwrap();

        let names: Vec<&str> = probes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["public.probe"], "the admin-only probe must be hidden from anonymous viewers");
    }

    /// A request that presents a bearer token the agent can't validate (an expired or otherwise
    /// invalid session) is rejected with a 401 rather than being silently downgraded to the anonymous
    /// listing. The SPA reacts to the 401 by renewing the session and retrying, so a signed-in
    /// viewer's probes don't quietly collapse to the public subset when their token lapses (e.g. while
    /// the tab was backgrounded). The OIDC endpoint is unreachable, so validation fails — exactly as
    /// it would for a genuinely expired token.
    #[actix_web::test]
    async fn invalid_bearer_token_is_rejected_not_downgraded() {
        let dir = tempdir().unwrap();
        let config = format!(
            "ui:\n  enabled: true\n  listen: 127.0.0.1:0\n  admin:\n    oidc:\n      endpoint: http://127.0.0.1:1\n      client_id: grey\n      client_secret: secret\nprobes:\n  - name: public.probe\n    policy: {{ interval: 60s, timeout: 5s }}\n    target: !Http\n      url: https://example.com\nstate: {}\n",
            dir.path().join("state.redb").display().to_string().replace('\\', "/")
        );
        let config_path = dir.path().join("config.yml");
        tokio::fs::write(&config_path, config).await.unwrap();
        let app_state = AppState::new(State::new(&config_path).await.unwrap());

        let req = TestRequest::default()
            .insert_header(("Authorization", "Bearer not-a-real-token"))
            .to_http_request();
        let resp = get_probes(req, web::Data::new(app_state))
            .await
            .expect("the handler renders the error as a response rather than returning Err");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
