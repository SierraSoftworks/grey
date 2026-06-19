//! The webhook event payload: the JSON document Grey delivers to a configured endpoint when a probe
//! or cron changes state.
//!
//! This is a pure DTO (it never references the `filt-rs` filter language or the HTTP machinery — those
//! live in the agent, which owns dispatch and filtering). It carries a small, stable summary of the
//! transition under [`WebhookState`] for easy filtering and routing, alongside the full entity
//! snapshot (the [`Probe`] — with its streak, history, observations and tags — or the [`Cron`] — with
//! its runs and last check-in) so a consumer has everything it needs without a follow-up read.
//!
//! The payload mirrors the probe/cron API representation rather than any single node's view: the
//! transition is derived from the cluster-converged [`crate::Streak`] (so recovery settling is
//! already accounted for), and the embedded snapshot carries the observations reported by *every*
//! observer. There is therefore no per-node identity on the event.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Cron, Probe};

/// The schema version stamped onto every [`WebhookEvent`]. Bump this when the payload shape changes
/// in a way consumers need to discriminate; a consumer can branch on `version` to handle multiple
/// schemas during a migration.
pub const WEBHOOK_SCHEMA_VERSION: &str = "v1";

/// The default schema version, used when deserializing a payload that predates the `version` field.
fn default_schema_version() -> String {
    WEBHOOK_SCHEMA_VERSION.to_string()
}

/// The kind of state-change event. The wire value is a dotted `"<entity>.state_changed"` token so a
/// consumer can route on it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebhookEventKind {
    #[serde(rename = "probe.state_changed")]
    ProbeStateChanged,
    #[serde(rename = "cron.state_changed")]
    CronStateChanged,
}

impl WebhookEventKind {
    /// The dotted token used both on the wire and in the `Grey-Webhook-Event` header.
    pub fn as_str(self) -> &'static str {
        match self {
            WebhookEventKind::ProbeStateChanged => "probe.state_changed",
            WebhookEventKind::CronStateChanged => "cron.state_changed",
        }
    }
}

/// Which kind of entity an event describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookEntityType {
    Probe,
    Cron,
}

impl WebhookEntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            WebhookEntityType::Probe => "probe",
            WebhookEntityType::Cron => "cron",
        }
    }
}

/// Identifies the entity whose state changed: its type, name, and tags. The tags are surfaced here
/// (in addition to the full snapshot below) so routing rules can address `entity.tags.<key>` without
/// reaching into the entity body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookEntity {
    #[serde(rename = "type")]
    pub entity_type: WebhookEntityType,
    pub name: String,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

/// A compact summary of the transition that triggered the event. `current`/`previous` are the
/// derived status tokens (a probe is `"passing"`/`"failing"`; a cron is one of the
/// [`crate::CronHealth`] tokens), while `healthy`/`was_healthy` collapse those onto the pass/fail axis
/// so a consumer can filter on `state.healthy == false` regardless of the specific failure mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookState {
    /// The status token the entity now reads as.
    pub current: String,
    /// The status token the entity read as before this transition.
    pub previous: String,
    /// Whether the current state reads as healthy (passing).
    pub healthy: bool,
    /// Whether the previous state read as healthy (passing).
    pub was_healthy: bool,
    /// When the current state was entered, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
    /// The entity's availability over its retained history, as a percentage. Only meaningful for
    /// probes; omitted for crons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<f64>,
}

/// A state-change notification for a single probe or cron, as delivered to a webhook endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// The payload schema version (`"v1"` today). Lets a consumer discriminate between schema
    /// versions as the payload evolves; see [`WEBHOOK_SCHEMA_VERSION`].
    #[serde(default = "default_schema_version")]
    pub version: String,

    /// A unique identifier for this event, also echoed in the `Grey-Webhook-Delivery` header so a
    /// consumer can de-duplicate retried or fan-out deliveries.
    pub id: String,

    /// The event kind (`probe.state_changed` / `cron.state_changed`).
    pub event: WebhookEventKind,

    /// When the event was generated.
    pub timestamp: DateTime<Utc>,

    /// The entity whose state changed.
    pub entity: WebhookEntity,

    /// A summary of the transition.
    pub state: WebhookState,

    /// The full probe snapshot (with streak, history, observations and tags), for a probe event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<Probe>,

    /// The full cron snapshot (with runs and the last check-in), for a cron event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<Cron>,
}

impl WebhookEvent {
    /// Builds a `probe.state_changed` event from the cluster-pooled probe snapshot, given the status
    /// it transitioned away from. The current state is derived from the snapshot's converged streak,
    /// and the embedded snapshot carries every observer's observations.
    pub fn for_probe(
        id: impl Into<String>,
        timestamp: DateTime<Utc>,
        probe: &Probe,
        previous_token: impl Into<String>,
        previous_healthy: bool,
    ) -> Self {
        Self {
            version: WEBHOOK_SCHEMA_VERSION.to_string(),
            id: id.into(),
            event: WebhookEventKind::ProbeStateChanged,
            timestamp,
            entity: WebhookEntity {
                entity_type: WebhookEntityType::Probe,
                name: probe.name.clone(),
                tags: probe.tags.clone(),
            },
            state: WebhookState {
                current: probe.status_token().to_string(),
                previous: previous_token.into(),
                healthy: probe.passing(),
                was_healthy: previous_healthy,
                since: probe.streak.since(),
                availability: Some(probe.availability()),
            },
            probe: Some(probe.clone()),
            cron: None,
        }
    }

