use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_ui_title")]
    pub title: String,
    #[serde(default = "default_ui_logo")]
    pub logo: String,
    pub links: Vec<UiLink>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            title: default_ui_title(),
            logo: default_ui_logo(),
            links: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiNotice {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub level: Option<NoticeLevel>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiLink {
    pub title: String,
    pub url: String,
}

fn default_ui_title() -> String {
    "Grey".into()
}

fn default_ui_logo() -> String {
    "https://cdn.sierrasoftworks.com/logos/icon.svg".into()
}
