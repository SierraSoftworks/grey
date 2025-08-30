use actix_web::{HttpResponse, Result, web};

use super::AppState;

impl From<&crate::config::UiConfig> for grey_api::UiConfig {
    fn from(config: &crate::config::UiConfig) -> Self {
        grey_api::UiConfig {
            title: config.title.clone(),
            logo: config.logo.clone(),
            links: config.links.clone(),
            reload_interval: config.reload_interval,
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