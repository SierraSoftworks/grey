use std::{fmt::Display, sync::atomic::AtomicBool};

use serde::{Deserialize, Serialize};

use crate::Sample;

mod dns;
mod http;
mod script;
mod tcp;

#[async_trait::async_trait]
pub trait Target: Display {
    async fn run(&self, cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TargetType {
    #[cfg(test)]
    Mock,
    Dns(dns::DnsTarget),
    Http(http::HttpTarget),
    #[cfg(feature = "scripts")]
    Script(script::ScriptTarget),
    Tcp(tcp::TcpTarget),
}

impl TargetType {
    pub async fn run(&self, cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        match self {
            #[cfg(test)]
            TargetType::Mock => Ok(Sample::default()),
            TargetType::Dns(target) => target.run(cancel).await,
            TargetType::Http(target) => target.run(cancel).await,
            #[cfg(feature = "scripts")]
            TargetType::Script(target) => target.run(cancel).await,
            TargetType::Tcp(target) => target.run(cancel).await,
        }
    }
}

impl Display for TargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(test)]
            TargetType::Mock => write!(f, "Mock"),
            TargetType::Dns(target) => write!(f, "{}", target),
            TargetType::Http(target) => write!(f, "{}", target),
            #[cfg(feature = "scripts")]
            TargetType::Script(target) => write!(f, "{}", target),
            TargetType::Tcp(target) => write!(f, "{}", target),
        }
    }
}

#[async_trait::async_trait]
impl Target for TargetType {
    async fn run(&self, cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        match self {
            #[cfg(test)]
            TargetType::Mock => Ok(Sample::default()),
            TargetType::Dns(target) => target.run(cancel).await,
            TargetType::Http(target) => target.run(cancel).await,
            #[cfg(feature = "scripts")]
            TargetType::Script(target) => target.run(cancel).await,
            TargetType::Tcp(target) => target.run(cancel).await,
        }
    }
}
