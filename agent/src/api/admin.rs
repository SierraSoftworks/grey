use actix_web::{HttpMessage, HttpRequest, HttpResponse, Result, http::header, web};
use chrono::Utc;
use grey_api::{AdminUser, ApiError, CreateIncident, Identifier, IncidentEdit, IncidentUpdate};
use serde_json::{Map, Value};
use std::str::FromStr;

use super::AppState;
use super::auth::Authenticated;
use crate::state::{CasOutcome, IncidentStore};

fn not_found() -> HttpResponse {
    HttpResponse::NotFound().json(
        ApiError::new("The incident you requested could not be found.").with_advice_lines([
            "Check that the incident ID in the address is correct.",
            "It may have been deleted since you last loaded the page.",
        ]),
    )
}

/// The ETag for an incident is simply its version, quoted per RFC 7232.
fn etag(version: u64) -> String {
    format!("\"{version}\"")
}

/// Parses the version out of an `If-Match` header (`"3"` or `W/"3"`), if present and numeric.
fn if_match_version(req: &HttpRequest) -> Option<u64> {
    let raw = req.headers().get(header::IF_MATCH)?.to_str().ok()?;
    raw.trim().trim_start_matches("W/").trim_matches('"').parse::<u64>().ok()
}

fn parse_id(raw: &str) -> Option<Identifier> {
    Identifier::from_str(raw).ok()
}

/// `GET /api/v1/admin/me` — the signed-in administrator, derived from validated token claims.
pub async fn me(req: HttpRequest) -> HttpResponse {
    let user = req
        .extensions()
        .get::<Authenticated>()
        .map(|auth| admin_user_from_claims(&auth.claims));

    match user {
        Some(user) => HttpResponse::Ok().json(user),
        None => HttpResponse::NoContent().finish(),
    }
}

fn admin_user_from_claims(claims: &Map<String, Value>) -> AdminUser {
    let string_claim = |key: &str| claims.get(key).and_then(Value::as_str).map(str::to_string);
    AdminUser {
        subject: string_claim("sub").unwrap_or_default(),
        email: string_claim("email"),
        name: string_claim("name").or_else(|| string_claim("preferred_username")),
    }
}

/// `GET /api/v1/admin/incidents` — every incident, including hidden ones.
pub async fn list_incidents(data: web::Data<AppState>) -> Result<HttpResponse> {
    let incidents = data.state.list_incidents(true).await?;
    Ok(HttpResponse::Ok().json(incidents))
}

/// `GET /api/v1/admin/incidents/{id}` — a single incident (including hidden), with its version ETag.
pub async fn get_incident(
    data: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let Some(id) = parse_id(&path.into_inner()) else {
        return Ok(not_found());
    };
    match data.state.get_incident(id).await? {
        Some(incident) => Ok(HttpResponse::Ok()
            .insert_header((header::ETAG, etag(incident.version)))
            .json(incident)),
        None => Ok(not_found()),
    }
}

/// `POST /api/v1/admin/incidents` — create an incident from a title and its opening update.
pub async fn create_incident(
    data: web::Data<AppState>,
    body: web::Json<CreateIncident>,
) -> Result<HttpResponse> {
    let input = body.into_inner();
    let initial = IncidentUpdate {
        impact: input.impact,
        timestamp: Utc::now(),
        message: input.message,
    };

    let created = data.state.create_incident(input.title, initial).await?;
    Ok(HttpResponse::Created()
        .insert_header((header::ETAG, etag(created.version)))
        .json(created))
}

/// `PUT /api/v1/admin/incidents/{id}` — replace an incident's title and updates with a check-and-set
/// against its version (the `If-Match` ETag). Returns 412 on a version mismatch, 428 when no
/// `If-Match` is supplied.
pub async fn replace_incident(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Json<IncidentEdit>,
) -> Result<HttpResponse> {
    let Some(id) = parse_id(&path.into_inner()) else {
        return Ok(not_found());
    };
    let Some(expected_version) = if_match_version(&req) else {
        return Ok(HttpResponse::PreconditionRequired().json(
            ApiError::new("An If-Match version header is required to edit this incident.")
                .with_advice("Reload the incident to obtain its current version, then retry."),
        ));
    };

    match data.state.replace_incident(id, expected_version, body.into_inner()).await? {
        CasOutcome::Updated(incident) => Ok(HttpResponse::Ok()
            .insert_header((header::ETAG, etag(incident.version)))
            .json(incident)),
        CasOutcome::VersionMismatch(current) => Ok(HttpResponse::PreconditionFailed()
            .insert_header((header::ETAG, etag(current)))
            .json(
                ApiError::new("The incident was modified by someone else.")
                    .with_advice("Reload the incident to see the latest version, then try again."),
            )),
        CasOutcome::NotFound => Ok(not_found()),
    }
}

