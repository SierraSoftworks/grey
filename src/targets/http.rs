use std::{collections::HashMap, fmt::Display, str::FromStr, sync::atomic::AtomicBool};

use serde::{Deserialize, Serialize};
use tracing_batteries::prelude::opentelemetry::trace::SpanKind as OpenTelemetrySpanKind;
use tracing_batteries::prelude::*;

use crate::{Sample, Target};

lazy_static! {
    static ref CLIENT_NO_VERIFY: reqwest::Client = reqwest::ClientBuilder::new()
        .user_agent(version!("SierraSoftworks/grey@v"))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    static ref CLIENT: reqwest::Client = reqwest::ClientBuilder::new()
        .user_agent(version!("SierraSoftworks/grey@v"))
        .build()
        .unwrap();
}

fn default_get() -> String {
    "GET".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpTarget {
    pub url: String,
    #[serde(default = "default_get")]
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub no_verify: bool,
}

#[async_trait::async_trait]
impl Target for HttpTarget {
    #[tracing::instrument(
        "target.http",
        skip(self, _cancel), err(Debug),
        fields(
            otel.kind=?OpenTelemetrySpanKind::Client,
            http.url = %self.url,
            http.method = %self.method,
            http.request_content_length = self.body.as_ref().map(|b| b.len()).unwrap_or(0),
            http.status_code = EmptyField,
            http.response_content_length = EmptyField,
            http.flavor = EmptyField,
            cert.no_verify = %self.no_verify,
    ))]
    async fn run(&self, _cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        let method = reqwest::Method::from_str(&self.method)?;

        let mut request = if self.no_verify {
            CLIENT_NO_VERIFY.request(method, self.url.clone())
        } else {
            CLIENT.request(method, self.url.clone())
        };

        let mut headers = self.headers.clone();
        opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&Span::current().context(), &mut headers)
        });

        for (key, value) in headers.iter() {
            request = request.header(key, value);
        }

        if let Some(body) = &self.body {
            request = request.body(body.clone());
        }

        let response = request.send().await?;
        Span::current()
            .record("http.status_code", response.status().as_u16())
            .record(
                "http.response_content_length",
                response.content_length().unwrap_or(0),
            )
            .record("http.flavor", debug(response.version()));

        let mut sample = Sample::default()
            .with("http.status", response.status().as_u16())
            .with("http.version", format!("{:?}", response.version()));

        for (key, value) in response.headers().iter() {
            sample = sample.with(
                format!("http.header.{}", key.as_str().to_lowercase()),
                value.to_str()?.to_owned(),
            );
        }

        Ok(sample.with("http.body", response.text().await?))
    }
}

impl Display for HttpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {} {}", self.method, self.url)
    }
}

#[cfg(test)]
mod tests {
    use crate::SampleValue;

    use super::*;

    #[tokio::test]
    async fn test_get() {
        let target = HttpTarget {
            url: "https://httpbin.org/get".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            no_verify: true,
        };

        let cancel = AtomicBool::new(false);

        let sample = target.run(&cancel).await.unwrap();
        assert_eq!(sample.get("http.status"), &200.into());
        assert_eq!(sample.get("http.version"), &"HTTP/1.1".into());
        assert!(matches!(sample.get("http.body"), SampleValue::String(s) if !s.is_empty()));
    }
}
