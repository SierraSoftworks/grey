//! Webhook notifications: a background task that watches the cluster-pooled probe and cron state for
//! transitions and delivers a signed JSON [`grey_api::WebhookEvent`] to every configured endpoint
//! whose filter matches.
//!
//! Detection is poll-based rather than event-driven, because some transitions are driven purely by
//! the passage of time rather than by a sample or a check-in: a probe recovers once no failure has
//! been observed for the recovery window, and a cron reads as *missing* once its next-due time plus
//! grace elapses. Re-deriving the displayed state on a fixed cadence (the same derivation the UI
//! renders) captures both event- and time-driven transitions with one mechanism.
//!
//! Transitions are read from the cluster-converged [`grey_api::Streak`] (probes) and the derived cron
//! health, both of which already fold in every observer's reports and the recovery settling window —
//! so an event represents the *cluster's* view of an entity, not a single node's observation, and the
//! published payload mirrors the probe/cron API shape (it carries no node identity). The emitted
//! snapshot includes the observations reported by every observer.
//!
//! The last-seen status is tracked in memory. On startup the first pass seeds the baseline silently,
//! so a restart never replays the state every entity is already in — only genuine transitions
//! observed thereafter are notified. Because the converged state is identical on every node,
//! operators typically configure webhooks on a single node; the `Grey-Webhook-Delivery` header still
//! lets a consumer de-duplicate if the same webhook is configured on several nodes.

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use filt_rs::{Filter, FilterValue, Filterable};
use grey_api::{Cron, Probe, WebhookEvent};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing_batteries::prelude::*;

use crate::config::WebhookConfig;
use crate::state::{CronStore, ProbeStore, State};

/// How often the notifier re-derives entity state to look for transitions. Kept short enough that a
/// state change is reported promptly, but long enough to avoid hammering the store.
const EVALUATION_INTERVAL: Duration = Duration::from_secs(15);

/// The signature header, in the Tailscale `t=<unix-seconds>,v1=<hex>` form (see [`WebhookConfig`]).
/// The signed timestamp travels in the `t=` field, so no separate timestamp header is needed.
const SIGNATURE_HEADER: &str = "Grey-Webhook-Signature";
/// The event's unique id, for downstream de-duplication of fan-out / retried deliveries.
const DELIVERY_HEADER: &str = "Grey-Webhook-Delivery";
/// The event kind (`probe.state_changed` / `cron.state_changed`).
const EVENT_HEADER: &str = "Grey-Webhook-Event";

/// The last-observed status of an entity, tracked to detect transitions between polls.
#[derive(Clone)]
struct Status {
    /// The derived status token (a probe is `passing`/`failing`; a cron is a `CronHealth` token).
    token: String,
    /// Whether that token reads as healthy, carried so an emitted event can report `was_healthy`.
    healthy: bool,
}

/// Watches pooled probe/cron state and dispatches webhook notifications on transitions.
pub struct Notifier {
    state: State,
    http: reqwest::Client,
    /// The last-seen status of each entity, keyed by a `"<type>:<name>"` discriminator.
    last: HashMap<String, Status>,
}

impl Notifier {
    pub fn new(state: State) -> Self {
        Self {
            state,
            http: reqwest::Client::new(),
            last: HashMap::new(),
        }
    }

    /// Runs the evaluation loop forever. The first pass runs immediately to seed the baseline (so
    /// startup doesn't replay existing state), then re-evaluates on [`EVALUATION_INTERVAL`].
    pub async fn run(mut self) {
        loop {
            if let Err(e) = self.evaluate().await {
                warn!(name: "webhook.evaluate", { exception = %e }, "Failed to evaluate notification state.");
            }
            tokio::time::sleep(EVALUATION_INTERVAL).await;
        }
    }

    /// Performs one evaluation pass: re-derives the pooled state, records transitions against the
    /// baseline, and delivers an event to every matching webhook. The baseline is always refreshed
    /// (even with no webhooks configured) so that adding a webhook via a config reload doesn't replay
    /// the state every entity is already in.
    async fn evaluate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let config = self.state.get_config();
        let now = Utc::now();

        let probes = self.state.get_probe_states().await?;
        let crons = self.state.get_cron_states().await?;

        let events = detect_transitions(
            &mut self.last,
            now,
            &probes,
            &crons,
            !config.webhooks.is_empty(),
        );

        if !events.is_empty() {
            self.dispatch(&config.webhooks, &events).await;
        }

