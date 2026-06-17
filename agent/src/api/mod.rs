use std::sync::Arc;

use actix_web::{App, HttpResponse, HttpServer, Result, http::header::ContentType, middleware::from_fn, web};
use include_dir::{Dir, include_dir};

use crate::state::State;

use auth::OidcVerifier;

mod admin;
mod api;
mod auth;
mod page;
mod trace;

// Embed the dist directory at compile time
static ASSETS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../ui/dist");

#[derive(Clone)]
pub struct AppState {
    pub state: State,
    /// Shared OIDC bearer-token validator (caches discovery + JWKS across requests).
    pub oidc: Arc<OidcVerifier>,
}

impl AppState {
    pub fn new(state: State) -> Self {
        Self {
            state,
            oidc: Arc::new(OidcVerifier::new()),
        }
    }

    #[cfg(test)]
    pub async fn test(temp_path: std::path::PathBuf) -> Self {
        Self {
            state: State::test(temp_path).await,
            oidc: Arc::new(OidcVerifier::new()),
        }
    }
}

// Custom handler for serving embedded static files
async fn serve_static(path: web::Path<String>) -> Result<HttpResponse> {
    let file_path = path.into_inner();

    if let Some(file) = ASSETS_DIR.get_file(&file_path) {
        let mut response = HttpResponse::Ok();

        // Set appropriate content type
        match file_path.split('.').next_back().unwrap_or("") {
            "js" => response.insert_header(("content-type", "application/javascript")),
            "wasm" => response.insert_header(("content-type", "application/wasm")),
            "css" => response.insert_header(("content-type", "text/css")),
            "html" => response.insert_header(ContentType::html()),
            _ => response.insert_header(ContentType::octet_stream()),
        };

        Ok(response.body(file.contents()))
    } else {
        Ok(HttpResponse::NotFound().body("File not found"))
    }
}

