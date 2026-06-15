use actix_web::{HttpMessage, HttpRequest, HttpResponse, Result, web};
use chrono::{DateTime, Utc};
use grey_api::{AdminUser, Incident, IncidentInput, IncidentUpdate, NewIncidentUpdate};
use serde_json::{Map, Value};

use super::AppState;
use super::auth::Authenticated;

/// A time-sortable incident/update id: a zero-padded millisecond timestamp with a random suffix to
/// avoid collisions between records created within the same millisecond.
fn new_id(now: DateTime<Utc>) -> String {
    format!("{:013}-{:08x}", now.timestamp_millis(), rand::random::<u32>())
}

fn not_found() -> HttpResponse {
    HttpResponse::NotFound().json(serde_json::json!({ "error": "Incident not found." }))
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

/// `POST /api/v1/admin/incidents` — create an incident.
pub async fn create_incident(
    data: web::Data<AppState>,
    body: web::Json<IncidentInput>,
) -> Result<HttpResponse> {
    let now = Utc::now();
    let input = body.into_inner();
    let incident = Incident {
        id: new_id(now),
        title: input.title,
        description: input.description,
        start_time: input.start_time,
        end_time: input.end_time,
        affected_services: input.affected_services,
        updates: vec![],
        created_at: now,
        updated_at: now,
    };

    data.state.put_incident(&incident).await?;
    Ok(HttpResponse::Created().json(incident))
}

/// `PUT /api/v1/admin/incidents/{id}` — replace an incident's editable fields, preserving its id,
/// creation time and status updates.
pub async fn update_incident(
    data: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Json<IncidentInput>,
) -> Result<HttpResponse> {
    let id = path.into_inner();
    let Some(mut incident) = data.state.get_incident(&id).await? else {
        return Ok(not_found());
    };

    let input = body.into_inner();
    incident.title = input.title;
    incident.description = input.description;
    incident.start_time = input.start_time;
    incident.end_time = input.end_time;
    incident.affected_services = input.affected_services;
    incident.updated_at = Utc::now();

    data.state.put_incident(&incident).await?;
    Ok(HttpResponse::Ok().json(incident))
}

/// `DELETE /api/v1/admin/incidents/{id}` — remove an incident.
pub async fn delete_incident(
    data: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    if data.state.delete_incident(&path.into_inner()).await? {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Ok(not_found())
    }
}

/// `POST /api/v1/admin/incidents/{id}/updates` — append a status update, returning the updated
/// incident.
pub async fn add_update(
    data: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Json<NewIncidentUpdate>,
) -> Result<HttpResponse> {
    let id = path.into_inner();
    let now = Utc::now();
    let input = body.into_inner();
    let update = IncidentUpdate {
        id: new_id(now),
        impact: input.impact,
        timestamp: input.timestamp.unwrap_or(now),
        message: input.message,
    };

    if data.state.add_incident_update(&id, update).await? {
        let incident = data.state.get_incident(&id).await?;
        Ok(HttpResponse::Created().json(incident))
    } else {
        Ok(not_found())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, body::MessageBody, http::StatusCode, test};
    use grey_api::Impact;
    use tempfile::tempdir;

    #[actix_web::test]
    async fn admin_incident_lifecycle() {
        // The handlers are exercised through a router (so `web::Path`/`web::Json` extraction is
        // real), but without the auth middleware — that gate is tested separately in the `api` module.
        let dir = tempdir().unwrap();
        let state = AppState::test(dir.path().to_path_buf()).await;
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(state))
                .route("/incidents", web::get().to(list_incidents))
                .route("/incidents", web::post().to(create_incident))
                .route("/incidents/{id}", web::put().to(update_incident))
                .route("/incidents/{id}", web::delete().to(delete_incident))
                .route("/incidents/{id}/updates", web::post().to(add_update)),
        )
        .await;

        // Create: a fresh incident has no updates, so it is a hidden draft.
        let req = test::TestRequest::post()
            .uri("/incidents")
            .set_json(serde_json::json!({
                "title": "Outage", "description": "desc", "start_time": 1_700_000_000
            }))
            .to_request();
        let created: Incident = test::call_and_read_body_json(&app, req).await;
        assert_eq!(created.title, "Outage");
        assert!(created.updates.is_empty());
        assert!(!created.is_public(), "a new incident is a hidden draft");

        // The admin list includes it.
        let req = test::TestRequest::get().uri("/incidents").to_request();
        let all: Vec<Incident> = test::call_and_read_body_json(&app, req).await;
        assert_eq!(all.len(), 1);

        // Replace editable fields: rename and resolve. id/created_at are preserved.
        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id))
            .set_json(serde_json::json!({
                "title": "Outage (resolved)", "start_time": 1_700_000_000,
                "end_time": 1_700_003_600, "affected_services": ["api"]
            }))
            .to_request();
        let updated: Incident = test::call_and_read_body_json(&app, req).await;
        assert_eq!(updated.title, "Outage (resolved)");
        assert!(updated.end_time.is_some());
        assert_eq!(updated.affected_services, vec!["api".to_string()]);
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.created_at, created.created_at);

        // Posting an offline update publishes it and sets its impact.
        let req = test::TestRequest::post()
            .uri(&format!("/incidents/{}/updates", created.id))
            .set_json(serde_json::json!({ "impact": "offline", "message": "Investigating" }))
            .to_request();
        let with_update: Incident = test::call_and_read_body_json(&app, req).await;
        assert_eq!(with_update.updates.len(), 1);
        assert_eq!(with_update.updates[0].impact, Impact::Offline);
        assert!(with_update.is_public());

        // Delete → 204, then everything 404s.
        let req = test::TestRequest::delete().uri(&format!("/incidents/{}", created.id)).to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::NO_CONTENT);

        let req = test::TestRequest::delete().uri(&format!("/incidents/{}", created.id)).to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::NOT_FOUND);

        let req = test::TestRequest::put()
            .uri(&format!("/incidents/{}", created.id))
            .set_json(serde_json::json!({ "title": "x", "start_time": 1 }))
            .to_request();
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
