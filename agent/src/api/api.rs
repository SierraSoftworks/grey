use actix_web::{HttpResponse, Result, web};

use super::AppState;

impl From<&crate::config::UiConfig> for grey_api::UiConfig {
    fn from(config: &crate::config::UiConfig) -> Self {
        grey_api::UiConfig {
            title: config.title.clone(),
            logo: config.logo.clone(),
            links: config.links.clone(),
            reload_interval: config.reload_interval,
            // Expose only the public OIDC parameters the SPA needs for browser-side PKCE — never the
            // client secret (there is none) or the admin ACL.
            auth: config.admin.as_ref().map(|admin| grey_api::UiAuthConfig {
                issuer: admin.oidc.endpoint.clone(),
                client_id: admin.oidc.client_id.clone(),
                scopes: admin.oidc.scopes.clone(),
            }),
        }
    }
}

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

pub async fn get_incidents(data: web::Data<AppState>) -> Result<HttpResponse> {
    // Public endpoint: only incidents marked visible are exposed to unauthenticated viewers.
    let incidents = data.state.list_incidents(false).await?;
    Ok(HttpResponse::Ok().json(incidents))
}

pub async fn get_probes(data: web::Data<AppState>) -> Result<HttpResponse> {
    let api_probes = data.state.get_probe_states().await?;
    let mut probes: Vec<grey_api::Probe> = api_probes.into_values().collect();
    probes.sort_by_key(|p| p.name.clone());

    Ok(HttpResponse::Ok().json(probes))
}

pub async fn get_peers(data: web::Data<AppState>) -> Result<HttpResponse> {
    let mut peers = data.state.get_peers().await?;
    peers.sort_by_key(|p| p.id.clone());

    Ok(HttpResponse::Ok().json(peers))
}


#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use tempfile::tempdir;
    use actix_web::http::StatusCode;

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

        let now = chrono::Utc::now();
        let mk = |id: &str, public: bool| grey_api::Incident {
            id: id.into(),
            title: format!("Incident {id}"),
            description: String::new(),
            start_time: now,
            end_time: None,
            detection_time: None,
            mitigation_time: None,
            affected_services: vec![],
            state: if public {
                grey_api::IncidentState::Offline
            } else {
                grey_api::IncidentState::Draft
            },
            updates: vec![],
            created_at: now,
            updated_at: now,
        };
        app_state.state.put_incident(&mk("vis", true)).await.unwrap();
        app_state.state.put_incident(&mk("hid", false)).await.unwrap();

        let resp = get_incidents(web::Data::new(app_state)).await.expect("Failed to get incidents");
        let body_bytes = resp.into_body().try_into_bytes().unwrap();
        let incidents: Vec<grey_api::Incident> =
            serde_json::from_str(&String::from_utf8_lossy(&body_bytes)).unwrap();
        assert_eq!(
            incidents.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
            vec!["vis"],
            "the public endpoint must hide draft incidents from unauthenticated viewers"
        );
    }

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