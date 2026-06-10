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
    #[cfg(test)]
    pub fn test(temp_dir: &PathBuf) -> Self {
        Self {
            probes: vec![
                Probe::test(),
            ],
            ui: UiConfig::default(),
            cluster: ClusterConfig::default(),
            state: temp_dir.join("test_state.redb"),
        }
    }

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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: default::ui::listen(),
            title: default::ui::title(),
            logo: default::ui::logo(),
            notices: vec![],
            links: vec![],
            reload_interval: default::ui::reload_interval(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ClusterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default::cluster::listen")]
    pub listen: String,
    /// The address other nodes should use to reach this one, advertised through the membership
    /// gossip so peers can be discovered transitively. When unset it falls back to `listen` if that
    /// is a concrete (non-wildcard) address; a wildcard `listen` with no `advertised_address` means
    /// this node self-advertises nothing (it is still discovered via the source address of its
    /// packets).
    #[serde(default)]
    pub advertised_address: Option<String>,
    pub peers: Vec<String>,
    pub secret: String,
    #[serde(default)]
    pub secrets: Vec<String>,

    #[serde(default = "default::cluster::gossip_interval")]
    #[serde(with = "humantime_serde")]
    pub gossip_interval: std::time::Duration,
    #[serde(default = "default::cluster::gossip_factor")]
    pub gossip_factor: usize,

    /// The maximum size, in bytes, of a gossip datagram this node will emit; larger messages are
    /// partitioned across rounds. Accepts the former `max_message_size` name for compatibility.
    #[serde(default = "default::cluster::message_mtu")]
    #[serde(alias = "max_message_size")]
    pub message_mtu: usize,

    /// Phi-accrual suspicion threshold; a peer whose phi exceeds this is considered suspect/dead.
    #[serde(default = "default::cluster::phi_threshold")]
    pub phi_threshold: f64,
    /// How long a peer has to answer a gossip message before that send counts as a missed exchange
    /// for the link's health (driving the per-address retry backoff).
    #[serde(default = "default::cluster::reply_timeout")]
    #[serde(with = "humantime_serde")]
    pub reply_timeout: std::time::Duration,

    #[serde(default = "default::cluster::peer_resolve_interval")]
    #[serde(with = "humantime_serde")]
    pub peer_resolve_interval: std::time::Duration,

    #[serde(default = "default::cluster::gc_interval")]
    #[serde(with = "humantime_serde")]
    pub gc_interval: std::time::Duration,
    #[serde(default = "default::cluster::gc_probe_expiry")]
    #[serde(with = "humantime_serde")]
    pub gc_probe_expiry: std::time::Duration,
    #[serde(default = "default::cluster::gc_peer_expiry")]
    #[serde(with = "humantime_serde")]
    pub gc_peer_expiry: std::time::Duration,
}

impl ClusterConfig {
    /// The addresses this node advertises about itself through membership gossip: the configured
    /// `advertised_address`, falling back to `listen` when that is a concrete (non-wildcard)
    /// address. Empty when neither yields a routable address, in which case the node is still
    /// discovered via the source address of its gossip messages.
    pub fn advertised_addresses(&self) -> Vec<String> {
        self.advertised_address
            .clone()
            .or_else(|| match self.listen.parse::<std::net::SocketAddr>() {
                Ok(addr) if !addr.ip().is_unspecified() => Some(self.listen.clone()),
                _ => None,
            })
            .into_iter()
            .collect()
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: default::cluster::listen(),
            advertised_address: None,
            peers: vec![],
            secret: "".into(),
            secrets: vec![],
            gossip_interval: default::cluster::gossip_interval(),
            gossip_factor: default::cluster::gossip_factor(),
            message_mtu: default::cluster::message_mtu(),
            phi_threshold: default::cluster::phi_threshold(),
            reply_timeout: default::cluster::reply_timeout(),
            peer_resolve_interval: default::cluster::peer_resolve_interval(),
            gc_interval: default::cluster::gc_interval(),
            gc_probe_expiry: default::cluster::gc_probe_expiry(),
            gc_peer_expiry: default::cluster::gc_peer_expiry(),
        }
    }
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
        pub fn listen() -> String {
            "0.0.0.0:8888".into()
        }

        pub fn gossip_interval() -> std::time::Duration {
            std::time::Duration::from_secs(30)
        }

        pub fn gossip_factor() -> usize {
            2
        }

        pub fn message_mtu() -> usize {
            // A conservative default: small enough that a lost datagram costs little and large
            // enough to carry plenty per round. Raise it (up to ~65507) for fewer rounds on
            // reliable links, or lower it below the path MTU to avoid IP fragmentation. Over-large
            // gossip messages are partitioned across rounds regardless.
            8 * 1024
        }

        pub fn peer_resolve_interval() -> std::time::Duration {
            std::time::Duration::from_secs(60)
        }

        pub fn phi_threshold() -> f64 {
            8.0
        }

        pub fn reply_timeout() -> std::time::Duration {
            // UDP replies arrive within a network round trip; five seconds tolerates slow links
            // and processing delays without conflating latency with loss.
            std::time::Duration::from_secs(5)
        }

        pub fn gc_interval() -> std::time::Duration {
            std::time::Duration::from_secs(5 * 60)
        }

        pub fn gc_probe_expiry() -> std::time::Duration {
            std::time::Duration::from_secs(7 * 24 * 60 * 60)
        }

        pub fn gc_peer_expiry() -> std::time::Duration {
            std::time::Duration::from_secs(30 * 60)
        }
    }
}
