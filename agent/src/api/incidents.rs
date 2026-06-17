use actix_web::{HttpResponse, Result, web};

use super::AppState;
use crate::state::IncidentStore;

/// `GET /api/v1/incidents` — the publicly visible incidents. Public: only incidents marked visible
/// are exposed to unauthenticated viewers (administrators use the admin API for the full list).
pub async fn get_incidents(data: web::Data<AppState>) -> Result<HttpResponse> {
    let incidents = data.state.list_incidents(false).await?;
    Ok(HttpResponse::Ok().json(incidents))
}

#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use actix_web::http::StatusCode;
    use tempfile::tempdir;

    use super::*;

    #[actix_web::test]
    async fn test_get_incidents() {
        let temp_dir = tempdir().unwrap();

        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let resp = get_incidents(web::Data::new(app_state)).await;

        let resp = resp.expect("Failed to get incidents");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("application/json"));
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        let incidents: Vec<grey_api::Incident> = serde_json::from_str(&body).unwrap();
        assert!(incidents.is_empty());
    }

    #[actix_web::test]
    async fn test_get_incidents_exposes_only_visible() {
        let temp_dir = tempdir().unwrap();
        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;

        let update = |impact| grey_api::IncidentUpdate {
            impact,
            timestamp: chrono::Utc::now(),
            message: String::new(),
        };
        // An offline opening update is public; a hidden one stays a draft.
        let visible = app_state
            .state
            .create_incident("Visible".into(), update(grey_api::Impact::Offline))
            .await
            .unwrap();
        app_state
            .state
            .create_incident("Hidden".into(), update(grey_api::Impact::Hidden))
            .await
            .unwrap();

        let resp = get_incidents(web::Data::new(app_state)).await.expect("Failed to get incidents");
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let incidents: Vec<grey_api::Incident> =
            serde_json::from_str(&String::from_utf8_lossy(&body_bytes)).unwrap();
        assert_eq!(
            incidents.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![visible.id],
            "the public endpoint must hide draft incidents from unauthenticated viewers"
        );
    }
}
