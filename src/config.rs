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

#[derive(Debug, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ui_listen")]
    pub listen: String,
}

fn default_ui_listen() -> String {
    "0.0.0.0:3002".to_string()
}
