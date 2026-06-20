use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use actix_web::{
    HttpMessage, HttpRequest, HttpResponse,
    body::BoxBody,
    dev::{ServiceRequest, ServiceResponse},
    http::{StatusCode, header::AUTHORIZATION, header::HeaderMap},
    middleware::Next,
    web,
};
use filt_rs::{FilterValue, Filterable};
use grey_api::ApiError;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::Deserialize;
use serde_json::{Map, Value};
use tracing::{info, warn};

use super::AppState;
use crate::config::OidcConfig;

const DISCOVERY_TTL: Duration = Duration::from_secs(60 * 60); // 1 hour
const JWKS_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// The subset of the OIDC discovery document the agent needs to validate tokens and to drive the
/// server-side code exchange.
#[derive(Clone, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub jwks_uri: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
}

/// Validated token claims, made available to handlers via request extensions.
#[derive(Clone)]
pub struct Authenticated {
    pub claims: Map<String, Value>,
}

/// Validates OIDC bearer tokens, caching the provider's discovery document and signing keys.
pub struct OidcVerifier {
    http: reqwest::Client,
    discovery: Mutex<HashMap<String, (Instant, OidcDiscovery)>>,
    jwks: Mutex<HashMap<String, (Instant, JwkSet)>>,
}

impl Default for OidcVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl OidcVerifier {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            discovery: Mutex::new(HashMap::new()),
            jwks: Mutex::new(HashMap::new()),
        }
    }

    async fn discovery(&self, endpoint: &str) -> Result<OidcDiscovery, Box<dyn Error>> {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        if let Some((fetched, doc)) = self.discovery.lock().unwrap().get(&endpoint)
            && fetched.elapsed() < DISCOVERY_TTL
        {
            return Ok(doc.clone());
        }

        let url = format!("{endpoint}/.well-known/openid-configuration");
        let doc: OidcDiscovery = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        self.discovery
            .lock()
            .unwrap()
            .insert(endpoint, (Instant::now(), doc.clone()));
        Ok(doc)
    }

    async fn jwks(&self, discovery: &OidcDiscovery, force: bool) -> Result<JwkSet, Box<dyn Error>> {
        let key = discovery.jwks_uri.clone();
        if !force
            && let Some((fetched, set)) = self.jwks.lock().unwrap().get(&key)
            && fetched.elapsed() < JWKS_TTL
        {
            return Ok(set.clone());
        }

        let set: JwkSet = self
            .http
            .get(&discovery.jwks_uri)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        self.jwks
            .lock()
            .unwrap()
            .insert(key, (Instant::now(), set.clone()));
        Ok(set)
    }

    /// Validates a bearer token's signature and registered claims against the configured provider,
    /// returning the decoded claim set.
    pub async fn validate(
        &self,
        oidc: &OidcConfig,
        token: &str,
    ) -> Result<Map<String, Value>, Box<dyn Error>> {
        let discovery = self.discovery(&oidc.endpoint).await?;
        let mut set = self.jwks(&discovery, false).await?;

        // The provider may have rotated keys since we cached the JWKS; refetch once on an unknown
        // key id before rejecting the token, so a rotation doesn't lock out valid sessions.
        if signing_key_missing(&set, token) {
            set = self.jwks(&discovery, true).await?;
        }

        verify_token(&oidc.client_id, &discovery.issuer, &set, token)
    }

    /// The provider's authorization endpoint, which the SPA needs to begin the login redirect.
    /// Resolved from the (cached) discovery document so the browser never has to call the provider
    /// cross-origin.
    pub async fn authorization_endpoint(&self, oidc: &OidcConfig) -> Result<String, Box<dyn Error>> {
        Ok(self.discovery(&oidc.endpoint).await?.authorization_endpoint)
    }

    /// Exchanges an authorization code for tokens using the confidential client credentials. The
    /// browser sends only the code; the secret stays here. Returns the `id_token` (used as the admin
    /// bearer) alongside the provider's `refresh_token`, if one was issued.
    pub async fn exchange_code(
        &self,
        oidc: &OidcConfig,
        code: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, Box<dyn Error>> {
        let discovery = self.discovery(&oidc.endpoint).await?;
        let body = form_body(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", oidc.client_id.as_str()),
            ("client_secret", oidc.client_secret.as_str()),
        ]);

        self.token_request(&discovery.token_endpoint, body).await
    }

    /// Renews a session from a previously issued refresh token, returning a fresh `id_token` (and a
    /// rotated refresh token when the provider supplies one). Lets the SPA extend a session without
    /// a new interactive login.
    pub async fn refresh_tokens(
        &self,
        oidc: &OidcConfig,
        refresh_token: &str,
    ) -> Result<TokenSet, Box<dyn Error>> {
        let discovery = self.discovery(&oidc.endpoint).await?;
        let body = form_body(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", oidc.client_id.as_str()),
            ("client_secret", oidc.client_secret.as_str()),
        ]);

        let mut tokens = self.token_request(&discovery.token_endpoint, body).await?;
        // Providers that don't rotate refresh tokens omit it from the response; keep using the one
        // the caller already holds so the session can continue to be renewed.
        if tokens.refresh_token.is_none() {
            tokens.refresh_token = Some(refresh_token.to_string());
        }
        Ok(tokens)
    }

    /// POSTs a form-encoded grant to the provider's token endpoint and parses the issued tokens.
    async fn token_request(
        &self,
        token_endpoint: &str,
        body: String,
    ) -> Result<TokenSet, Box<dyn Error>> {
        let tokens: Value = self
            .http
            .post(token_endpoint)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let id_token = tokens
            .get("id_token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or("the token response did not include an id_token")?;
        let refresh_token = tokens
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(TokenSet {
            id_token,
            refresh_token,
        })
    }
}

