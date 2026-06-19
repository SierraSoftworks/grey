use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_ui_title")]
    pub title: String,

    #[serde(default = "default_ui_logo")]
    pub logo: String,

    pub links: Vec<UiLink>,

    #[serde(default = "default_reload_interval")]
    pub reload_interval: std::time::Duration,

    /// Public OIDC parameters the SPA needs to start a browser-side PKCE login. `None` when admin
    /// authentication is not configured. Only public values are ever exposed here — never a client
    /// secret or the admin access-control list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<UiAuthConfig>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            title: default_ui_title(),
            logo: default_ui_logo(),
            links: Vec::new(),
            reload_interval: default_reload_interval(),
            auth: None,
        }
    }
}

/// The public OIDC configuration handed to the browser so it can run the Authorization Code + PKCE
/// flow as a public client. Deliberately carries no secret.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiAuthConfig {
    /// The OIDC issuer / provider base URL used for discovery and the PKCE flow.
    pub issuer: String,
    /// The public OAuth2 client id registered for the SPA.
    pub client_id: String,
    /// Additional scopes to request beyond the implicit `openid`.
    #[serde(default)]
    pub scopes: Vec<String>,
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

impl NoticeLevel {
    /// The lowercase token matching the serde representation, which doubles as the CSS modifier class
    /// for the notice (`ok` / `warning` / `error`).
    pub fn as_str(&self) -> &'static str {
        match self {
            NoticeLevel::Ok => "ok",
            NoticeLevel::Warning => "warning",
            NoticeLevel::Error => "error",
        }
    }
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

fn default_reload_interval() -> std::time::Duration {
    std::time::Duration::from_secs(60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_level_as_str_matches_the_serde_token() {
        // The CSS class is the serde token, so the two must stay in lockstep.
        for level in [NoticeLevel::Ok, NoticeLevel::Warning, NoticeLevel::Error] {
            let token = serde_json::to_value(&level).unwrap();
            assert_eq!(token, serde_json::Value::String(level.as_str().to_string()));
        }
        // In particular the warning class is "warning", not "warn".
        assert_eq!(NoticeLevel::Warning.as_str(), "warning");
    }
}
