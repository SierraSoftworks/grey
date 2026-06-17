use actix_web::{HttpResponse, Result, web};

use super::AppState;
use crate::state::ProbeStore;

/// `GET /api/v1/probes` — every probe's current state, sorted by name. Public.
pub async fn get_probes(data: web::Data<AppState>) -> Result<HttpResponse> {
    let api_probes = data.state.get_probe_states().await?;
    let mut probes: Vec<grey_api::Probe> = api_probes.into_values().collect();
    probes.sort_by_key(|p| p.name.clone());

    Ok(HttpResponse::Ok().json(probes))
}

#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use actix_web::http::StatusCode;
    use tempfile::tempdir;

    use super::*;

    #[actix_web::test]
    async fn test_get_probes() {
        let temp_dir = tempdir().unwrap();

        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let resp = get_probes(web::Data::new(app_state)).await;

        let resp = resp.expect("Failed to get probes");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("application/json"));
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        let probes: Vec<grey_api::Probe> = serde_json::from_str(&body).unwrap();
        assert_eq!(probes.len(), 1);
    }
}
