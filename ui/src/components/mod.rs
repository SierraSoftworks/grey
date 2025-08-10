pub mod header;
pub mod banner;
pub mod notice;
pub mod probe;
pub mod history;
pub mod status_indicator;
pub mod page;

pub use header::Header;
pub use banner::{Banner, BannerKind};
pub use notice::Notice;
pub use probe::{Probe, ProbeData, SampleData};
pub use history::History;
pub use page::{Page, PageProps};

// Config types for the client - matching server types
use serde::{Deserialize, Serialize};

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
    pub notices: Vec<UiNotice>,
    
    #[serde(default)]
    pub links: Vec<UiLink>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiNotice {
    pub title: String,
    pub description: String,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiLink {
    pub title: String,
    pub url: String,
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