        Ok(())
    }

    /// Delivers each event to every webhook whose filter matches it. The JSON body is serialized once
    /// per event and the matching deliveries are dispatched concurrently.
    ///
    /// Instrumented so a batch of deliveries shares a parent span; each individual delivery (and any
    /// failure) is a child [`deliver`] span beneath it.
    #[tracing::instrument(
        name = "webhook.dispatch",
        skip_all,
        fields(otel.kind = "internal", events = events.len())
    )]
    async fn dispatch(&self, webhooks: &[WebhookConfig], events: &[WebhookEvent]) {
        let mut sends = Vec::new();
        for event in events {
            let body = match serde_json::to_vec(event) {
                Ok(body) => body,
                Err(e) => {
                    warn!(name: "webhook.encode", { event.id = event.id, exception = %e }, "Failed to encode a webhook event; skipping it.");
                    continue;
                }
            };

            for webhook in webhooks {
                match event_matches(&webhook.filter, event) {
                    Ok(true) => sends.push(deliver(&self.http, webhook, event, body.clone())),
                    Ok(false) => {
                        trace!(name: "webhook.filtered", { webhook = webhook.label(), event.id = event.id, entity = event.entity.name }, "A webhook filter excluded this event.");
                    }
                    Err(e) => {
                        warn!(name: "webhook.filter", { webhook = webhook.label(), event.id = event.id, exception = %e }, "Failed to evaluate a webhook filter; skipping this delivery.");
                    }
                }
            }
        }

        // Each delivery surfaces its own outcome on its span (success status, or a span error via
        // `deliver`'s `err`), so the collected results are intentionally discarded here.
        let _ = futures::future::join_all(sends).await;
    }
}

/// Records the current status of every probe and cron against `last`, returning a
/// [`WebhookEvent`] for each entity whose status token changed since the previous pass.
///
/// `last` is always updated to the current status (so it tracks the pooled view continuously); events
/// are only produced when `notify` is set, when the entity already had a recorded baseline, and when
/// its token actually changed — so the first time an entity is seen it is seeded silently.
fn detect_transitions(
    last: &mut HashMap<String, Status>,
    now: DateTime<Utc>,
    probes: &HashMap<String, Probe>,
    crons: &HashMap<String, Cron>,
    notify: bool,
) -> Vec<WebhookEvent> {
    let mut events = Vec::new();

    for (name, probe) in probes {
        let key = format!("probe:{name}");
        // The token is derived from the cluster-converged streak (recovery settling included).
        let token = probe.status_token();
        let healthy = probe.passing();

        if notify
            && let Some(previous) = last.get(&key)
            && previous.token != token
        {
            events.push(WebhookEvent::for_probe(
                new_id(),
                now,
                probe,
                previous.token.clone(),
                previous.healthy,
            ));
        }

        last.insert(key, Status { token: token.to_string(), healthy });
    }

    for (name, cron) in crons {
        let key = format!("cron:{name}");
        let health = cron.health(now);
        let token = health.as_str();
        let healthy = health.passing();

        if notify
            && let Some(previous) = last.get(&key)
            && previous.token != token
        {
            events.push(WebhookEvent::for_cron(
                new_id(),
                now,
                cron,
                now,
                previous.token.clone(),
                previous.healthy,
            ));
        }

        last.insert(key, Status { token: token.to_string(), healthy });
    }

    events
}

/// A fresh, unique event identifier.
fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Whether `filter` matches `event`, evaluating the expression against the event's exposed fields.
///
/// `filt-rs` already returns a [`human_errors::Error`] (carrying actionable advice); we wrap it with
/// the offending expression and pointers to the documented fields so an operator can fix a bad filter
/// in their configuration without reading the source.
fn event_matches(filter: &Filter, event: &WebhookEvent) -> Result<bool, filt_rs::Error> {
    filter.matches(&WebhookEventFilter(event)).map_err(|e| {
        human_errors::wrap_user(
            e,
            format!("Failed to evaluate the webhook filter '{}'.", filter.raw()),
            &[
                "Check that the filter expression in your webhook configuration is valid filt-rs syntax.",
                "Only the fields documented in docs/guide/webhooks.md (event, entity.*, state.*) are available to a webhook filter.",
            ],
        )
    })
}

