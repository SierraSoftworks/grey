use std::{fmt::Display, sync::atomic::AtomicBool};

use serde::{Deserialize, Serialize};
use tonic::{
    transport::{Certificate, Channel},
    Request,
};
use tonic_health::pb::health_client::HealthClient;
use tonic_health::pb::HealthCheckRequest;
use tracing_batteries::prelude::opentelemetry::trace::SpanKind as OpenTelemetrySpanKind;
use tracing_batteries::prelude::*;

use crate::{Sample, Target};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GrpcTarget {
    pub url: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub ca_cert: Option<String>,
}

impl Target for GrpcTarget {
    #[tracing::instrument(
        "target.grpc",
        skip(self, _cancel), err(Debug),
        fields(
            otel.kind=?OpenTelemetrySpanKind::Client,
            grpc.url = %self.url,
            grpc.service = %self.service,
            grpc.status = EmptyField,
            grpc.method = "/grpc.health.v1.Health/Check",
    ))]
    async fn run(&self, _cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        let mut tls_config = tonic::transport::ClientTlsConfig::default()
            .with_native_roots()
            .with_enabled_roots();

        if let Some(cert) = &self.ca_cert {
            let cert = Certificate::from_pem(cert);
            tls_config = tls_config.ca_certificate(cert);
        }

        let endpoint = Channel::from_shared(self.url.clone())?
            .tls_config(tls_config)?
            .user_agent(format!(
                "SierraSoftworks/grey@{}",
                env!("CARGO_PKG_VERSION")
            ))?;

        let channel = endpoint.connect().await?;
        let mut client = HealthClient::new(channel);

        let request = Request::new(HealthCheckRequest {
            service: self.service.clone(),
        });

        let response = client.check(request).await?;
        let health_response = response.into_inner();

        Span::current().record("grpc.status", health_response.status().as_str_name());

        let sample = Sample::default()
            .with("grpc.status", health_response.status().as_str_name())
            .with("grpc.status_code", health_response.status);

        Ok(sample)
    }
}

impl Display for GrpcTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.service.is_empty() {
            write!(f, "gRPC {}", self.url)
        } else {
            write!(f, "gRPC {} ({})", self.url, self.service)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_target_display() {
        let target = GrpcTarget {
            url: "https://localhost:50051".to_string(),
            service: "".to_string(),
            ca_cert: None,
        };
        assert_eq!(target.to_string(), "gRPC https://localhost:50051");

        let target_with_service = GrpcTarget {
            url: "https://localhost:50051".to_string(),
            service: "myservice".to_string(),
            ca_cert: None,
        };
        assert_eq!(
            target_with_service.to_string(),
            "gRPC https://localhost:50051 (myservice)"
        );
    }
}
