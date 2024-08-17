use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::Sample;

mod dns;
mod http;
mod tcp;

#[async_trait::async_trait]
pub trait Target: Display {
    async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TargetType {
    #[cfg(test)]
    Mock,
    Dns(dns::DnsTarget),
    Http(http::HttpTarget),
    Tcp(tcp::TcpTarget),
}

impl Display for TargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(test)]
            TargetType::Mock => write!(f, "Mock"),
            TargetType::Dns(target) => write!(f, "{}", target),
            TargetType::Http(target) => write!(f, "{}", target),
            TargetType::Tcp(target) => write!(f, "{}", target),
        }
    }
}

#[async_trait::async_trait]
impl Target for TargetType {
    async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>> {
        match self {
            #[cfg(test)]
            TargetType::Mock => Ok(Sample::default()),
            TargetType::Dns(target) => target.run().await,
            TargetType::Http(target) => target.run().await,
            TargetType::Tcp(target) => target.run().await,
        }
    }
}
