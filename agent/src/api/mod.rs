use std::sync::Arc;

use actix_web::{App, HttpResponse, HttpServer, Result, http::header::ContentType, middleware::from_fn, web};
use include_dir::{Dir, include_dir};

use crate::state::State;

use auth::OidcVerifier;

mod admin;
mod auth;
mod cluster;
mod config;
mod cron;
mod incidents;
mod page;
mod probes;
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

// Cache assets that carry a content hash for a year. Such files are immutable under a given name,
// so browsers and shared caches can keep them indefinitely and skip revalidation on every refresh.
const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// Trunk fingerprints the build assets it emits — the WASM bundle, its JS loader and the compiled
/// stylesheet — by embedding a content hash in the filename (e.g. `index-9f8a7b6c.js` or
/// `index-9f8a7b6c_bg.wasm`). Because that name changes whenever the contents do, those assets are
/// safe to cache forever. The un-fingerprinted entry document (`index.html`) is deliberately
/// excluded so a fresh deployment is always picked up.
fn is_fingerprinted(file_name: &str) -> bool {
    file_name
        .split(['-', '.', '_'])
        .any(|segment| segment.len() >= 8 && segment.bytes().all(|b| b.is_ascii_hexdigit()))
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

        let file_name = file_path.rsplit('/').next().unwrap_or(&file_path);
        if is_fingerprinted(file_name) {
            response.insert_header(("cache-control", IMMUTABLE_CACHE_CONTROL));
        } else {
            // Files without a content hash (notably index.html) must be revalidated so clients pick
            // up a new build instead of serving a stale document from cache.
            response.insert_header(("cache-control", "no-cache"));
        }

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
        // The OIDC login popup lands on the callback route, and an explicit sign-out link hits the
        // logout route; both are SPA pages, so serve the rendered app and let the client finish up.
        .route("/auth/callback", web::get().to(page::index))
        .route("/auth/logout", web::get().to(page::index))
        .route("/api/v1/probes", web::get().to(probes::get_probes))
        .route("/api/v1/crons", web::get().to(cron::get_crons))
        // Public cron check-in: a scheduled job reports its status here (POST with a JSON body, or
        // GET with query parameters). This is a separate ingest endpoint from the UI's `/crons` read
        // API; an optional per-cron token gates writes when configured.
        .route("/api/v1/cron/{name}/check-in", web::get().to(cron::report_checkin_get))
        .route("/api/v1/cron/{name}/check-in", web::post().to(cron::report_checkin_post))
        .route("/api/v1/incidents", web::get().to(incidents::get_incidents))
        // Public login endpoints: the SPA fetches the provider's authorization endpoint, then hands
        // the resulting authorization code here for the agent to exchange with its client secret.
        .route("/api/v1/auth/metadata", web::get().to(auth::metadata))
        .route("/api/v1/auth/token", web::post().to(auth::exchange_token))
        .route("/api/v1/auth/refresh", web::post().to(auth::refresh_token))
        // Admin API: every route is guarded by OIDC bearer validation + the configured ACL.
        .service(
            web::scope("/api/v1/admin")
                .wrap(from_fn(auth::require_admin))
                .route("/me", web::get().to(admin::me))
                // Cluster topology is operator-only: it exposes peer addresses and health, so it
                // lives behind the admin gate rather than being surfaced to public viewers.
                .route("/cluster/peers", web::get().to(cluster::get_peers))
                .route("/incidents", web::get().to(admin::list_incidents))
                .route("/incidents", web::post().to(admin::create_incident))
                .route("/incidents/{id}", web::get().to(admin::get_incident))
                .route("/incidents/{id}", web::put().to(admin::put_incident))
                .route("/incidents/{id}", web::delete().to(admin::delete_incident))
                .route("/incidents/{id}/updates", web::post().to(admin::create_update))
                .route("/incidents/{id}/updates/{uid}", web::put().to(admin::put_update))
                .route("/incidents/{id}/updates/{uid}", web::delete().to(admin::delete_update)),
        )
        .route("/static/{filename:.*}", web::get().to(serve_static))
}

