use serde::{Deserialize, Serialize};
use std::{fmt::Display, sync::atomic::AtomicBool};
use tokio::net::{lookup_host, TcpSocket};

use crate::{Sample, Target};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TcpTarget {
    pub host: String,
}

#[async_trait::async_trait]
impl Target for TcpTarget {
    async fn run(&self, _cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
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

        Ok(Sample::default().with("net.ip", addr.ip().to_string()))
    }
}

impl Display for TcpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TCP {}", self.host)
    }
}

#[cfg(test)]
mod tests {
    use crate::SampleValue;

    use super::*;

    #[tokio::test]
    async fn test_tcp_target() {
        let target = TcpTarget {
            host: "httpbin.org:443".to_string(),
        };

        let cancel = AtomicBool::new(false);
        let sample = target.run(&cancel).await.unwrap();

        assert!(matches!(sample.get("net.ip"), SampleValue::String(s) if !s.is_empty()));
    }
}