/// Delivers one event to one webhook: POSTs the (already serialized) JSON body with Grey's
/// signature/metadata headers and any operator-configured extra headers.
///
/// Instrumented as an outbound client span so a delivery — and any failure to send it — is
/// observable in the trace pipeline. The secret-bearing `webhook` is deliberately **not** recorded
/// (only its label and endpoint are); the response status is stamped onto the span on completion, and
/// a transport error or a non-success response is surfaced as the span's error via `err`, so operators
/// can find and debug failing deliveries by their status code, endpoint, and the entity involved.
#[tracing::instrument(
    name = "webhook.deliver",
    skip_all,
    err(Display),
    fields(
        otel.kind = "client",
        otel.name = webhook.label(),
        webhook.name = webhook.label(),
        webhook.endpoint = %webhook.endpoint,
        event.id = %event.id,
        event.kind = event.event.as_str(),
        entity.name = %event.entity.name,
        http.status_code = EmptyField,
    )
)]
async fn deliver(
    http: &reqwest::Client,
    webhook: &WebhookConfig,
    event: &WebhookEvent,
    body: Vec<u8>,
) -> Result<(), human_errors::Error> {
    let mut request = http
        .post(&webhook.endpoint)
        .timeout(webhook.timeout)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(EVENT_HEADER, event.event.as_str())
        .header(DELIVERY_HEADER, event.id.as_str());

    // Operator-supplied headers are applied first, before Grey's own signature/trace headers. They
    // are NOT covered by the signature (which authenticates only the timestamp and body), so a
    // receiver must not treat them as authenticated or trust them to be unmodified in transit. A
    // misconfigured name/value only fails this single delivery rather than the whole pass.
    for (name, value) in &webhook.headers {
        request = request.header(name.as_str(), value.as_str());
    }

    // Sign the event's own timestamp plus the body. The timestamp travels inside the signature
    // header's `t=` field, so a receiver has everything it needs to verify without a separate header.
    if let Some(secret) = webhook.secret.as_deref().filter(|s| !s.is_empty()) {
        let timestamp = event.timestamp.timestamp();
        let signature = sign(secret, timestamp, &body);
        request = request.header(SIGNATURE_HEADER, format!("t={timestamp},v1={signature}"));
    }

    // Propagate the current (`webhook.deliver`) trace context via W3C `traceparent`/`tracestate`, so a
    // receiver that records traces can stitch its handling onto Grey's delivery span. With no
    // telemetry pipeline configured the propagator is a no-op and the carrier stays empty.
    let mut carrier = HashMap::new();
    tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&Span::current().context(), &mut carrier)
    });
    for (name, value) in carrier {
        request = request.header(name, value);
    }

    let response = request.body(body).send().await.map_err(|e| {
        // A transport failure (DNS, connection, TLS, timeout) is an environmental/system problem.
        human_errors::wrap_system(
            e,
            format!("Could not deliver a webhook notification to '{}'.", webhook.endpoint),
            &[
                "Check that the endpoint URL is correct and reachable from this host (DNS, firewall, TLS).",
                "If the endpoint is healthy this is likely transient; the next state change will be delivered.",
            ],
        )
    })?;

    let status = response.status();
    Span::current().record("http.status_code", status.as_u16());

    if status.is_success() {
        debug!(name: "webhook.delivered", { webhook = webhook.label(), event.id = event.id, entity = event.entity.name, http.status_code = status.as_u16() }, "Delivered webhook event.");
        Ok(())
    } else if status.is_client_error() {
        // A 4xx is almost always a configuration problem at the Grey end (wrong URL, missing or
        // invalid auth header, a payload the endpoint won't accept).
        Err(human_errors::user(
            format!(
                "The webhook endpoint '{}' rejected the notification with HTTP {status}.",
                webhook.endpoint
            ),
            &[
                "Check that the endpoint accepts an HTTP POST with a JSON body at this URL.",
                "If the endpoint requires authentication, set the necessary header(s) under this webhook's `headers`.",
            ],
        ))
    } else {
        // A 5xx (or other non-success) is a fault on the receiving side.
        Err(human_errors::system(
            format!(
                "The webhook endpoint '{}' returned a server error (HTTP {status}).",
                webhook.endpoint
            ),
            &[
                "Check the health of the receiving service; Grey delivered the request but it could not process it.",
                "This is often transient; the next state change will be delivered.",
            ],
        ))
    }
}

