use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing_batteries::prelude::*;

use crate::Probe;

#[tracing::instrument(skip(path), err(Debug))]
pub async fn load_config<P: Into<PathBuf>>(path: P) -> Result<Config, Box<dyn std::error::Error>> {
    let path = path.into();
    let config = tokio::fs::read_to_string(path).await?;
    let config: Config = serde_yaml::from_str(&config)?;
    Ok(config)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub probes: Vec<Probe>,

    pub ui: UiConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ui_listen")]
    pub listen: String,

    #[serde(default = "default_ui_title")]
    pub title: String,
    #[serde(default = "default_ui_logo")]
    pub logo: String,

    #[serde(default)]
    pub notices: Vec<UiNotice>,
}

fn default_ui_listen() -> String {
    "0.0.0.0:3002".to_string()
}

fn default_ui_title() -> String {
    "Grey Service Uptime".to_string()
}

fn default_ui_logo() -> String {
    "https://cdn.sierrasoftworks.com/logos/icon.svg".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UiNotice {
    pub title: String,
    pub description: String,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}
