use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tracing::Level;
use tracing_batteries::prelude::*;

use crate::Probe;
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub probes: Vec<Probe>,

    #[serde(default)]
    pub ui: UiConfig,

    #[serde(default)]
    pub cluster: ClusterConfig,

    #[serde(rename = "state")]
    #[serde(default = "default::state")]
    pub state: PathBuf,
}

impl Config {
    #[tracing::instrument(name = "config.load", skip(path), err(Debug))]
    pub async fn load_from_path(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config = tokio::fs::read_to_string(path).await.map_err(|e| {
            error!(name: "config.load", { config.path=%path.display(), exception = %e }, "Failed to load configuration file from {}: {}", path.display(), e);
            let err: Box<dyn std::error::Error> = format!("Failed to load configuration file from {}: {}", path.display(), e).into();
            err
        })?;

        let config: Self = serde_yaml::from_str(&config)?;
        Ok(config)
    }

    #[tracing::instrument(name = "config.reload", level=Level::DEBUG, skip(path), err(Debug))]
    pub async fn load_if_modified_since(
        path: &Path,
        last_modified: SystemTime,
    ) -> Result<Option<(Config, SystemTime)>, Box<dyn std::error::Error>> {
        let metadata = tokio::fs::metadata(path).await.map_err(|e| {
            error!(name: "config.reload", { config.path=%path.display(), exception = %e }, "Failed to get metadata for {}: {}", path.display(), e);
            let err: Box<dyn std::error::Error> = format!("Failed to get metadata for {}: {}", path.display(), e).into();
            err
        })?;

        let modified = metadata.modified()?;
        if modified > last_modified {
            let config = Self::load_from_path(path).await?;
            Ok(Some((config, modified)))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default::ui::listen")]
    pub listen: String,

    #[serde(default = "default::ui::title")]
    pub title: String,
    #[serde(default = "default::ui::logo")]
    pub logo: String,

    #[serde(default)]
    pub notices: Vec<grey_api::UiNotice>,

    #[serde(default)]
    pub links: Vec<grey_api::UiLink>,

    #[serde(default = "default::ui::reload_interval")]
    #[serde(with = "humantime_serde")]
    pub reload_interval: std::time::Duration,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
pub struct ClusterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default::cluster::hostname")]
    pub hostname: String,
    #[serde(default = "default::cluster::listen")]
    pub listen: String,
    #[serde(default = "default::cluster::gossip_interval")]
    #[serde(with = "humantime_serde")]
    pub gossip_interval: std::time::Duration,
    #[serde(default = "default::cluster::gossip_factor")]
    pub gossip_factor: usize,
    pub peers: Vec<String>,
    pub secret: String,
}

mod default {
    use super::*;

    pub fn state() -> PathBuf {
        PathBuf::from("state.redb")
    }

    pub mod ui {
        pub fn listen() -> String {
            "0.0.0.0:8888".into()
        }

        pub fn title() -> String {
            "Grey".into()
        }

        pub fn logo() -> String {
            "https://cdn.sierrasoftworks.com/logos/icon.svg".into()
        }

        pub fn reload_interval() -> std::time::Duration {
            std::time::Duration::from_secs(60)
        }
    }

    pub mod cluster {
        pub fn hostname() -> String {
            gethostname::gethostname().to_string_lossy().to_string()
        }

        pub fn listen() -> String {
            "0.0.0.0:8888".into()
        }

        pub fn gossip_interval() -> std::time::Duration {
            std::time::Duration::from_secs(30)
        }

        pub fn gossip_factor() -> usize {
            2
        }
    }
}
