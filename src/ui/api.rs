use actix_web::{web, HttpResponse, Result};
use grey_api;

use super::AppState;

// Implement conversions from src types to API types
impl From<&crate::Probe> for grey_api::Probe {
    fn from(probe: &crate::Probe) -> Self {
        grey_api::Probe {
            name: probe.name.clone(),
            policy: grey_api::Policy {
                interval: std::time::Duration::from_millis(probe.policy.interval.as_millis() as u64),
                retries: probe.policy.retries,
                timeout: std::time::Duration::from_millis(probe.policy.timeout.as_millis() as u64),
            },
            target: format!("{}", &probe.target),
            tags: probe.tags.clone(),
            validators: probe.validators.iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            availability: probe.availability(),
        }
    }
}

impl From<&crate::result::ProbeResult> for grey_api::ProbeResult {
    fn from(result: &crate::result::ProbeResult) -> Self {
        grey_api::ProbeResult {
            start_time: result.start_time,
            duration: std::time::Duration::from_millis(result.duration.num_milliseconds() as u64),
            attempts: result.attempts,
            pass: result.pass,
            message: result.message.clone(),
            validations: result.validations.iter()
                .map(|(k, v)| (k.clone(), grey_api::ValidationResult {
                    condition: v.condition.clone(),
                    pass: v.pass,
                    message: v.message.clone(),
                }))
                .collect(),
        }
    }
}

impl From<&crate::config::UiConfig> for grey_api::UiConfig {
    fn from(config: &crate::config::UiConfig) -> Self {
        grey_api::UiConfig {
            title: config.title.clone(),
            logo: config.logo.clone(),
            links: config.links.clone(),
        }
    }
}

pub async fn get_ui_config(data: web::Data<AppState>) -> Result<HttpResponse> {
    let api_config: grey_api::UiConfig = (&data.ui_config).into();
    Ok(HttpResponse::Ok().json(api_config))
}

pub async fn get_notices(data: web::Data<AppState>) -> Result<HttpResponse> {
    let api_notices: Vec<grey_api::UiNotice> = data.ui_config.notices
        .iter()
        .map(|notice| notice.clone().into())
        .collect();
    Ok(HttpResponse::Ok().json(api_notices))
}

pub async fn get_probes(data: web::Data<AppState>) -> Result<HttpResponse> {
    let api_probes: Vec<grey_api::Probe> = data
        .probes
        .values()
        .map(|probe| probe.as_ref().into())
        .collect();
    
    Ok(HttpResponse::Ok().json(api_probes))
}

pub async fn get_history(
    path: web::Path<String>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    let probe_name = path.into_inner();
    
    let probe = data
        .probes
        .get(&probe_name)
        .ok_or_else(|| actix_web::error::ErrorNotFound("Probe not found"))?;

    match probe.history.read() {
        Ok(history) => {
            let api_history: Vec<grey_api::ProbeResult> = history
                .iter()
                .map(|result| result.into())
                .collect();
            Ok(HttpResponse::Ok().json(api_history))
        }
        Err(_) => Err(actix_web::error::ErrorInternalServerError(
            "Failed to read history (lock is poisoned)",
        )),
    }
}