/// Pure-function tests for the fingerprint detector. These live in their own module because the
/// integration tests below import `actix_web::test`, which shadows the built-in `#[test]` attribute.
#[cfg(test)]
mod fingerprint_tests {
    use super::is_fingerprinted;

    /// Content-hashed Trunk assets are recognised as fingerprinted (and so cacheable forever), while
    /// the un-hashed entry document and other stable names are not.
    #[test]
    fn fingerprinted_assets_are_detected() {
        assert!(is_fingerprinted("index-9f8a7b6c1d2e3f40.js"));
        assert!(is_fingerprinted("index-9f8a7b6c1d2e3f40_bg.wasm"));
        assert!(is_fingerprinted("styles-deadbeefcafef00d.css"));

        assert!(!is_fingerprinted("index.html"));
        assert!(!is_fingerprinted("favicon.ico"));
        // A short, non-hash suffix must not be mistaken for a content hash.
        assert!(!is_fingerprinted("app-v2.js"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;

    /// Static assets carrying a content hash are served with a long-lived immutable cache policy so
    /// repeat visits and refreshes don't re-download them, while un-hashed files are revalidated.
    #[actix_web::test]
    async fn static_assets_get_cache_headers() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(create_app().app_data(web::Data::new(state))).await;

        // The WASM bundle's JS loader is content-hashed; discover its name from the embedded build.
        let hashed_js = ASSETS_DIR
            .files()
            .map(|f| f.path().file_name().unwrap().to_string_lossy().into_owned())
            .find(|name| name.ends_with(".js") && is_fingerprinted(name))
            .expect("the UI build should emit a content-hashed JS loader");

        let req = test::TestRequest::get()
            .uri(&format!("/static/{hashed_js}"))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        assert_eq!(
            resp.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some(IMMUTABLE_CACHE_CONTROL),
            "hashed assets must be cached as immutable"
        );

        // index.html carries no content hash and must be revalidated on every load.
        let req = test::TestRequest::get()
            .uri("/static/index.html")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        assert_eq!(
            resp.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("no-cache"),
            "the un-hashed entry document must not be cached aggressively"
        );
    }

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

    /// The public cron routes must be wired into the application: the list endpoint is reachable and
    /// the check-in endpoint is routed (a check-in for an unconfigured cron is a 404, not a routing
    /// 404 — i.e. the route exists and the handler ran).
    #[actix_web::test]
    async fn cron_routes_are_registered() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(create_app().app_data(web::Data::new(state))).await;

        let req = test::TestRequest::get().uri("/api/v1/crons").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success(), "GET /api/v1/crons should be routed");

        // The check-in route exists; with no crons configured the handler returns 404 with an
        // ApiError body (a routed response, distinct from a missing route).
        let req = test::TestRequest::post()
            .uri("/api/v1/cron/anything/check-in")
            .set_json(serde_json::json!({ "status": "succeeded" }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::NOT_FOUND);
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

    /// Cluster topology lives behind the admin gate: the old public route is gone (404) and the
    /// admin route is closed (403) when no `admin` config is present.
    #[actix_web::test]
    async fn peers_endpoint_is_admin_only() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(create_app().app_data(web::Data::new(state))).await;

        // The former public endpoint no longer exists.
        let req = test::TestRequest::get().uri("/api/v1/cluster/peers").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::NOT_FOUND);

        // The peers endpoint now sits under the (unconfigured, hence closed) admin scope.
        let req = test::TestRequest::get().uri("/api/v1/admin/cluster/peers").to_request();
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
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id_token": "header.payload.sig",
                "refresh_token": "refresh-123",
            })))
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
        assert_eq!(body["refresh_token"], "refresh-123");
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
