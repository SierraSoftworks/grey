use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiConfig {
    pub title: String,
    pub logo: String,
    pub notices: Vec<UiNotice>,
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