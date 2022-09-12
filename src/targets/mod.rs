use serde::{Serialize, Deserialize};

use crate::Sample;

mod http;

#[async_trait::async_trait]
pub trait Target {
    async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TargetType {
    Http(http::HttpTarget),
}

#[async_trait::async_trait]
impl Target for TargetType {
    async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>> {
        match self {
            TargetType::Http(target) => target.run().await,
        }
    }
}