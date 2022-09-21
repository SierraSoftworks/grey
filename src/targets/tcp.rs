use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tokio::net::{lookup_host, TcpSocket};

use crate::{Sample, Target};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpTarget {
    pub host: String,
}

#[async_trait::async_trait]
impl Target for TcpTarget {
    async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>> {
        let addr = lookup_host(&self.host)
            .await?
            .next()
            .ok_or(format!("Could not resolve the hostname '{}'.", &self.host))?;

        let sock = if addr.is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };

        let _stream = sock.connect(addr).await?;

        Ok(Sample::default()
            .with("net.ip", addr.ip().to_string()))
    }
}

impl Display for TcpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TCP {}", self.host)
    }
}