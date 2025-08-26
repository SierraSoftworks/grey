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
            validators: probe
                .validators
                .iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            availability: 0.0,
        }
    }
}

impl From<&crate::history::StateBucket> for grey_api::ProbeHistory {
    fn from(bucket: &crate::history::StateBucket) -> Self {
        let state_duration = match bucket.end_time {
            Some(end_time) => std::time::Duration::from_secs(
                (end_time - bucket.start_time).num_seconds().max(1) as u64,
            ),
            None => std::time::Duration::from_secs(
                (chrono::Utc::now() - bucket.start_time)
                    .num_seconds()
                    .max(1) as u64,
            ),
        };

        grey_api::ProbeHistory {
            start_time: bucket.start_time,
            latency: std::time::Duration::from_millis(
                bucket.total_latency.num_milliseconds() as u64
                / bucket.total_samples
            ),
            state_duration,
            attempts: bucket.total_attempts,
            pass: bucket.state.pass,
            message: bucket.state.message.clone(),
            validations: bucket
                .state
                .validations
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        grey_api::ValidationResult {
                            condition: v.condition.clone(),
                            pass: v.pass,
                            message: v.message.clone(),
                        },
                    )
                })
                .collect(),
            sample_count: bucket.total_samples,
            successful_samples: bucket.successful_samples,
        }
    }
}

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
        .state.get_config()
        .ui
        .notices
        .iter()
        .map(|notice| notice.clone().into())
        .collect();
    Ok(HttpResponse::Ok().json(api_notices))
}

pub async fn get_probes(data: web::Data<AppState>) -> Result<HttpResponse> {
    let mut api_probes: Vec<grey_api::Probe> = data
        .state.get_config()
        .probes
        .iter()
        .map(|probe| probe.into())
        .collect();

    for probe in api_probes.iter_mut() {
        probe.availability = data.state.get_history(&probe.name).map(|h| h.availability()).unwrap_or(100.0);
    }

    Ok(HttpResponse::Ok().json(api_probes))
}

pub async fn get_history(
    probe: web::Path<String>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    let probe_name = probe.into_inner();

    let history = data
        .state
        .get_history(&probe_name)
        .ok_or_else(|| actix_web::error::ErrorNotFound("Probe not found"))?;

    let api_history: Vec<grey_api::ProbeHistory> = history
        .get_state_buckets()
        .iter()
        .map(|bucket| bucket.into())
        .collect();
    Ok(HttpResponse::Ok().json(api_history))
}
