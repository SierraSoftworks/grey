use std::{str::FromStr, fmt::Display};

use serde::{Deserialize, Serialize};
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    TokioAsyncResolver, proto::rr::RecordType,
};

use crate::{Sample, Target};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsTarget {
    pub domain: String,
    pub record_type: Option<String>,
}

#[async_trait::async_trait]
impl Target for DnsTarget {
    async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>> {
        let lookup = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default())
            .lookup(self.domain.as_str(), RecordType::from_str(self.record_type.as_deref().unwrap_or("A"))?)
            .await?;

        Ok(Sample::default().with(
            "dns.answers",
            lookup
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<String>>(),
        ))
    }
}

impl Display for DnsTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DNS {} {}", self.record_type.as_deref().unwrap_or("A"), self.domain)
    }
}

#[cfg(test)]
mod tests {
    use crate::sample::SampleValue;

    use super::*;

    #[tokio::test]
    async fn test_a() {
        let target = DnsTarget {
            domain: "google.com".to_string(),
            record_type: None,
        };
        let sample = target.run().await.unwrap();
        assert!(matches!(sample.get("dns.answers"), &SampleValue::List(_)));
    }

    #[tokio::test]
    async fn test_mx() {
        let target = DnsTarget {
            domain: "google.com".to_string(),
            record_type: Some("MX".to_string()),
        };
        let sample = target.run().await.unwrap();
        assert_eq!(sample.get("dns.answers"), &SampleValue::List(vec![
            SampleValue::String("10 smtp.google.com.".into()),
        ]));
    }
}
