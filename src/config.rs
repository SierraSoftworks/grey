use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tracing::Level;
use tracing_batteries::prelude::*;

use crate::Probe;

#[derive(Clone)]
pub struct ConfigProvider {
    state_directory: Option<PathBuf>,
    probes: Arc<RwLock<Arc<Vec<Probe>>>>,
    ui: Arc<RwLock<UiConfig>>,
    last_modified: Arc<Mutex<std::time::SystemTime>>,
    config_path: Option<PathBuf>,
}

impl ConfigProvider {
    #[tracing::instrument(name = "config.load", skip(path), err(Debug))]
    pub async fn from_path<P: Into<PathBuf>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.into();
        let config = Self::load_from_path(&path).await?;
        let last_modified = Self::get_last_modified(&path)?;
        let config_path = Some(path.into());
        Ok(Self {
            config_path,
            state_directory: config.state_directory,
            probes: Arc::new(RwLock::new(Arc::new(config.probes))),
            ui: Arc::new(RwLock::new(config.ui)),
            last_modified: Arc::new(Mutex::new(last_modified)),
        })
    }

    #[tracing::instrument(name = "config.reload", level=Level::DEBUG, skip(self), err(Debug))]
    pub async fn reload(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = self
            .config_path
            .as_ref()
            .ok_or("No config path set for reloading")?;

        let last_modified = Self::get_last_modified(&config_path)?;
        let last_read = self.last_modified.lock().unwrap().clone();
        if last_read >= last_modified {
            return Ok(());
        }

        let new_config = Self::load_from_path(config_path).await?;
        *self.ui.write().unwrap() = new_config.ui;
        *self.probes.write().unwrap() = Arc::new(new_config.probes);
        *self.last_modified.lock().unwrap() = last_modified;

        Ok(())
    }

    pub fn state_dir(&self) -> Option<PathBuf> {
        self.state_directory.clone()
    }

    pub fn probes(&self) -> Arc<Vec<Probe>> {
        self.probes.read().unwrap().clone()
    }

    pub fn ui(&self) -> UiConfig {
        self.ui.read().unwrap().clone()
    }

    async fn load_from_path(path: &PathBuf) -> Result<Config, Box<dyn std::error::Error>> {
        let config = tokio::fs::read_to_string(path).await.map_err(|e| {
            error!(name: "config.load", { config.path=%path.display(), exception = %e }, "Failed to load configuration file from {}: {}", path.display(), e);
            let err: Box<dyn std::error::Error> = format!("Failed to load configuration file from {}: {}", path.display(), e).into();
            err
        })?;

        let config: Config = serde_yaml::from_str(&config)?;
        Ok(config)
    }

    fn get_last_modified(path: &PathBuf) -> Result<SystemTime, Box<dyn std::error::Error>> {
        let metadata = std::fs::metadata(path).map_err(|e| {
            error!(name: "config.load", { config.path=%path.display(), exception = %e }, "Failed to get metadata for {}: {}", path.display(), e);
            let err: Box<dyn std::error::Error> = format!("Failed to get metadata for {}: {}", path.display(), e).into();
            err
        })?;
        Ok(metadata.modified()?)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub probes: Vec<Probe>,

    pub ui: UiConfig,

    #[serde(rename = "state")]
    pub state_directory: Option<PathBuf>,
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

    #[serde(default = "default_reload_interval")]
    pub reload_interval: std::time::Duration,
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

fn default_reload_interval() -> std::time::Duration {
    std::time::Duration::from_secs(60)
}
