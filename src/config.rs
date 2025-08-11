use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tracing_batteries::prelude::*;

use crate::Probe;

#[derive(Clone)]
pub struct ConfigProvider {
    pub probes: Vec<Arc<Probe>>,
    pub ui: Arc<RwLock<UiConfig>>,
    pub config_path: Option<PathBuf>,
}

impl ConfigProvider {
    #[tracing::instrument(name = "config.load", skip(path), err(Debug))]
    pub async fn from_path<P: Into<PathBuf>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.into();
        let config = Self::load_from_path(&path).await?;
        let config_path = Some(path.into());
        Ok(Self {
            config_path,
            probes: config.probes.into_iter().map(Arc::new).collect(),
            ui: Arc::new(RwLock::new(config.ui)),
        })
    }

    #[tracing::instrument(name = "config.reload", skip(self), err(Debug))]
    pub async fn reload(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = self
            .config_path
            .as_ref()
            .ok_or("No config path set for reloading")?;

        let new_config = Self::load_from_path(config_path).await?;
        let ui = self.ui.clone();
        if let Err(err) = ui.write().map(move |mut config| {
            *config = new_config.ui;
        }) {
            return Err(format!("Failed to acquire lock for UI config entry: {}", err).into());
        }
        Ok(())
    }

    pub fn probes(&self) -> Vec<Arc<Probe>> {
        self.probes.clone()
    }

    pub fn ui(&self) -> UiConfig {
        self.ui.read().unwrap().clone()
    }

    async fn load_from_path(path: &PathBuf) -> Result<Config, Box<dyn std::error::Error>> {
        let config = tokio::fs::read_to_string(path).await?;
        let config: Config = serde_yaml::from_str(&config)?;
        Ok(config)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub probes: Vec<Probe>,

    pub ui: UiConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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
    pub notices: Vec<grey_api::UiNotice>,

    #[serde(default)]
    pub links: Vec<grey_api::UiLink>,
}

fn default_ui_listen() -> String {
    "0.0.0.0:8888".into()
}

fn default_ui_title() -> String {
    "Grey".into()
}

fn default_ui_logo() -> String {
    "https://cdn.sierrasoftworks.com/logos/icon.svg".into()
}
