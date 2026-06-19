use actix_web::{HttpMessage, HttpRequest, HttpResponse, Result, http::header, web};
use grey_api::{
    AdminUser, ApiError, CreateIncident, CreateUpdate, Identifier, IncidentUpdateId, PutIncident,
    PutUpdate, parse_if_match, version_etag,
};
use serde_json::{Map, Value};
use std::str::FromStr;

use super::AppState;
use super::auth::Authenticated;
use super::incidents::ListQuery;
use crate::state::{CasOutcome, DEFAULT_INCIDENT_PAGE, IncidentStore};

/// The maximum length (bytes) of an incident-update message accepted at ingest. Keeps a single
/// gossiped update snapshot comfortably within the UDP datagram, which is not fragmented per-entity.
const MAX_MESSAGE_BYTES: usize = 32 * 1024;

fn not_found() -> HttpResponse {
    ApiError::not_found("The incident you requested could not be found.")
        .with_advice_lines([
            "Check that the incident ID in the address is correct.",
            "It may have been deleted since you last loaded the page.",
        ])
        .into()
}

fn too_large() -> HttpResponse {
    ApiError::payload_too_large("The update message is too large.")
        .with_advice("Shorten the message; very large updates cannot be replicated across the cluster.")
        .into()
}

/// Parses the version out of an `If-Match` header (`"3"` or `W/"3"`), if present and numeric. The
/// header extraction stays here; the [`grey_api::parse_if_match`] codec owns the wire format.
fn if_match_version(req: &HttpRequest) -> Option<u64> {
    let raw = req.headers().get(header::IF_MATCH)?.to_str().ok()?;
    parse_if_match(raw)
}

fn precondition_required() -> HttpResponse {
    ApiError::precondition_required("An If-Match version header is required to edit this resource.")
        .with_advice("Reload to obtain the current version, then retry.")
        .into()
}

fn version_conflict(current: u64) -> HttpResponse {
    // Carries the current ETag alongside the standard 412 body so the client can retry the
    // check-and-set without a separate reload.
    HttpResponse::PreconditionFailed()
        .insert_header((header::ETAG, version_etag(current)))
        .json(
            ApiError::precondition_failed("The resource was modified by someone else.")
                .with_advice("Reload to see the latest version, then try again."),
        )
}

fn parse_id(raw: &str) -> Option<Identifier> {
    Identifier::from_str(raw).ok()
}

fn parse_update_id(raw: &str) -> Option<IncidentUpdateId> {
    IncidentUpdateId::from_str(raw).ok()
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

/// `GET /api/v1/admin/incidents?limit=&cursor=` — every incident (including hidden), paginated.
pub async fn list_incidents(
    data: web::Data<AppState>,
    query: web::Query<ListQuery>,
) -> Result<HttpResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_INCIDENT_PAGE).clamp(1, 100);
    let cursor = query.cursor.as_deref().and_then(Identifier::parse);
    let page = data.state.list_incidents(true, limit, cursor).await?;
    Ok(HttpResponse::Ok().json(page))
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
        Some(view) => Ok(HttpResponse::Ok()
            .insert_header((header::ETAG, version_etag(view.incident.version)))
            .json(view)),
        None => Ok(not_found()),
    }
}

/// `POST /api/v1/admin/incidents` — create an incident from a title and its opening update.
pub async fn create_incident(
    data: web::Data<AppState>,
    body: web::Json<CreateIncident>,
) -> Result<HttpResponse> {
    let input = body.into_inner();
    if input.message.len() > MAX_MESSAGE_BYTES {
        return Ok(too_large());
    }
    let view = data.state.create_incident(input.title, input.impact, input.message).await?;
    Ok(HttpResponse::Created()
        .insert_header((header::ETAG, version_etag(view.incident.version)))
        .json(view))
}

/// `PUT /api/v1/admin/incidents/{id}` — replace an incident's title with a check-and-set against its
/// version (the `If-Match` ETag). 412 on a version mismatch, 428 when no `If-Match` is supplied.
pub async fn put_incident(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Json<PutIncident>,
) -> Result<HttpResponse> {
    let Some(id) = parse_id(&path.into_inner()) else {
        return Ok(not_found());
    };
    let Some(expected) = if_match_version(&req) else {
        return Ok(precondition_required());
    };

    match data.state.put_incident(id, expected, body.into_inner()).await? {
        CasOutcome::Updated(version, view) => Ok(HttpResponse::Ok()
            .insert_header((header::ETAG, version_etag(version)))
            .json(view)),
        CasOutcome::VersionMismatch(current) => Ok(version_conflict(current)),
        CasOutcome::NotFound => Ok(not_found()),
    }
}

/// `DELETE /api/v1/admin/incidents/{id}` — tombstone an incident (check-and-set against its version).
pub async fn delete_incident(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let Some(id) = parse_id(&path.into_inner()) else {
        return Ok(not_found());
    };
    let Some(expected) = if_match_version(&req) else {
        return Ok(precondition_required());
    };
    match data.state.delete_incident(id, expected).await? {
        CasOutcome::Updated(_, ()) => Ok(HttpResponse::NoContent().finish()),
        CasOutcome::VersionMismatch(current) => Ok(version_conflict(current)),
        CasOutcome::NotFound => Ok(not_found()),
    }
}