    /// Builds a `cron.state_changed` event from the cluster-pooled cron snapshot evaluated at `now`,
    /// given the status it transitioned away from.
    pub fn for_cron(
        id: impl Into<String>,
        timestamp: DateTime<Utc>,
        cron: &Cron,
        now: DateTime<Utc>,
        previous_token: impl Into<String>,
        previous_healthy: bool,
    ) -> Self {
        let health = cron.health(now);
        Self {
            version: WEBHOOK_SCHEMA_VERSION.to_string(),
            id: id.into(),
            event: WebhookEventKind::CronStateChanged,
            timestamp,
            entity: WebhookEntity {
                entity_type: WebhookEntityType::Cron,
                name: cron.name.clone(),
                tags: cron.tags.clone(),
            },
            state: WebhookState {
                current: health.as_str().to_string(),
                previous: previous_token.into(),
                healthy: health.passing(),
                was_healthy: previous_healthy,
                since: cron.since(health),
                availability: None,
            },
            probe: None,
            cron: Some(cron.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CheckIn, CronRun, CronSchedule, CronStatus, Streak};
    use std::time::Duration;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn failing_probe(name: &str) -> Probe {
        let now = Utc::now();
        let mut probe = Probe {
            name: name.into(),
            tags: vec![("service".into(), "Web".into())].into_iter().collect(),
            last_updated: now,
            history: Vec::new(),
            observations: HashMap::new(),
            streak: Streak::default(),
        };
        // A single failing observation flips the streak to failing.
        probe.streak.observe(false, now);
        probe
    }

    #[test]
    fn probe_event_summarises_the_transition_and_carries_the_snapshot() {
        let probe = failing_probe("web.prod");
        let event = WebhookEvent::for_probe("evt-1", ts(1_000), &probe, "passing", true);

        assert_eq!(event.event, WebhookEventKind::ProbeStateChanged);
        assert_eq!(event.entity.entity_type, WebhookEntityType::Probe);
        assert_eq!(event.entity.name, "web.prod");
        assert_eq!(event.entity.tags.get("service").map(String::as_str), Some("Web"));
        assert_eq!(event.state.current, "failing");
        assert_eq!(event.state.previous, "passing");
        assert!(!event.state.healthy);
        assert!(event.state.was_healthy);
        assert!(event.state.availability.is_some());
        // The full snapshot rides along so a consumer sees streak/history/observations.
        assert!(event.probe.is_some());
        assert!(event.cron.is_none());
    }

    #[test]
    fn cron_event_uses_the_derived_health_token() {
        let mut cron = Cron::from_config(
            "backup",
            vec![("team".into(), "Ops".into())].into_iter().collect(),
            CronSchedule::Every(Duration::from_secs(60)),
            None,
            None,
        );
        cron.push_run(CronRun { started_at: ts(100), status: CronStatus::Failed, duration: Some(Duration::from_secs(5)) });
        cron.last_checkin = Some(CheckIn { at: ts(105), status: CronStatus::Failed, message: "boom".into() });

        let event = WebhookEvent::for_cron("evt-2", ts(200), &cron, ts(200), "succeeded", true);

        assert_eq!(event.event, WebhookEventKind::CronStateChanged);
        assert_eq!(event.entity.entity_type, WebhookEntityType::Cron);
        assert_eq!(event.state.current, "failed");
        assert_eq!(event.state.previous, "succeeded");
        assert!(!event.state.healthy);
        assert!(event.state.availability.is_none(), "crons have no availability");
        assert!(event.cron.is_some());
        assert!(event.probe.is_none());
    }

    #[test]
    fn serialises_with_a_stable_external_shape() {
        let probe = failing_probe("web.prod");
        let event = WebhookEvent::for_probe("evt-1", ts(1_000), &probe, "passing", true);
        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["version"], "v1");
        assert_eq!(json["event"], "probe.state_changed");
        assert_eq!(json["entity"]["type"], "probe");
        assert_eq!(json["entity"]["name"], "web.prod");
        assert_eq!(json["state"]["current"], "failing");
        assert_eq!(json["state"]["healthy"], false);
        // The probe snapshot exposes the streak/history/observations to consumers.
        assert!(json["probe"]["streak"].is_object());
        assert!(json["probe"]["observations"].is_object());
        assert!(json.get("cron").is_none(), "the cron field is omitted for probe events");
        // The payload matches the probe/cron API shape: there is no per-node identity on it.
        assert!(json.get("node").is_none(), "events carry no node field");
    }

    /// A freshly built event is stamped with the current schema version, and a payload that predates
    /// the `version` field still deserializes — defaulting to the current version.
    #[test]
    fn version_is_stamped_and_defaults_when_absent() {
        let event = WebhookEvent::for_probe("evt", ts(1), &failing_probe("web"), "passing", true);
        assert_eq!(event.version, WEBHOOK_SCHEMA_VERSION);

        // A payload missing `version` (e.g. from a pre-versioning agent) decodes with the default.
        let legacy = serde_json::json!({
            "id": "evt",
            "event": "probe.state_changed",
            "timestamp": "2026-06-19T12:00:00Z",
            "entity": { "type": "probe", "name": "web" },
            "state": { "current": "failing", "previous": "passing", "healthy": false, "was_healthy": true }
        });
        let decoded: WebhookEvent = serde_json::from_value(legacy).unwrap();
        assert_eq!(decoded.version, WEBHOOK_SCHEMA_VERSION);
        assert_eq!(decoded.entity.name, "web");
    }
}
