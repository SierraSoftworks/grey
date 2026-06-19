//! The public cron "deadman's switch" API: a check-in *ingest* endpoint at
//! `/api/v1/cron/{name}/check-in` (POST with a JSON body or GET with query parameters), kept separate
//! from the UI's `/api/v1/crons` read API. Crons are config-declared; a check-in for an unknown cron
//! is rejected, and a cron that declares a `token` requires it on every check-in.

use actix_web::{HttpRequest, HttpResponse, Result, web};
use grey_api::{ApiError, Cron, CronStatus};
use serde::Deserialize;

use super::AppState;
use crate::state::CronStore;

/// The HTTP header a caller may use to present a cron's shared secret (the alternative to the
/// `token` query parameter).
const CRON_TOKEN_HEADER: &str = "X-Cron-Token";

/// Check-in parameters, shared between the JSON body (POST) and the query string (GET). `status` is
/// kept as a string and validated by hand so an unknown value yields a clean 400 rather than a
/// generic deserialization error.
#[derive(Debug, Default, Deserialize)]
pub struct CronCheckinParams {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

/// A length-revealing but content-constant-time byte comparison, so a wrong token can't be recovered
/// by timing the comparison.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// `POST /api/v1/cron/{name}/check-in` — a check-in carried in a JSON body. The body is optional so a
/// caller may also pass everything in the query string; missing/invalid parameters are reported as a
/// 400.
pub async fn report_checkin_post(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<String>,
    body: Option<web::Json<CronCheckinParams>>,
) -> Result<HttpResponse> {
    let params = body.map(web::Json::into_inner).unwrap_or_default();
    record(req, data, path.into_inner(), params).await
}

/// `GET /api/v1/cron/{name}/check-in` — the same check-in carried in query parameters, for callers (restricted
/// cron environments, simple `curl` invocations) that can only issue a GET.
pub async fn report_checkin_get(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<String>,
    query: web::Query<CronCheckinParams>,
) -> Result<HttpResponse> {
    record(req, data, path.into_inner(), query.into_inner()).await
}

async fn record(
    req: HttpRequest,
    data: web::Data<AppState>,
    name: String,
    params: CronCheckinParams,
) -> Result<HttpResponse> {
    let Some(status) = params.status.as_deref().and_then(|s| s.parse::<CronStatus>().ok()) else {
        return Ok(ApiError::bad_request(
            "A 'status' of 'running', 'succeeded' or 'failed' is required.",
        )
        .into());
    };

    // Look the cron up in local config to both authorise the check-in and confirm it exists.
    let config = data.state.get_config();
    let Some(cfg) = config.crons.iter().find(|c| c.name == name) else {
        return Ok(ApiError::not_found("No cron with that name is configured.").into());
    };

    if let Some(expected) = cfg.token.as_deref() {
        let provided = req
            .headers()
            .get(CRON_TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .or(params.token.as_deref());

        if !provided
            .map(|p| constant_time_eq(p.as_bytes(), expected.as_bytes()))
            .unwrap_or(false)
        {
            return Ok(
                ApiError::unauthorized("A valid token is required to check in to this cron.").into(),
            );
        }
    }

    let checkin = crate::cron::CronCheckin::new(
        status,
        params.message.unwrap_or_default(),
        chrono::Utc::now(),
    );

    if data.state.record_cron_checkin(&name, checkin).await? {
        // A bare 202: the check-in is accepted with no body; the cron's state is read separately via
        // `GET /api/v1/crons`.
        Ok(HttpResponse::Accepted().finish())
    } else {
        // The cron was removed from config between the lookup above and the write.
        Ok(ApiError::not_found("No cron with that name is configured.").into())
    }
}

/// `GET /api/v1/crons` — every cron's current state, sorted by name. Public, mirroring `/probes`.
pub async fn get_crons(data: web::Data<AppState>) -> Result<HttpResponse> {
    let crons = data.state.get_cron_states().await?;
    let mut crons: Vec<Cron> = crons.into_values().collect();
    crons.sort_by_key(|c| c.name.clone());

    Ok(HttpResponse::Ok().json(crons))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use actix_web::http::StatusCode;
    use actix_web::{App, test};

    async fn app_state(dir: &std::path::Path) -> AppState {
        let config = format!(
            "ui:\n  enabled: true\n  listen: 127.0.0.1:0\ncrons:\n  - name: backup\n    interval: 60s\n  - name: secure\n    interval: 60s\n    token: s3cr3t\nstate: {}\nprobes: []\n",
            dir.join("state.redb").display().to_string().replace('\\', "/")
        );
        let config_path = dir.join("config.yml");
        tokio::fs::write(&config_path, config).await.unwrap();
        AppState::new(State::new(&config_path).await.unwrap())
    }

    fn configure(cfg: &mut web::ServiceConfig) {
        cfg.route("/api/v1/crons", web::get().to(get_crons))
            .route("/api/v1/cron/{name}/check-in", web::get().to(report_checkin_get))
            .route("/api/v1/cron/{name}/check-in", web::post().to(report_checkin_post));
    }

    #[actix_web::test]
    async fn post_checkin_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let state = app_state(dir.path()).await;
        let app = test::init_service(
            App::new().app_data(web::Data::new(state)).configure(configure),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/v1/cron/backup/check-in")
            .set_json(serde_json::json!({ "status": "succeeded", "message": "42GB" }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        // It now appears in the public list as passing.
        let req = test::TestRequest::get().uri("/api/v1/crons").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = test::read_body(resp).await;
        let crons: Vec<Cron> = serde_json::from_slice(&body).unwrap();
        let backup = crons.iter().find(|c| c.name == "backup").unwrap();
        assert_eq!(backup.runs.len(), 1);
    }

    #[actix_web::test]
    async fn get_checkin_via_query_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let state = app_state(dir.path()).await;
        let app = test::init_service(
            App::new().app_data(web::Data::new(state)).configure(configure),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/v1/cron/backup/check-in?status=running&message=go")
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::ACCEPTED);
    }

    #[actix_web::test]
    async fn missing_or_invalid_status_is_a_400() {
        let dir = tempfile::tempdir().unwrap();
        let state = app_state(dir.path()).await;
        let app = test::init_service(
            App::new().app_data(web::Data::new(state)).configure(configure),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/v1/cron/backup/check-in")
            .set_json(serde_json::json!({ "message": "no status" }))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::BAD_REQUEST);

        let req = test::TestRequest::get()
            .uri("/api/v1/cron/backup/check-in?status=bogus")
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn unknown_cron_is_a_404() {
        let dir = tempfile::tempdir().unwrap();
        let state = app_state(dir.path()).await;
        let app = test::init_service(
            App::new().app_data(web::Data::new(state)).configure(configure),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/v1/cron/ghost/check-in")
            .set_json(serde_json::json!({ "status": "succeeded" }))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn token_is_enforced_when_configured() {
        let dir = tempfile::tempdir().unwrap();
        let state = app_state(dir.path()).await;
        let app = test::init_service(
            App::new().app_data(web::Data::new(state)).configure(configure),
        )
        .await;

        // No token → 401.
        let req = test::TestRequest::post()
            .uri("/api/v1/cron/secure/check-in")
            .set_json(serde_json::json!({ "status": "succeeded" }))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::UNAUTHORIZED);

        // Wrong token → 401.
        let req = test::TestRequest::get()
            .uri("/api/v1/cron/secure/check-in?status=succeeded&token=nope")
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::UNAUTHORIZED);

        // Correct token via the header → 202.
        let req = test::TestRequest::post()
            .uri("/api/v1/cron/secure/check-in")
            .insert_header((CRON_TOKEN_HEADER, "s3cr3t"))
            .set_json(serde_json::json!({ "status": "succeeded" }))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::ACCEPTED);

        // Correct token via the query string → 202.
        let req = test::TestRequest::get()
            .uri("/api/v1/cron/secure/check-in?status=succeeded&token=s3cr3t")
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::ACCEPTED);
    }
}