/// Computes the delivery signature: the hex-encoded HMAC-SHA256 of `"<timestamp>.<body>"` keyed by
/// the shared secret — the scheme documented for Tailscale webhooks.
fn sign(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts a key of any length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Exposes a [`WebhookEvent`]'s fields to the `filt-rs` filter language. The addressable fields are:
/// `event`, `entity.type` (alias `entity.kind`), `entity.name`, `entity.tags.<key>` (alias
/// `tags.<key>`), and the `state.*` summary (`current`, `previous`, `healthy`, `was_healthy`,
/// `availability`). Unknown keys resolve to null, matching `filt-rs`'s own convention.
struct WebhookEventFilter<'a>(&'a WebhookEvent);

impl Filterable for WebhookEventFilter<'_> {
    fn get(&self, key: &str) -> FilterValue<'_> {
        let event = self.0;
        match key {
            "event" => string(event.event.as_str()),
            "entity.type" | "entity.kind" => string(event.entity.entity_type.as_str()),
            "entity.name" => string(&event.entity.name),
            "state.current" => string(&event.state.current),
            "state.previous" => string(&event.state.previous),
            "state.healthy" => FilterValue::Bool(event.state.healthy),
            "state.was_healthy" => FilterValue::Bool(event.state.was_healthy),
            "state.availability" => event
                .state
                .availability
                .map(FilterValue::Number)
                .unwrap_or(FilterValue::Null),
            k if k.starts_with("entity.tags.") => tag(&event.entity.tags, &k["entity.tags.".len()..]),
            k if k.starts_with("tags.") => tag(&event.entity.tags, &k["tags.".len()..]),
            _ => FilterValue::Null,
        }
    }
}

fn string(value: &str) -> FilterValue<'_> {
    FilterValue::String(Cow::Borrowed(value))
}