/// Tokens issued by the OIDC token endpoint. `id_token` is the bearer the agent validates;
/// `refresh_token` (when present) lets the SPA renew the session without re-authenticating.
#[derive(Clone)]
pub struct TokenSet {
    pub id_token: String,
    pub refresh_token: Option<String>,
}

/// Builds an `application/x-www-form-urlencoded` body from key/value pairs.
fn form_body(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{k}={}", form_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// RFC 3986 percent-encoding for an `application/x-www-form-urlencoded` field value.
fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Whether the token's signing-key id is absent from the cached key set (suggesting key rotation).
fn signing_key_missing(set: &JwkSet, token: &str) -> bool {
    match jsonwebtoken::decode_header(token) {
        Ok(header) => header.kid.is_some_and(|kid| set.find(&kid).is_none()),
        Err(_) => false,
    }
}

/// Verifies a token's signature and registered claims (`aud`, `iss`, `exp`, `nbf`).
fn verify_token(
    client_id: &str,
    issuer: &str,
    set: &JwkSet,
    token: &str,
) -> Result<Map<String, Value>, Box<dyn Error>> {
    let header = jsonwebtoken::decode_header(token)?;

    // Reject symmetric algorithms outright: JWKS publishes asymmetric keys, and accepting HMAC would
    // open an algorithm-confusion attack where a public key is used as an HMAC secret.
    if matches!(
        header.alg,
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
    ) {
        return Err("token is signed with an unsupported symmetric algorithm".into());
    }

    let kid = header
        .kid
        .ok_or("token does not identify a signing key (missing kid)")?;
    let jwk = set
        .find(&kid)
        .ok_or("token was signed with an unknown key")?;
    let decoding_key = DecodingKey::from_jwk(jwk)?;

    let mut validation = Validation::new(header.alg);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&[issuer]);
    validation.validate_exp = true;
    validation.validate_nbf = true;

    let data = jsonwebtoken::decode::<Map<String, Value>>(token, &decoding_key, &validation)?;
    Ok(data.claims)
}

