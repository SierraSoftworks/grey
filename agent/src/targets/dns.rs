use std::{fmt::Display, str::FromStr, sync::atomic::AtomicBool};

use hickory_resolver::{
    TokioResolver,
    config::{ConnectionConfig, GOOGLE, NameServerConfig, ResolverConfig, ResolverOpts},
    net::runtime::TokioRuntimeProvider,
    proto::{
        op::Query,
        rr::{Name, RData, Record, RecordType},
    },
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
            answer_records(
                lookup.query(),
                lookup.answers(),
                lookup.additionals(),
                lookup.authorities(),
            )
            .map(|record| record.data.to_string())
            .collect::<Vec<String>>(),
        ))
    }
}

/// Selects the records reported as `dns.answers`, reproducing the set that
/// trust-dns 0.23's `Lookup::iter()` yielded. hickory-resolver keeps the
/// response sections separate, whereas trust-dns merged the answer, additional
/// and authority sections and kept records which (a) match the query type and
/// name (or a CNAME target), (b) are intermediate CNAMEs, or (c) are A/AAAA
/// glue for SRV targets and NS queries.
fn answer_records<'a>(
    query: &'a Query,
    answers: &'a [Record],
    additionals: &'a [Record],
    authorities: &'a [Record],
) -> impl Iterator<Item = &'a Record> + 'a {
    let query_type = query.query_type();

    // The names which trust-dns treated as the lookup's "search name": the
    // queried name plus any names reached by following CNAMEs.
    let mut names: Vec<&Name> = vec![query.name()];
    names.extend(answers.iter().filter_map(|r| match &r.data {
        RData::CNAME(cname) => Some(&cname.0),
        _ => None,
    }));

    answers
        .iter()
        .chain(additionals)
        .chain(authorities)
        .filter(move |r| {
            if r.dns_class != query.query_class() {
                return false;
            }

            let record_type = r.record_type();
            let name_matches = names.contains(&&r.name);
            ((query_type.is_any() || query_type == record_type) && name_matches)
                || record_type == RecordType::CNAME
                || (query_type.is_srv() && record_type.is_ip_addr() && name_matches)
                || (query_type.is_ns() && record_type.is_ip_addr())
        })
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
mod answer_tests {
    use std::net::Ipv4Addr;

    use hickory_resolver::proto::rr::rdata::{A, NS, TXT};

    use super::*;

    fn name(s: &str) -> Name {
        Name::from_str(s).unwrap()
    }

    fn select(query: &Query, answers: &[Record], additionals: &[Record]) -> Vec<String> {
        answer_records(query, answers, additionals, &[])
            .map(|r| r.data.to_string())
            .collect()
    }

    #[test]
    fn ns_lookup_includes_glue_records() {
        let query = Query::query(name("example.com."), RecordType::NS);
        let answers = [Record::from_rdata(
            name("example.com."),
            300,
            RData::NS(NS(name("ns1.example.com."))),
        )];
        let additionals = [Record::from_rdata(
            name("ns1.example.com."),
            300,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        )];

        assert_eq!(
            select(&query, &answers, &additionals),
            vec!["ns1.example.com.".to_string(), "192.0.2.1".to_string()]
        );
    }

    #[test]
    fn unrelated_additional_records_are_excluded() {
        let query = Query::query(name("example.com."), RecordType::A);
        let answers = [Record::from_rdata(
            name("example.com."),
            300,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        )];
        let additionals = [
            Record::from_rdata(
                name("other.example.com."),
                300,
                RData::A(A(Ipv4Addr::new(192, 0, 2, 2))),
            ),
            Record::from_rdata(
                name("example.com."),
                300,
                RData::TXT(TXT::new(vec!["hello".to_string()])),
            ),
        ];

        assert_eq!(
            select(&query, &answers, &additionals),
            vec!["192.0.2.1".to_string()]
        );
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
