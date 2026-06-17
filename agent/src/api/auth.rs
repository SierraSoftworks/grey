use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use actix_web::{
    HttpMessage, HttpResponse,
    body::BoxBody,
    dev::{ServiceRequest, ServiceResponse},
    http::{StatusCode, header::AUTHORIZATION},
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

    /// Exchanges an authorization code for an ID token using the confidential client credentials.
    /// The browser sends only the code; the secret stays here.
    pub async fn exchange_code(
        &self,
        oidc: &OidcConfig,
        code: &str,
        redirect_uri: &str,
    ) -> Result<String, Box<dyn Error>> {
        let discovery = self.discovery(&oidc.endpoint).await?;
        let body = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", oidc.client_id.as_str()),
            ("client_secret", oidc.client_secret.as_str()),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", form_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

        let tokens: Value = self
            .http
            .post(&discovery.token_endpoint)
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

        tokens
            .get("id_token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "the token response did not include an id_token".into())
    }
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

fn json_error(status: StatusCode, error: ApiError) -> HttpResponse {
    HttpResponse::build(status).json(error)
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

    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .map(|token| token.trim().to_string());

    let Some(token) = token else {
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

/// `POST /api/v1/auth/token` — exchanges an authorization code for an ID token using the
/// server-held client secret, returning the token to the SPA. Public (it is the login step).
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
        Ok(token) => Ok(HttpResponse::Ok().json(serde_json::json!({ "token": token }))),
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
