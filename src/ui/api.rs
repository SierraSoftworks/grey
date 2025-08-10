use actix_web::{web, HttpResponse, Result};
use serde_json::json;
use serde::{Deserialize, Serialize};
use grey_ui::{UiConfig, ProbeData, SampleData};

use super::AppState;

#[derive(Serialize, Deserialize)]
pub struct AppData {
    pub config: UiConfig,
    pub availability: f64,
    pub probes: Vec<ProbeData>,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_update: chrono::DateTime<chrono::Utc>,
}

pub async fn get_app_data(data: web::Data<AppState>) -> Result<HttpResponse> {
    let current_time = chrono::Utc::now();

    let probes_data = data
        .probes
        .values()
        .map(|probe| {
            let samples = if let Ok(history) = probe.history.read() {
                history.iter().map(|sample| SampleData {
                    pass: sample.pass,
                    message: sample.message.clone(),
                }).collect()
            } else {
                Vec::new()
            };

            ProbeData {
                name: probe.name.clone(),
                availability: probe.availability(),
                target: format!("{}", probe.target),
                policy: format!("{}", probe.policy),
                samples,
            }
        })
        .collect::<Vec<_>>();

    let availability = if probes_data.is_empty() {
        100.0
    } else {
        probes_data.iter().map(|probe| probe.availability).sum::<f64>() / (probes_data.len() as f64)
    };

    let app_data = AppData {
        config: data.config.clone(),
        availability,
        probes: probes_data,
        last_update: current_time,
    };

    Ok(HttpResponse::Ok().json(app_data))
}

pub async fn get_probes(data: web::Data<AppState>) -> Result<HttpResponse> {
    let probes = data
        .probes
        .values()
        .map(|probe| probe.as_ref())
        .collect::<Vec<_>>();
    
    Ok(HttpResponse::Ok().json(json!(probes)))
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
            let history_vec = history.iter().cloned().collect::<Vec<_>>();
            Ok(HttpResponse::Ok().json(json!(history_vec)))
        }
        Err(_) => Err(actix_web::error::ErrorInternalServerError(
            "Failed to read history (lock is poisoned)",
        )),
    }
}
