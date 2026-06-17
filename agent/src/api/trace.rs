//! Request-scoped tracing middleware.
//!
//! Wraps every request in a server span (continuing any inbound W3C trace context) and echoes the
//! resulting `traceparent` back on the response. Clients can log or display that value so a user can
//! quote it when reporting a problem to an operator, making the request trivial to correlate in the
//! telemetry backend.

use std::collections::HashMap;

use actix_web::{
    Error,
    body::BoxBody,
    dev::{ServiceRequest, ServiceResponse},
    http::header::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
};
use tracing_batteries::prelude::*;

const TRACEPARENT: &str = "traceparent";

/// Reads W3C trace-context headers off an incoming request so an inbound trace can be continued.
struct HeaderExtractor<'a>(&'a HeaderMap);

impl tracing_batteries::prelude::opentelemetry::propagation::Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|name| name.as_str()).collect()
    }
}

/// Creates a server span for the request, runs the handler within it, and surfaces the request's
/// `traceparent` on the response headers.
pub async fn trace_requests(
    req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let span = info_span!(
        "http.request",
        otel.kind = "server",
        otel.name = format!("{} {}", req.method(), req.path()),
        http.request.method = %req.method(),
        url.path = %req.path(),
    );

    // Continue a trace the client may already have started.
    let parent = tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
        p.extract(&HeaderExtractor(req.headers()))
    });
    let _ = span.set_parent(parent);

    let mut response = next.call(req).instrument(span.clone()).await?;

    // Echo the trace context back so the client can record it for support requests.
    let mut carrier = HashMap::new();
    tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
        p.inject_context(&span.context(), &mut carrier)
    });

    if let Some(traceparent) = carrier.get(TRACEPARENT)
        && let Ok(value) = HeaderValue::from_str(traceparent)
    {
        response
            .headers_mut()
            .insert(HeaderName::from_static(TRACEPARENT), value);
    }

    Ok(response)
}