fn tag<'a>(tags: &'a HashMap<String, String>, key: &str) -> FilterValue<'a> {
    tags.get(key)
        .map(|value| FilterValue::String(Cow::Borrowed(value)))
        .unwrap_or(FilterValue::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::{CronRun, CronSchedule, CronStatus, Streak};
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn probe(name: &str, failing: bool) -> Probe {
        let now = Utc::now();
        let mut probe = Probe {
            name: name.into(),
            tags: vec![("service".into(), "Web".into())].into_iter().collect(),
            last_updated: now,
            history: Vec::new(),
            observations: HashMap::new(),
            streak: Streak::default(),
        };
        probe.streak.observe(!failing, now);
        probe
    }

    fn cron(name: &str, status: CronStatus) -> Cron {
        let mut cron = Cron::from_config(
            name,
            HashMap::new(),
            CronSchedule::Every(Duration::from_secs(3600)),
            None,
            None,
        );
        cron.push_run(CronRun {
            started_at: Utc::now(),
            status,
            duration: Some(Duration::from_secs(1)),
        });
        cron
    }

    fn webhook(endpoint: String, secret: Option<&str>, filter: &str) -> WebhookConfig {
        WebhookConfig {
            name: Some("test".into()),
            endpoint,
            secret: secret.map(str::to_string),
            headers: HashMap::new(),
            filter: Filter::new(filter).unwrap(),
            timeout: Duration::from_secs(5),
        }
    }

    /// The first pass seeds the baseline silently; only a subsequent token change produces an event,
    /// and an unchanged token produces nothing.
    #[test]
    fn detects_only_genuine_transitions() {
        let mut last = HashMap::new();
        let now = Utc::now();

        let passing = HashMap::from([("web".to_string(), probe("web", false))]);
        let failing = HashMap::from([("web".to_string(), probe("web", true))]);
        let empty_crons = HashMap::new();

        // First pass seeds "passing" and fires nothing.
        let events = detect_transitions(&mut last, now, &passing, &empty_crons, true);
        assert!(events.is_empty(), "the first observation must seed silently");

        // The probe goes failing: one event, summarising the transition.
        let events = detect_transitions(&mut last, now, &failing, &empty_crons, true);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state.previous, "passing");
        assert_eq!(events[0].state.current, "failing");
        assert!(!events[0].state.healthy);
        assert!(events[0].state.was_healthy);

        // No further change: nothing fires.
        let events = detect_transitions(&mut last, now, &failing, &empty_crons, true);
        assert!(events.is_empty());
    }

    /// With no webhooks configured the baseline is still tracked, so re-enabling notifications does
    /// not replay the state every entity is already in.
    #[test]
    fn seeds_baseline_even_when_not_notifying() {
        let mut last = HashMap::new();
        let now = Utc::now();
        let failing = HashMap::from([("web".to_string(), probe("web", true))]);
        let crons = HashMap::new();

        // notify=false: no events, but the baseline is recorded as "failing".
        let events = detect_transitions(&mut last, now, &failing, &crons, false);
        assert!(events.is_empty());

        // Now notifying, with the same (failing) state: still nothing, because it matches the seeded
        // baseline rather than being treated as a fresh transition.
        let events = detect_transitions(&mut last, now, &failing, &crons, true);
        assert!(events.is_empty());
    }

    /// Cron transitions are detected against the derived health token.
    #[test]
    fn detects_cron_transitions() {
        let mut last = HashMap::new();
        let now = Utc::now();
        let probes = HashMap::new();

        let succeeded = HashMap::from([("backup".to_string(), cron("backup", CronStatus::Succeeded))]);
        let failed = HashMap::from([("backup".to_string(), cron("backup", CronStatus::Failed))]);

        assert!(detect_transitions(&mut last, now, &probes, &succeeded, true).is_empty());
        let events = detect_transitions(&mut last, now, &probes, &failed, true);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state.previous, "succeeded");
        assert_eq!(events[0].state.current, "failed");
    }

    #[test]
    fn filters_match_the_exposed_fields() {
        let event = WebhookEvent::for_probe(
            "id",
            Utc::now(),
            &probe("web.prod", true),
            "passing",
            true,
        );

        assert!(event_matches(&Filter::new(r#"entity.type == "probe""#).unwrap(), &event).unwrap());
        assert!(event_matches(&Filter::new("state.healthy == false").unwrap(), &event).unwrap());
        assert!(event_matches(&Filter::new(r#"entity.tags.service == "Web""#).unwrap(), &event).unwrap());
        assert!(event_matches(&Filter::new(r#"tags.service == "Web""#).unwrap(), &event).unwrap());
        assert!(event_matches(&Filter::new(r#"entity.name matches r"^web\.""#).unwrap(), &event).unwrap());

        // A filter that doesn't match this event.
        assert!(!event_matches(&Filter::new(r#"entity.type == "cron""#).unwrap(), &event).unwrap());
        assert!(!event_matches(&Filter::new("state.was_healthy == false").unwrap(), &event).unwrap());
    }

    /// The HMAC matches an independent (OpenSSL-computed) reference vector, confirming the exact
    /// `"<timestamp>.<body>"` construction and hex encoding:
    ///
    /// ```sh
    /// printf '%s' '1700000000.{"hello":"world"}' | openssl dgst -sha256 -hmac 'topsecret'
    /// ```
    #[test]
    fn signature_matches_reference_vector() {
        let expected = "79883357e4c4c4abee43cf4b32367d67a1344520479e3e8c85e98406a6d6a2a5";
        let actual = sign("topsecret", 1_700_000_000, br#"{"hello":"world"}"#);
        assert_eq!(actual, expected);
    }

    /// A delivery is signed (Tailscale `t=,v1=` form), carries Grey's metadata headers and any extra
    /// operator headers, and posts the JSON body.
    #[tokio::test]
    async fn deliver_signs_and_posts() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut wh = webhook(format!("{}/hook", server.uri()), Some("topsecret"), "true");
        wh.headers.insert("X-Custom".into(), "yes".into());

        let event = WebhookEvent::for_probe(
            "evt-1",
            Utc::now(),
            &probe("web.prod", true),
            "passing",
            true,
        );
        let body = serde_json::to_vec(&event).unwrap();

        deliver(&reqwest::Client::new(), &wh, &event, body).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];

        assert_eq!(
            request.headers.get("grey-webhook-event").unwrap().to_str().unwrap(),
            "probe.state_changed"
        );
        assert_eq!(request.headers.get("grey-webhook-delivery").unwrap().to_str().unwrap(), "evt-1");
        assert_eq!(request.headers.get("x-custom").unwrap().to_str().unwrap(), "yes");

        // The signature header reconstructs as HMAC over `"<t>.<body>"`.
        let signature = request.headers.get("grey-webhook-signature").unwrap().to_str().unwrap();
        let (t, v1) = parse_signature(signature);
        assert_eq!(sign("topsecret", t, &request.body), v1);

        // The body is the JSON event.
        let decoded: WebhookEvent = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(decoded.entity.name, "web.prod");
        assert_eq!(decoded.state.current, "failing");
    }

    /// With no secret configured, no signature header is sent.
    #[tokio::test]
    async fn deliver_without_secret_is_unsigned() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&server)
            .await;

        let wh = webhook(format!("{}/hook", server.uri()), None, "true");
        let event = WebhookEvent::for_probe("evt", Utc::now(), &probe("web", true), "passing", true);
        let body = serde_json::to_vec(&event).unwrap();
        deliver(&reqwest::Client::new(), &wh, &event, body).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].headers.get("grey-webhook-signature").is_none());
    }

    /// A non-success response is surfaced as an error (which the `#[instrument(err)]` on `deliver`
    /// records on the span), so operators can observe failed deliveries.
    #[tokio::test]
    async fn deliver_reports_non_success_as_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let wh = webhook(format!("{}/hook", server.uri()), None, "true");
        let event = WebhookEvent::for_probe("evt", Utc::now(), &probe("web", true), "passing", true);
        let body = serde_json::to_vec(&event).unwrap();

        let result = deliver(&reqwest::Client::new(), &wh, &event, body).await;
        let err = result.expect_err("a 500 response must be reported as an error");
        assert!(err.to_string().contains("500"), "the error should name the status: {err}");
    }

    /// A transport failure (an unreachable endpoint) is reported as an error rather than panicking.
    #[tokio::test]
    async fn deliver_reports_transport_failure_as_error() {
        // A reserved TEST-NET-1 address that won't accept connections; a short timeout bounds the test.
        let mut wh = webhook("http://192.0.2.1:9/hook".into(), None, "true");
        wh.timeout = Duration::from_millis(300);
        let event = WebhookEvent::for_probe("evt", Utc::now(), &probe("web", true), "passing", true);
        let body = serde_json::to_vec(&event).unwrap();

        let result = deliver(&reqwest::Client::new(), &wh, &event, body).await;
        assert!(result.is_err(), "an unreachable endpoint must be reported as an error");
    }

    /// A 4xx is reported as a (user/configuration) error distinct from a transport/5xx failure.
    #[tokio::test]
    async fn deliver_reports_client_error_as_user_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let wh = webhook(format!("{}/hook", server.uri()), None, "true");
        let event = WebhookEvent::for_probe("evt", Utc::now(), &probe("web", true), "passing", true);
        let body = serde_json::to_vec(&event).unwrap();

        let err = deliver(&reqwest::Client::new(), &wh, &event, body)
            .await
            .expect_err("a 401 response must be reported as an error");
        assert!(err.to_string().contains("401"), "the error should name the status: {err}");
    }

    /// End-to-end: a full evaluation pass over a real [`State`] seeds silently, then — once a probe
    /// flips to failing — delivers a matching `probe.state_changed` event to the configured endpoint.
    /// Exercises `evaluate` → `dispatch` → filter match → `deliver` wired through the store.
    #[tokio::test]
    async fn evaluate_delivers_on_a_probe_transition() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config = format!(
            "state: {state}\nprobes:\n  - name: web\n    policy:\n      interval: 60s\n      timeout: 5s\n    target: !Http\n      url: https://example.com\nwebhooks:\n  - endpoint: {endpoint}/hook\n    secret: topsecret\n    filter: 'true'\n",
            state = dir.path().join("state.redb").display().to_string().replace('\\', "/"),
            endpoint = server.uri(),
        );
        let config_path = dir.path().join("config.yml");
        tokio::fs::write(&config_path, config).await.unwrap();

        let state = State::new(&config_path).await.unwrap();
        let mut notifier = Notifier::new(state.clone());

        // First pass seeds the baseline (the probe reads as passing) and delivers nothing.
        notifier.evaluate().await.unwrap();
        assert!(server.received_requests().await.unwrap().is_empty());

        // Record a failing sample so the pooled probe flips to failing.
        let mut sample = crate::result::ProbeResult::new();
        sample.pass = false;
        sample.message = "boom".into();
        state.update_probe_state("web", sample.finish()).await.unwrap();

        // The next pass detects passing -> failing and delivers a matching event.
        notifier.evaluate().await.unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "the transition should produce exactly one delivery");

        let delivered: WebhookEvent = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(delivered.entity.name, "web");
        assert_eq!(delivered.state.previous, "passing");
        assert_eq!(delivered.state.current, "failing");

        // A further pass with no further change delivers nothing more.
        notifier.evaluate().await.unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    fn parse_signature(header: &str) -> (i64, String) {
        let mut timestamp = 0i64;
        let mut v1 = String::new();
        for part in header.split(',') {
            if let Some(t) = part.strip_prefix("t=") {
                timestamp = t.parse().unwrap();
            } else if let Some(sig) = part.strip_prefix("v1=") {
                v1 = sig.to_string();
            }
        }
        (timestamp, v1)
    }
}