/// Exposes request metadata and validated token claims to the admin ACL expression. Token claims are
/// addressed under the `claims.` prefix (e.g. `claims.email`).
struct AdminRequestFilter<'a> {
    method: &'a str,
    path: &'a str,
    claims: &'a Map<String, Value>,
}

impl Filterable for AdminRequestFilter<'_> {
    fn get(&self, key: &str) -> FilterValue<'_> {
        match key {
            "method" => FilterValue::String(Cow::Borrowed(self.method)),
            "path" => FilterValue::String(Cow::Borrowed(self.path)),
            k if k.starts_with("claims.") => self
                .claims
                .get(&k["claims.".len()..])
                .map(json_to_filter_value)
                .unwrap_or(FilterValue::Null),
            _ => FilterValue::Null,
        }
    }
}

fn json_to_filter_value(value: &Value) -> FilterValue<'_> {
    match value {
        Value::Null => FilterValue::Null,
        Value::Bool(b) => FilterValue::Bool(*b),
        Value::Number(n) => FilterValue::Number(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => FilterValue::String(Cow::Borrowed(s)),
        Value::Array(items) => FilterValue::Tuple(items.iter().map(json_to_filter_value).collect()),
        // Object-valued claims have no scalar filter representation; treat them as absent.
        Value::Object(_) => FilterValue::Null,
    }
}

/// Extracts a bearer token from an `Authorization` header, accepting either capitalisation of the
/// `Bearer` scheme. Shared by the admin middleware and the public-endpoint [`resolve_auth_context`].
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .map(|token| token.trim().to_string())
}

/// A request's resolved authentication context, used to decide which probes and crons a viewer may
/// see via their `visible` filter.
///
/// Unlike [`require_admin`] (which rejects an unauthorised request outright), this is computed for the
/// *public* read endpoints, so an anonymous request — or one carrying an invalid/expired token —
/// simply resolves to "not authenticated, not admin" and sees only the entities visible to everyone.
#[derive(Clone, Default)]
pub struct AuthContext {
    /// Whether a valid OIDC bearer token accompanied the request.
    pub authenticated: bool,
    /// Whether the validated identity also satisfied the configured admin ACL.
    pub admin: bool,
    /// The validated token claims (empty when unauthenticated), exposed to a `visible` filter under
    /// the `claims.` prefix for parity with the admin ACL.
    pub claims: Map<String, Value>,
}

impl AuthContext {
    /// Whether a viewer with this context may see an entity guarded by `visible`. A filter that fails
    /// to evaluate is treated as *not* visible (fail-closed), matching the deny-by-default posture of
    /// the admin ACL: a malformed visibility rule hides the entity rather than leaking it.
    pub fn can_see(&self, visible: &filt_rs::Filter) -> bool {
        visible.matches(&VisibilityFilter(self)).unwrap_or(false)
    }
}

/// Exposes a request's resolved [`AuthContext`] to a probe/cron `visible` filter. The addressable
/// fields are `auth` (a boolean: any valid token was presented), `auth.admin` (a boolean: the admin
/// ACL passed), and `claims.<name>` (a validated token claim). Unknown keys resolve to null, matching
/// `filt-rs`'s own convention — so `visible: auth.admin` restricts an entity to administrators while
/// the default `visible: true` shows it to everyone.
struct VisibilityFilter<'a>(&'a AuthContext);

impl Filterable for VisibilityFilter<'_> {
    fn get(&self, key: &str) -> FilterValue<'_> {
        match key {
            "auth" => FilterValue::Bool(self.0.authenticated),
            "auth.admin" => FilterValue::Bool(self.0.admin),
            k if k.starts_with("claims.") => self
                .0
                .claims
                .get(&k["claims.".len()..])
                .map(json_to_filter_value)
                .unwrap_or(FilterValue::Null),
            _ => FilterValue::Null,
        }
    }
}

