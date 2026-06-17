use actix_web::{HttpResponse, Result, web};

use super::AppState;

/// `GET /api/v1/notices` — the configured UI notices. Public.
pub async fn get_notices(data: web::Data<AppState>) -> Result<HttpResponse> {
    let api_notices: Vec<grey_api::UiNotice> = data
        .state
        .get_config()
        .ui
        .notices
        .iter()
        .map(|notice| notice.clone().into())
        .collect();
    Ok(HttpResponse::Ok().json(api_notices))
}

#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use actix_web::http::StatusCode;
    use tempfile::tempdir;

    use super::*;

    #[actix_web::test]
    async fn test_get_notices() {
        let temp_dir = tempdir().unwrap();

        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let resp = get_notices(web::Data::new(app_state)).await;

        let resp = resp.expect("Failed to get notices");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("application/json"));
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        let notices: Vec<grey_api::UiNotice> = serde_json::from_str(&body).unwrap();
        assert!(notices.is_empty());
    }
}
