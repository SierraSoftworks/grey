use grey_api::{UiConfig, UiNotice, Probe, ProbeHistory};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AppState {
    pub config: UiConfig,
    pub notices: Vec<UiNotice>,
    pub probes: Vec<Probe>,
    pub probe_histories: HashMap<String, Vec<ProbeHistory>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config: UiConfig::default(),
            notices: Vec::new(),
            probes: Vec::new(),
            probe_histories: HashMap::new(),
        }
    }
}

impl AppState {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[cfg(feature = "wasm")]
    pub fn from_dom() -> Option<Self> {
        use web_sys::window;

        let window = window()?;
        let document = window.document()?;
        let app_element = document.get_element_by_id("app")?;
        
        let config_data = app_element.get_attribute("data-config")?;
        let notices_data = app_element.get_attribute("data-notices")?;
        let probes_data = app_element.get_attribute("data-probes")?;
        let histories_data = app_element.get_attribute("data-probe-histories")?;

        let config: UiConfig = serde_json::from_str(&config_data).ok()?;
        let notices: Vec<UiNotice> = serde_json::from_str(&notices_data).ok()?;
        let probes: Vec<Probe> = serde_json::from_str(&probes_data).ok()?;
        let probe_histories: HashMap<String, Vec<ProbeHistory>> = serde_json::from_str(&histories_data).ok()?;

        Some(Self {
            config,
            notices,
            probes,
            probe_histories,
        })
    }
}