/// Resolves the [`AuthContext`] for a request to a public read endpoint.
///
/// Returns an anonymous context when admin auth is not configured, or when the request carries no
/// bearer token at all — such a viewer sees only the entities visible to everyone. A *valid* token
/// populates the claims and marks the request authenticated; the configured admin ACL is then
/// evaluated (against the same `method`/`path`/`claims` fields [`require_admin`] uses) to decide
/// whether the viewer also counts as an administrator.
///
/// A token that is *present but invalid or expired* is reported as an [`Err`] (a `401`) rather than
/// being silently downgraded to the anonymous view. A viewer who presents a token is asking for their
/// authenticated set; quietly returning the smaller public set instead would make a signed-in
/// administrator's probes and crons vanish the moment their token lapsed — e.g. while the tab sat
/// backgrounded and polling was paused — with no 401 to prompt the SPA to renew it. Surfacing the
/// 401 lets the client refresh the session and retry (its existing reaction to a 401), restoring the
/// authenticated set. The SSR page render, where a token can't be renewed mid-request, opts back into
/// the anonymous fallback via `.unwrap_or_default()`.
pub async fn resolve_auth_context(
    req: &HttpRequest,
    data: &AppState,
) -> Result<AuthContext, ApiError> {
    let config = data.state.get_config();
    let Some(admin) = config.ui.admin.as_ref() else {
        return Ok(AuthContext::default());
    };

    let Some(token) = bearer_token(req.headers()) else {
        return Ok(AuthContext::default());
    };

    let claims = match data.oidc.validate(&admin.oidc, &token).await {
        Ok(claims) => claims,
        Err(e) => {
            info!("Rejected a public read carrying an invalid bearer token: {e}");
            return Err(ApiError::unauthorized("Your session is invalid or has expired.")
                .with_advice("Sign in again to continue."));
        }
    };

    let is_admin = admin
        .acl
        .matches(&AdminRequestFilter {
            method: req.method().as_str(),
            path: req.path(),
            claims: &claims,
        })
        .unwrap_or(false);

    Ok(AuthContext {
        authenticated: true,
        admin: is_admin,
        claims,
    })
}

/// Drops the probes the given viewer may not see, honouring each probe's configured `visible` filter.
/// A pooled probe with no local configuration entry (e.g. a gossiped record for a probe this node no
/// longer configures) has no `visible` rule and stays visible, preserving the pre-visibility default.
pub fn retain_visible_probes(
    config: &crate::config::Config,
    ctx: &AuthContext,
    probes: &mut Vec<grey_api::Probe>,
) {
    probes.retain(|probe| {
        config
            .probes
            .iter()
            .find(|cfg| cfg.name == probe.name)
            .map(|cfg| ctx.can_see(&cfg.visible))
            .unwrap_or(true)
    });
}

/// Drops the crons the given viewer may not see, honouring each cron's configured `visible` filter.
/// As with [`retain_visible_probes`], a pooled cron with no local configuration entry stays visible.
pub fn retain_visible_crons(
    config: &crate::config::Config,
    ctx: &AuthContext,
    crons: &mut Vec<grey_api::Cron>,
) {
    crons.retain(|cron| {
        config
            .crons
            .iter()
            .find(|cfg| cfg.name == cron.name)
            .map(|cfg| ctx.can_see(&cfg.visible))
            .unwrap_or(true)
    });
}

fn json_error(status: StatusCode, error: ApiError) -> HttpResponse {
    // Stamp the HTTP status onto the body so clients can classify the failure from the error object
    // alone (the SPA branches on this code without re-reading the transport status). `ApiError`'s
    // `Into<HttpResponse>` then renders the JSON body with the matching status.
    error.with_code(status.as_u16()).into()
}

