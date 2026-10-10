use std::{fmt::Display, str::FromStr, sync::atomic::AtomicBool};

use hickory_resolver::{
    TokioResolver,
    config::{ConnectionConfig, GOOGLE, NameServerConfig, ResolverConfig, ResolverOpts},
    net::runtime::TokioRuntimeProvider,
    proto::rr::RecordType,
};
use serde::{Deserialize, Serialize};

use crate::{Sample, Target};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DnsTarget {
    pub domain: String,
    pub record_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nameservers: Option<Vec<String>>,
}

impl Target for DnsTarget {
    async fn run(&self, _cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        let resolver_config = self.resolver_config()?;
        let lookup =
            TokioResolver::builder_with_config(resolver_config, TokioRuntimeProvider::default())
                .with_options(ResolverOpts::default())
                .build()?
                .lookup(
                    self.domain.as_str(),
                    RecordType::from_str(self.record_type.as_deref().unwrap_or("A"))?,
                )
                .await?;

        Ok(Sample::default().with(
            "dns.answers",
            lookup
                .answers()
                .iter()
                .map(|record| record.data.to_string())
                .collect::<Vec<String>>(),
        ))
    }
}

impl Display for DnsTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DNS {} {}",
            self.record_type.as_deref().unwrap_or("A"),
            self.domain
        )
    }
}

impl DnsTarget {
    fn resolver_config(&self) -> Result<ResolverConfig, Box<dyn std::error::Error>> {
        if let Some(nameservers) = &self.nameservers {
            let mut config = ResolverConfig::from_name_servers(vec![]);
            for ns in nameservers {
                let ns = match core::net::SocketAddr::from_str(&ns) {
                    Ok(addr) => Ok(addr),
                    Err(_) => format!("{ns}:53").parse(),
                }
                .map_err(|e| format!("Invalid nameserver address '{}': {}", ns, e))?;

                let mut connection = ConnectionConfig::udp();
                connection.port = ns.port();
                config.add_name_server(NameServerConfig::new(ns.ip(), true, vec![connection]));
            }
            Ok(config)
        } else {
            // Matches the previous trust-dns `ResolverConfig::default()`, which
            // used Google Public DNS over UDP and TCP.
            Ok(ResolverConfig::udp_and_tcp(&GOOGLE))
        }
    }
}

#[cfg(test)]
#[cfg(not(feature = "pure_tests"))]
mod tests {
    use crate::sample::SampleValue;

    use super::*;

    #[tokio::test]
    async fn test_a() {
        let target = DnsTarget {
            domain: "google.com".to_string(),
            record_type: None,
            nameservers: None,
        };
        let cancel = AtomicBool::new(false);
        let sample = target.run(&cancel).await.unwrap();
        assert!(matches!(sample.get("dns.answers"), &SampleValue::List(_)));
    }

    #[tokio::test]
    async fn test_mx() {
        let target = DnsTarget {
            domain: "google.com".to_string(),
            record_type: Some("MX".to_string()),
            nameservers: None,
        };
        let cancel = AtomicBool::new(false);
        let sample = target.run(&cancel).await.unwrap();
        assert_eq!(
            sample.get("dns.answers"),
            &SampleValue::List(vec![SampleValue::String("10 smtp.google.com.".into()),])
        );
    }

    #[tokio::test]
    async fn test_nameservers() {
        let target = DnsTarget {
            domain: "google.com".to_string(),
            record_type: None,
            nameservers: Some(vec!["8.8.8.8:53".to_string(), "8.8.4.4:53".to_string()]),
        };
        let cancel = AtomicBool::new(false);
        let sample = target.run(&cancel).await.unwrap();
        assert!(matches!(sample.get("dns.answers"), &SampleValue::List(_)));
    }
}
