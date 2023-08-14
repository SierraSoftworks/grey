use std::{collections::HashMap, str::FromStr, fmt::Display};

use opentelemetry::{trace::SpanKind, sdk::propagation::TraceContextPropagator, propagation::TextMapPropagator};
use serde::{Serialize, Deserialize};
use tracing::{field, Span};

use crate::{Target, Sample};

lazy_static! {
    static ref CLIENT_NO_VERIFY: reqwest::Client = reqwest::ClientBuilder::new()
        .user_agent(version!("SierraSoftworks/grey@v"))
        .build()
        .unwrap();

    static ref CLIENT: reqwest::Client = reqwest::ClientBuilder::new()
        .user_agent(version!("SierraSoftworks/grey@v"))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
}

fn default_get() -> String {
    "GET".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpTarget {
    pub url: String,
    #[serde(default="default_get")]
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
    #[instrument(
        "target.http",
        skip(self), err(Debug), fields(
        otel.kind=?SpanKind::Client,
        http.url = %self.url,
        http.method = %self.method,
        http.request_content_length = self.body.as_ref().map(|b| b.len()).unwrap_or(0),
        http.status_code = field::Empty,
        http.response_content_length = field::Empty,
        http.flavor = field::Empty,
    ))]
    async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>> {
        let method = reqwest::Method::from_str(&self.method)?;

        let mut request = if self.no_verify { CLIENT_NO_VERIFY.request(method, self.url.clone()) } else { CLIENT.request(method, self.url.clone()) };

        let mut headers = self.headers.clone();
        let propagator = TraceContextPropagator::new();
        propagator.inject(&mut headers);

        for (key, value) in headers.iter() {
            request = request.header(key, value);
        }

        if let Some(body) = &self.body {
            request = request.body(body.clone());
        }

        let response = request.send().await?;
        Span::current()
            .record("http.status_code", response.status().as_u16())
            .record("http.response_content_length", response.content_length().unwrap_or(0))
            .record("http.flavor", field::debug(response.version()));

        let mut sample = Sample::default()
            .with("http.status", response.status().as_u16())
            .with("http.version", format!("{:?}", response.version()));

        for (key, value) in response.headers().iter() {
            sample = sample.with(format!("http.header.{}", key.as_str().to_lowercase()), value.to_str()?.to_owned());
        }

        Ok(sample.with("http.body", response.text().await?))
    }
}

impl Display for HttpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {} {}", self.method, self.url)
    }
}