/// Middleware guarding the admin API: it requires a valid OIDC bearer token whose claims satisfy the
/// configured ACL. `401` is returned when the token is missing/invalid, `403` when the ACL denies a
/// validated identity, and the admin scope is closed entirely when no `admin` config is present.
pub async fn require_admin(
    req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let Some(data) = req.app_data::<web::Data<AppState>>().cloned() else {
        return Ok(req.into_response(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::new("The service context was unavailable while handling your request.")
                .with_advice("This is a server-side problem; please try again shortly."),
        )));
    };

    let config = data.state.get_config();
    let Some(admin) = config.ui.admin.as_ref() else {
        return Ok(req.into_response(json_error(
            StatusCode::FORBIDDEN,
            ApiError::new("Administrative access is not configured on this server."),
        )));
    };

    let Some(token) = bearer_token(req.headers()) else {
        return Ok(req.into_response(json_error(
            StatusCode::UNAUTHORIZED,
            ApiError::new("Authentication is required to access this resource.")
                .with_advice("Sign in and try again."),
        )));
    };

    let claims = match data.oidc.validate(&admin.oidc, &token).await {
        Ok(claims) => claims,
        Err(e) => {
            info!("Rejected an admin request with an invalid bearer token: {e}");
            return Ok(req.into_response(json_error(
                StatusCode::UNAUTHORIZED,
                ApiError::new("Your session is invalid or has expired.")
                    .with_advice("Sign in again to continue."),
            )));
        }
    };

    let allowed = admin
        .acl
        .matches(&AdminRequestFilter {
            method: req.method().as_str(),
            path: req.path(),
            claims: &claims,
        })
        .unwrap_or(false);

    if !allowed {
        // Authentication already succeeded, so a denial here is a permanent authorization failure.
        return Ok(req.into_response(json_error(
            StatusCode::FORBIDDEN,
            ApiError::new("Your account is not permitted to perform this action.")
                .with_advice("Contact an administrator if you believe you should have access."),
        )));
    }

    req.extensions_mut().insert(Authenticated { claims });
    next.call(req).await
}

/// `GET /api/v1/auth/metadata` — the provider's authorization endpoint, so the SPA can build its
/// login redirect without calling the provider cross-origin. Public (it precedes authentication).
pub async fn metadata(data: web::Data<AppState>) -> actix_web::Result<HttpResponse> {
    let config = data.state.get_config();
    let Some(admin) = config.ui.admin.as_ref() else {
        return Ok(json_error(
            StatusCode::NOT_FOUND,
            ApiError::new("Administrative access is not configured on this server."),
        ));
    };

    match data.oidc.authorization_endpoint(&admin.oidc).await {
        Ok(authorization_endpoint) => {
            Ok(HttpResponse::Ok().json(serde_json::json!({ "authorization_endpoint": authorization_endpoint })))
        }
        Err(e) => {
            warn!("Failed to resolve the OIDC authorization endpoint: {e}");
            Ok(json_error(
                StatusCode::BAD_GATEWAY,
                ApiError::new("We could not reach the configured identity provider.")
                    .with_advice("Please try again in a few moments."),
            ))
        }
    }
}

#[derive(Deserialize)]
pub struct TokenExchangeRequest {
    pub code: String,
    pub redirect_uri: String,
}

/// `POST /api/v1/auth/token` — exchanges an authorization code for tokens using the server-held
/// client secret, returning the `id_token` (and refresh token, if issued) to the SPA. Public (it is
/// the login step).
pub async fn exchange_token(
    data: web::Data<AppState>,
    body: web::Json<TokenExchangeRequest>,
) -> actix_web::Result<HttpResponse> {
    let config = data.state.get_config();
    let Some(admin) = config.ui.admin.as_ref() else {
        return Ok(json_error(
            StatusCode::NOT_FOUND,
            ApiError::new("Administrative access is not configured on this server."),
        ));
    };

    let body = body.into_inner();
    match data
        .oidc
        .exchange_code(&admin.oidc, &body.code, &body.redirect_uri)
        .await
    {
        Ok(tokens) => Ok(HttpResponse::Ok().json(token_response(&tokens))),
        Err(e) => {
            warn!("OIDC code exchange failed: {e}");
            Ok(json_error(
                StatusCode::BAD_GATEWAY,
                ApiError::new("The sign-in could not be completed.")
                    .with_advice("Please try signing in again."),
            ))
        }
    }
}

