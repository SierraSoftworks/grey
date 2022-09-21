use std::fmt::Display;

use serde::{Serialize, Deserialize};

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
    Dns(dns::DnsTarget),
    Http(http::HttpTarget),
    Tcp(tcp::TcpTarget),
}

impl Display for TargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
            TargetType::Dns(target) => target.run().await,
            TargetType::Http(target) => target.run().await,
            TargetType::Tcp(target) => target.run().await,
        }
    }
}