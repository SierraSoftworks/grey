use actix_web::{HttpResponse, Result, web};

use super::AppState;

/// `GET /api/v1/admin/cluster/peers` — the cluster's peers as seen by this node, sorted by id.
///
/// Cluster topology exposes peer addresses and health, so this endpoint is operator-only and lives
/// behind the admin authentication gate (see [`super::create_app`]).
pub async fn get_peers(data: web::Data<AppState>) -> Result<HttpResponse> {
    let mut peers = data.state.get_peers().await?;
    peers.sort_by_key(|p| p.id.clone());

    Ok(HttpResponse::Ok().json(peers))
}

#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use actix_web::http::StatusCode;
    use tempfile::tempdir;

    use super::*;

    #[actix_web::test]
    async fn test_get_peers() {
        let temp_dir = tempdir().unwrap();

        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let resp = get_peers(web::Data::new(app_state)).await;

        let resp = resp.expect("Failed to get peers");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("application/json"));
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        let peers: Vec<grey_api::Peer> = serde_json::from_str(&body).unwrap();
        // One gossiped peer plus the serving node itself.
        assert_eq!(peers.len(), 2);
        assert_eq!(peers.iter().filter(|p| p.current).count(), 1);
    }
}