/// `DELETE /api/v1/admin/incidents/{id}` — remove an incident.
pub async fn delete_incident(
    data: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let Some(id) = parse_id(&path.into_inner()) else {
        return Ok(not_found());
    };
    if data.state.delete_incident(id).await? {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Ok(not_found())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, body::MessageBody, http::StatusCode, test};
    use grey_api::{Impact, Incident};

    #[actix_web::test]
    async fn admin_incident_cas_lifecycle() {
        // The handlers run through a router (real path/json/header extraction) without the auth
        // middleware — that gate is tested separately in the `api` module.
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(state))
                .route("/incidents", web::get().to(list_incidents))
                .route("/incidents", web::post().to(create_incident))
                .route("/incidents/{id}", web::get().to(get_incident))
                .route("/incidents/{id}", web::put().to(replace_incident))
                .route("/incidents/{id}", web::delete().to(delete_incident)),
        )
        .await;

        // Create with an opening update -> 201 + ETag, version 1, public.
        let req = test::TestRequest::post()
            .uri("/incidents")
            .set_json(serde_json::json!({ "title": "Outage", "impact": "offline", "message": "Investigating" }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(resp.headers().get("etag").unwrap(), "\"1\"");
        let created: Incident =
            serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(created.version, 1);
        assert_eq!(created.updates.len(), 1);
        assert!(created.is_public());

        // The admin list includes it.
        let req = test::TestRequest::get().uri("/incidents").to_request();
        let all: Vec<Incident> = test::call_and_read_body_json(&app, req).await;
        assert_eq!(all.len(), 1);

        // PUT without If-Match -> 428.
        let edit = serde_json::json!({
            "title": "Outage (resolved)",
            "updates": [
                { "impact": "offline", "timestamp": 1_700_000_000, "message": "down" },
                { "impact": "none", "timestamp": 1_700_003_600, "message": "fixed" }
            ]
        });
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id))
            .set_json(edit.clone())
            .to_request();
        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::PRECONDITION_REQUIRED
        );

        // PUT with the right If-Match -> 200, version bumped to 2.
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id))
            .insert_header(("If-Match", "\"1\""))
            .set_json(edit.clone())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("etag").unwrap(), "\"2\"");
        let updated: Incident = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(updated.version, 2);
        assert_eq!(updated.title, "Outage (resolved)");
        assert_eq!(updated.current_impact(), Impact::None);

        // PUT again with the stale version -> 412.
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id))
            .insert_header(("If-Match", "\"1\""))
            .set_json(edit)
            .to_request();
        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::PRECONDITION_FAILED
        );

        // GET single -> 200 + ETag.
        let req = test::TestRequest::get().uri(&format!("/incidents/{}", created.id)).to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.headers().get("etag").unwrap(), "\"2\"");

        // Delete -> 204, then 404.
        let req = test::TestRequest::delete().uri(&format!("/incidents/{}", created.id)).to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::NO_CONTENT);
        let req = test::TestRequest::delete().uri(&format!("/incidents/{}", created.id)).to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn me_returns_user_from_claims() {
        let mut claims = Map::new();
        claims.insert("sub".into(), Value::String("user-1".into()));
        claims.insert("email".into(), Value::String("a@b.com".into()));

        let req = test::TestRequest::default().to_http_request();
        req.extensions_mut().insert(Authenticated { claims });

        let resp = me(req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().try_into_bytes().unwrap();
        let user: AdminUser = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(user.subject, "user-1");
        assert_eq!(user.email.as_deref(), Some("a@b.com"));
    }
}