/// `POST /api/v1/admin/incidents/{id}/updates` — add a new update to an incident. 404 if the incident
/// does not exist. The ETag is the new update's version.
pub async fn create_update(
    data: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Json<CreateUpdate>,
) -> Result<HttpResponse> {
    let Some(id) = parse_id(&path.into_inner()) else {
        return Ok(not_found());
    };
    let input = body.into_inner();
    if input.message.len() > MAX_MESSAGE_BYTES {
        return Ok(too_large());
    }
    match data.state.create_update(id, input).await? {
        Some((version, view)) => Ok(HttpResponse::Created()
            .insert_header((header::ETAG, version_etag(version)))
            .json(view)),
        None => Ok(not_found()),
    }
}

/// `PUT /api/v1/admin/incidents/{id}/updates/{uid}` — replace an update's message (check-and-set
/// against the update's version). The update's impact is fixed once posted.
pub async fn put_update(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String)>,
    body: web::Json<PutUpdate>,
) -> Result<HttpResponse> {
    let (_incident, uid) = path.into_inner();
    let Some(uid) = parse_update_id(&uid) else {
        return Ok(not_found());
    };
    let Some(expected) = if_match_version(&req) else {
        return Ok(precondition_required());
    };
    let input = body.into_inner();
    if input.message.len() > MAX_MESSAGE_BYTES {
        return Ok(too_large());
    }
    match data.state.put_update(uid, expected, input).await? {
        CasOutcome::Updated(version, view) => Ok(HttpResponse::Ok()
            .insert_header((header::ETAG, version_etag(version)))
            .json(view)),
        CasOutcome::VersionMismatch(current) => Ok(version_conflict(current)),
        CasOutcome::NotFound => Ok(not_found()),
    }
}

/// `DELETE /api/v1/admin/incidents/{id}/updates/{uid}` — tombstone an update (check-and-set).
pub async fn delete_update(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse> {
    let (_incident, uid) = path.into_inner();
    let Some(uid) = parse_update_id(&uid) else {
        return Ok(not_found());
    };
    let Some(expected) = if_match_version(&req) else {
        return Ok(precondition_required());
    };
    match data.state.delete_update(uid, expected).await? {
        CasOutcome::Updated(_, ()) => Ok(HttpResponse::NoContent().finish()),
        CasOutcome::VersionMismatch(current) => Ok(version_conflict(current)),
        CasOutcome::NotFound => Ok(not_found()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, body::MessageBody, http::StatusCode, test};
    use grey_api::{Impact, IncidentView};

    #[actix_web::test]
    async fn admin_incident_and_update_lifecycle() {
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
                .route("/incidents/{id}", web::put().to(put_incident))
                .route("/incidents/{id}", web::delete().to(delete_incident))
                .route("/incidents/{id}/updates", web::post().to(create_update))
                .route("/incidents/{id}/updates/{uid}", web::put().to(put_update))
                .route("/incidents/{id}/updates/{uid}", web::delete().to(delete_update)),
        )
        .await;

        // Create with an opening update -> 201 + ETag, one update, public.
        let req = test::TestRequest::post()
            .uri("/incidents")
            .set_json(serde_json::json!({ "title": "Outage", "impact": "offline", "message": "Investigating" }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created: IncidentView = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(created.updates.len(), 1);
        assert!(created.is_public());
        let v0 = created.incident.version;

        // PUT title without If-Match -> 428.
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id()))
            .set_json(serde_json::json!({ "title": "Outage (renamed)" }))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::PRECONDITION_REQUIRED);

        // PUT with the right If-Match -> 200, version bumped.
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id()))
            .insert_header(("If-Match", version_etag(v0)))
            .set_json(serde_json::json!({ "title": "Outage (renamed)" }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let updated: IncidentView = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(updated.title(), "Outage (renamed)");
        let v1 = updated.incident.version;

        // PUT again with the stale version -> 412.
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id()))
            .insert_header(("If-Match", version_etag(v0)))
            .set_json(serde_json::json!({ "title": "x" }))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::PRECONDITION_FAILED);

        // Add an update -> 201, two updates, current impact follows the new update.
        let req = test::TestRequest::post()
            .uri(&format!("/incidents/{}/updates", created.id()))
            .set_json(serde_json::json!({ "impact": "none", "message": "Resolved" }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let with_update: IncidentView = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(with_update.updates.len(), 2);
        assert!(
            with_update.updates.iter().any(|u| u.impact == Impact::Offline)
                && with_update.updates.iter().any(|u| u.impact == Impact::None)
        );

        // Edit the new (None) update's message via CAS on its own version.
        let new_update = with_update.updates.iter().find(|u| u.impact == Impact::None).unwrap().clone();
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}/updates/{}", created.id(), new_update.id))
            .insert_header(("If-Match", version_etag(new_update.version)))
            .set_json(serde_json::json!({ "message": "All clear" }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let edited: IncidentView = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(edited.updates.iter().find(|u| u.id == new_update.id).unwrap().message, "All clear");

        // Delete the incident -> 204, then a GET is 404.
        let req = test::TestRequest::delete()
            .uri(&format!("/incidents/{}", created.id()))
            .insert_header(("If-Match", version_etag(v1)))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::NO_CONTENT);
        let req = test::TestRequest::get().uri(&format!("/incidents/{}", created.id())).to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn oversized_messages_are_rejected_with_413() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(state))
                .route("/incidents", web::post().to(create_incident))
                .route("/incidents/{id}/updates", web::post().to(create_update)),
        )
        .await;

        // A create whose opening update exceeds the cap is refused before any storage write.
        let huge = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let req = test::TestRequest::post()
            .uri("/incidents")
            .set_json(serde_json::json!({ "title": "Outage", "impact": "offline", "message": huge }))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::PAYLOAD_TOO_LARGE);
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