pub fn create_app() -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        // Surface a `traceparent` on every response so clients can correlate problems with traces.
        .wrap(from_fn(trace::trace_requests))
        .route("/", web::get().to(page::index))
        // Client-routed pages are server-rendered by the same handler; it reads the request path to
        // seed the router. New top-level SPA routes need a matching entry here.
        .route("/incidents", web::get().to(page::index))
        .route("/incidents/{id}", web::get().to(page::index))
        .route("/api/v1/probes", web::get().to(api::get_probes))
        .route("/api/v1/notices", web::get().to(api::get_notices))
        .route("/api/v1/incidents", web::get().to(api::get_incidents))
        .route("/api/v1/cluster/peers", web::get().to(api::get_peers))
        // Public login endpoints: the SPA fetches the provider's authorization endpoint, then hands
        // the resulting authorization code here for the agent to exchange with its client secret.
        .route("/api/v1/auth/metadata", web::get().to(auth::metadata))
        .route("/api/v1/auth/token", web::post().to(auth::exchange_token))
        // Admin API: every route is guarded by OIDC bearer validation + the configured ACL.
        .service(
            web::scope("/api/v1/admin")
                .wrap(from_fn(auth::require_admin))
                .route("/me", web::get().to(admin::me))
                .route("/incidents", web::get().to(admin::list_incidents))
                .route("/incidents", web::post().to(admin::create_incident))
                .route("/incidents/{id}", web::get().to(admin::get_incident))
                .route("/incidents/{id}", web::put().to(admin::replace_incident))
                .route("/incidents/{id}", web::delete().to(admin::delete_incident)),
        )
        .route("/static/{filename:.*}", web::get().to(serve_static))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;

    /// The incident routes must be wired into the application (the handler unit tests call the
    /// handlers directly and so don't exercise route registration).
    #[actix_web::test]
    async fn incident_routes_are_registered() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(create_app().app_data(web::Data::new(state))).await;

        // The public incidents API is reachable.
        let req = test::TestRequest::get().uri("/api/v1/incidents").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success(), "GET /api/v1/incidents should be routed");

        // The /incidents SPA route is server-rendered rather than 404.
        let req = test::TestRequest::get().uri("/incidents").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success(), "GET /incidents should be server-rendered");
    }

    /// The tracing middleware is wired in front of every route. It echoes a `traceparent` when a
    /// telemetry pipeline is initialised; here we only assert it doesn't interfere with responses
    /// (the test process has no OpenTelemetry runtime, so the global propagator is a no-op).
    #[actix_web::test]
    async fn tracing_middleware_does_not_break_responses() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(create_app().app_data(web::Data::new(state))).await;

        let req = test::TestRequest::get().uri("/api/v1/incidents").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success(), "the tracing middleware must pass requests through");
    }

    /// With no `admin` configuration, the admin scope is closed entirely (403, not a route 404).
    #[actix_web::test]
    async fn admin_routes_are_closed_when_unconfigured() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(create_app().app_data(web::Data::new(state))).await;

        let req = test::TestRequest::get().uri("/api/v1/admin/incidents").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    /// With `admin` configured but no bearer token, the admin API responds 401 (authenticate),
    /// distinct from the 403 returned when admin is disabled entirely.
    #[actix_web::test]
    async fn admin_routes_require_a_token_when_configured() {
        let dir = tempfile::tempdir().unwrap();
        let config = format!(
            "ui:\n  enabled: true\n  listen: 127.0.0.1:0\n  admin:\n    acl: 'true'\n    oidc:\n      endpoint: https://issuer.example\n      client_id: test-client\n      client_secret: test-secret\nstate: {}\nprobes: []\n",
            dir.path().join("state.redb").display().to_string().replace('\\', "/")
        );
        let config_path = dir.path().join("config.yml");
        tokio::fs::write(&config_path, config).await.unwrap();
        let state = State::new(&config_path).await.unwrap();
        let app = test::init_service(create_app().app_data(web::Data::new(AppState::new(state)))).await;

        let req = test::TestRequest::get().uri("/api/v1/admin/incidents").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    }

    /// The login endpoints surface the provider's authorization endpoint and exchange an
    /// authorization code for a token server-side (using the client secret), against a mocked IdP.
    #[actix_web::test]
    async fn auth_metadata_and_code_exchange() {
        use actix_web::body::MessageBody;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let issuer = server.uri();

        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": issuer,
                "jwks_uri": format!("{issuer}/jwks"),
                "authorization_endpoint": format!("{issuer}/authorize"),
                "token_endpoint": format!("{issuer}/token"),
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "id_token": "header.payload.sig" })),
            )
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config = format!(
            "ui:\n  enabled: true\n  listen: 127.0.0.1:0\n  admin:\n    acl: 'true'\n    oidc:\n      endpoint: {issuer}\n      client_id: test-client\n      client_secret: test-secret\nstate: {}\nprobes: []\n",
            dir.path().join("state.redb").display().to_string().replace('\\', "/")
        );
        let config_path = dir.path().join("config.yml");
        tokio::fs::write(&config_path, config).await.unwrap();
        let data = web::Data::new(AppState::new(State::new(&config_path).await.unwrap()));

        // Metadata exposes the provider's authorization endpoint.
        let resp = auth::metadata(data.clone()).await.unwrap();
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&resp.into_body().try_into_bytes().unwrap()).unwrap();
        assert_eq!(body["authorization_endpoint"], format!("{issuer}/authorize"));

        // The code is exchanged server-side and the token returned to the caller.
        let resp = auth::exchange_token(
            data,
            web::Json(auth::TokenExchangeRequest {
                code: "auth-code".into(),
                redirect_uri: "http://localhost/".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&resp.into_body().try_into_bytes().unwrap()).unwrap();
        assert_eq!(body["token"], "header.payload.sig");
    }
}

pub async fn start_server(state: State) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState::new(state);

    let listen_addr = state.state.get_config().ui.listen.clone();

    Ok(
        HttpServer::new(move || create_app().app_data(web::Data::new(state.clone())))
            .workers(1)
            .bind(&listen_addr)?
            .run()
            .await
            .map_err(|e| format!("{}", e))?,
    )
}