#[derive(Deserialize)]
pub struct TokenRefreshRequest {
    pub refresh_token: String,
}

/// `POST /api/v1/auth/refresh` — renews a session from a refresh token, returning a fresh `id_token`
/// (and rotated refresh token, when the provider issues one). Public (a refresh token is the only
/// credential required, and the agent re-validates the resulting id_token on subsequent requests).
pub async fn refresh_token(
    data: web::Data<AppState>,
    body: web::Json<TokenRefreshRequest>,
) -> actix_web::Result<HttpResponse> {
    let config = data.state.get_config();
    let Some(admin) = config.ui.admin.as_ref() else {
        return Ok(json_error(
            StatusCode::NOT_FOUND,
            ApiError::new("Administrative access is not configured on this server."),
        ));
    };

    let body = body.into_inner();
    match data.oidc.refresh_tokens(&admin.oidc, &body.refresh_token).await {
        Ok(tokens) => Ok(HttpResponse::Ok().json(token_response(&tokens))),
        Err(e) => {
            warn!("OIDC token refresh failed: {e}");
            Ok(json_error(
                StatusCode::UNAUTHORIZED,
                ApiError::new("Your session could not be renewed.")
                    .with_advice("Please sign in again."),
            ))
        }
    }
}

/// The JSON body returned for a successful token exchange or refresh. `refresh_token` is omitted when
/// the provider did not issue one.
fn token_response(tokens: &TokenSet) -> Value {
    let mut body = serde_json::Map::new();
    body.insert("token".into(), Value::String(tokens.id_token.clone()));
    if let Some(refresh) = &tokens.refresh_token {
        body.insert("refresh_token".into(), Value::String(refresh.clone()));
    }
    Value::Object(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_all_acl_rejects_everything() {
        let acl = filt_rs::Filter::new("false").unwrap();
        let claims = serde_json::Map::new();
        let filter = AdminRequestFilter {
            method: "POST",
            path: "/api/v1/admin/incidents",
            claims: &claims,
        };
        assert!(!acl.matches(&filter).unwrap());
    }

    #[test]
    fn acl_can_match_on_claims() {
        let acl = filt_rs::Filter::new(r#"claims.email == "admin@example.com""#).unwrap();

        let mut claims = serde_json::Map::new();
        claims.insert("email".into(), Value::String("admin@example.com".into()));
        assert!(
            acl.matches(&AdminRequestFilter { method: "GET", path: "/", claims: &claims })
                .unwrap()
        );

        let mut other = serde_json::Map::new();
        other.insert("email".into(), Value::String("nope@example.com".into()));
        assert!(
            !acl.matches(&AdminRequestFilter { method: "GET", path: "/", claims: &other })
                .unwrap()
        );
    }

    #[test]
    fn acl_can_match_membership_in_a_claims_array() {
        let acl = filt_rs::Filter::new(r#"claims.groups contains "admins""#).unwrap();
        let mut claims = serde_json::Map::new();
        claims.insert(
            "groups".into(),
            Value::Array(vec![Value::String("users".into()), Value::String("admins".into())]),
        );
        assert!(
            acl.matches(&AdminRequestFilter { method: "GET", path: "/", claims: &claims })
                .unwrap()
        );
    }

    /// The `visible` filter sees `auth` (any valid session) and `auth.admin` (the admin ACL passed)
    /// as booleans, and the default `true` filter is visible to everyone.
    #[test]
    fn visibility_filter_exposes_auth_and_admin_flags() {
        let public = filt_rs::Filter::new("true").unwrap();
        let any_auth = filt_rs::Filter::new("auth").unwrap();
        let admin_only = filt_rs::Filter::new("auth.admin").unwrap();

        let anonymous = AuthContext::default();
        let signed_in = AuthContext { authenticated: true, admin: false, claims: Map::new() };
        let admin = AuthContext { authenticated: true, admin: true, claims: Map::new() };

        // The default filter shows the entity to everyone.
        assert!(anonymous.can_see(&public));
        assert!(signed_in.can_see(&public));
        assert!(admin.can_see(&public));

        // `auth` gates on any valid session.
        assert!(!anonymous.can_see(&any_auth));
        assert!(signed_in.can_see(&any_auth));
        assert!(admin.can_see(&any_auth));

        // `auth.admin` gates on passing the admin ACL.
        assert!(!anonymous.can_see(&admin_only));
        assert!(!signed_in.can_see(&admin_only));
        assert!(admin.can_see(&admin_only));
    }

    /// For parity with the admin ACL, a `visible` filter can also match on token `claims.*`; an
    /// anonymous viewer carries no claims, so a claims-based filter excludes them.
    #[test]
    fn visibility_filter_exposes_claims() {
        let in_group = filt_rs::Filter::new(r#"claims.groups contains "ops""#).unwrap();

        assert!(!AuthContext::default().can_see(&in_group), "an anonymous viewer has no claims");

        let mut claims = Map::new();
        claims.insert(
            "groups".into(),
            Value::Array(vec![Value::String("ops".into()), Value::String("dev".into())]),
        );
        let ctx = AuthContext { authenticated: true, admin: false, claims };
        assert!(ctx.can_see(&in_group));
    }

    /// The retain helper drops entities the viewer may not see, keeps unrestricted ones, and leaves
    /// pooled records with no local configuration entry (orphans) visible.
    #[tokio::test]
    async fn retain_visible_probes_honours_context_and_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let config_yaml = format!(
            "state: {}\nprobes:\n  - name: public.probe\n    policy: {{ interval: 60s, timeout: 5s }}\n    target: !Http\n      url: https://example.com\n  - name: admin.probe\n    policy: {{ interval: 60s, timeout: 5s }}\n    target: !Http\n      url: https://example.com\n    visible: auth.admin\n",
            dir.path().join("s.redb").display().to_string().replace('\\', "/")
        );
        let path = dir.path().join("c.yml");
        tokio::fs::write(&path, config_yaml).await.unwrap();
        let config = crate::Config::load_from_path(&path).await.unwrap();

        let api_probe = |name: &str| grey_api::Probe {
            name: name.into(),
            tags: std::collections::HashMap::new(),
            last_updated: chrono::DateTime::UNIX_EPOCH,
            history: Vec::new(),
            observations: std::collections::HashMap::new(),
            streak: grey_api::Streak::default(),
        };

        // Anonymous: only the public probe survives, but the orphan (no config entry) is kept.
        let mut probes = vec![api_probe("public.probe"), api_probe("admin.probe"), api_probe("orphan.probe")];
        retain_visible_probes(&config, &AuthContext::default(), &mut probes);
        let mut names: Vec<&str> = probes.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["orphan.probe", "public.probe"]);

        // An administrator sees the restricted probe too.
        let admin = AuthContext { authenticated: true, admin: true, claims: Map::new() };
        let mut probes = vec![api_probe("public.probe"), api_probe("admin.probe")];
        retain_visible_probes(&config, &admin, &mut probes);
        let mut names: Vec<&str> = probes.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["admin.probe", "public.probe"]);
    }

    #[test]
    fn symmetric_algorithms_are_rejected() {
        use base64::prelude::*;

        // Craft a token with an HS256 header (no signing needed — verification rejects it on the
        // algorithm before any key lookup, guarding against algorithm confusion).
        let header = BASE64_URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = BASE64_URL_SAFE_NO_PAD.encode(b"{}");
        let token = format!("{header}.{payload}.signature");

        let empty = JwkSet { keys: vec![] };
        let err = verify_token("client", "https://issuer", &empty, &token).unwrap_err();
        assert!(err.to_string().contains("symmetric"));
    }
}
