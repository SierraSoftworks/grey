use std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex, RwLock}};

use crate::{history::History, Config};

#[derive(Clone)]
pub struct State {
    config_path: PathBuf,
    config_last_modified: Arc<Mutex<std::time::SystemTime>>,

    config: Arc<RwLock<Arc<Config>>>,

    history: Arc<RwLock<HashMap<String, Arc<History>>>>,
}

impl State {
    pub async fn new<P: Into<PathBuf>>(config_path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = config_path.into();
        let config = Config::load_from_path(&config_path).await?;

        Ok(Self {
            config_path,
            config_last_modified: Arc::new(Mutex::new(std::time::SystemTime::now())),

            config: Arc::new(RwLock::new(Arc::new(config))),

            history: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn reload(&self) -> Result<(), Box<dyn std::error::Error>> {
        let last_modified = *self.config_last_modified.lock().unwrap();
        if let Some((config, modified)) = Config::load_if_modified_since(&self.config_path, last_modified).await? {
            *self.config.write().unwrap() = Arc::new(config);
            *self.config_last_modified.lock().unwrap() = modified;
        }

        Ok(())
    }

    pub fn get_config(&self) -> Arc<Config> {
        self.config.read().unwrap().clone()
    }

    pub fn get_history(&self, probe_name: &str) -> Option<Arc<History>> {
        self.history.read().unwrap().get(probe_name).cloned()
    }

    pub fn with_default_history<N: ToString>(&self, probe_name: N, history: Arc<History>) -> Arc<History> {
        self.history.write().unwrap().entry(probe_name.to_string()).or_insert_with(|| history.clone()).clone()
    }
}