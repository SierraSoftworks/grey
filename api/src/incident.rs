use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The impact an incident update reports. An incident's current impact is that of its most recent
/// update (defaulting to `Hidden` when it has none). `Hidden` keeps the incident from unauthenticated
/// viewers, replacing the previous draft/visible concept.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Impact {
    /// The service is fully unavailable.
    Offline,
    /// The service is degraded but partially available.
    Degraded,
    /// No service impact (e.g. resolved, or informational).
    None,
    /// Hidden from unauthenticated viewers — used while preparing an incident.
    #[default]
    Hidden,
}

impl Impact {
    /// Severity rank for picking the "worst" impact among active incidents. Higher is worse;
    /// `Hidden` ranks lowest since hidden incidents never affect the public status.
    pub fn rank(self) -> u8 {
        match self {
            Impact::Offline => 3,
            Impact::Degraded => 2,
            Impact::None => 1,
            Impact::Hidden => 0,
        }
    }
}

/// A single update posted against an incident. `message` is markdown; `impact` drives the incident's
/// status from this point on the timeline.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentUpdate {
    pub id: String,
    pub impact: Impact,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,
    pub message: String,
}

/// An operator-recorded incident/event, persisted in the state database and surfaced on the UI
/// timeline. Its impact is derived from its updates, which run from `start_time` to the optional
/// `end_time`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Incident {
    pub id: String,
    pub title: String,

    /// Markdown description of the incident.
    #[serde(default)]
    pub description: String,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub start_time: DateTime<Utc>,
    #[serde(default, with = "chrono::serde::ts_seconds_option")]
    pub end_time: Option<DateTime<Utc>>,

    /// Optional service tags (matching probe `service` tags) this incident affects.
    #[serde(default)]
    pub affected_services: Vec<String>,

    #[serde(default)]
    pub updates: Vec<IncidentUpdate>,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub updated_at: DateTime<Utc>,
}

impl Incident {
    /// The incident's current impact: that of its most recent update, or `Hidden` when it has no
    /// updates (a freshly created, not-yet-published incident).
    pub fn current_impact(&self) -> Impact {
        self.updates
            .iter()
            .max_by_key(|u| u.timestamp)
            .map(|u| u.impact)
            .unwrap_or(Impact::Hidden)
    }

    /// Whether the incident is visible to unauthenticated viewers (its current impact is not hidden).
    pub fn is_public(&self) -> bool {
        self.current_impact() != Impact::Hidden
    }

    /// Whether the incident is currently affecting service: ongoing (no `end_time`) and currently
    /// offline or degraded. Drives the "active incidents" header colour.
    pub fn is_active(&self) -> bool {
        self.end_time.is_none() && matches!(self.current_impact(), Impact::Offline | Impact::Degraded)
    }

    /// Whether the incident is still ongoing (has no recorded `end_time`).
    pub fn is_ongoing(&self) -> bool {
        self.end_time.is_none()
    }

    /// When the current impact began: the start of the trailing run of updates sharing the current
    /// impact. `None` when there are no updates. Drives the "offline for 1h" header text.
    pub fn impact_since(&self) -> Option<DateTime<Utc>> {
        let current = self.current_impact();
        let mut sorted: Vec<&IncidentUpdate> = self.updates.iter().collect();
        sorted.sort_by_key(|u| u.timestamp);
        let mut since = None;
        for update in sorted.iter().rev() {
            if update.impact == current {
                since = Some(update.timestamp);
            } else {
                break;
            }
        }
        since
    }
}

/// The editable fields of an incident, supplied by an administrator when creating or replacing one.
/// The server owns `id`, `created_at`, `updated_at` and the `updates` list, so they are not part of
/// the input.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentInput {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub start_time: DateTime<Utc>,
    #[serde(default, with = "chrono::serde::ts_seconds_option")]
    pub end_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub affected_services: Vec<String>,
}

/// An update posted against an incident. The server assigns the update's `id`, and defaults the
/// `timestamp` to the current time when omitted.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct NewIncidentUpdate {
    pub impact: Impact,
    pub message: String,
    #[serde(default, with = "chrono::serde::ts_seconds_option")]
    pub timestamp: Option<DateTime<Utc>>,
}

/// The signed-in administrator, derived from validated token claims, returned by `/api/v1/admin/me`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminUser {
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn update(id: &str, impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate {
            id: id.into(),
            impact,
            timestamp: ts(secs),
            message: format!("update {id}"),
        }
    }

    fn sample() -> Incident {
        Incident {
            id: "1700000000-abc".into(),
            title: "Database outage".into(),
            description: "Primary DB unreachable".into(),
            start_time: ts(1_700_000_000),
            end_time: None,
            affected_services: vec!["api".into()],
            updates: vec![update("u1", Impact::Offline, 1_700_000_100)],
            created_at: ts(1_700_000_000),
            updated_at: ts(1_700_000_100),
        }
    }

    #[test]
    fn impact_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Impact::Degraded).unwrap(), "\"degraded\"");
        assert_eq!(serde_json::to_string(&Impact::None).unwrap(), "\"none\"");
        assert_eq!(serde_json::from_str::<Impact>("\"hidden\"").unwrap(), Impact::Hidden);
        assert_eq!(Impact::default(), Impact::Hidden);
        assert!(Impact::Offline.rank() > Impact::Degraded.rank());
        assert!(Impact::None.rank() > Impact::Hidden.rank());
    }

    #[test]
    fn incident_round_trips_with_epoch_timestamps() {
        let incident = sample();
        let json = serde_json::to_value(&incident).unwrap();
        assert_eq!(json["startTime"].as_i64(), None, "fields are snake_case, not camelCase");
        assert_eq!(json["start_time"].as_i64(), Some(1_700_000_000));
        assert_eq!(json["end_time"], serde_json::Value::Null);
        assert_eq!(json["updates"][0]["impact"], "offline");

        let decoded: Incident = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, incident);
    }

    #[test]
    fn current_impact_follows_latest_update() {
        let mut incident = sample();
        // Latest update is offline -> public and active.
        assert_eq!(incident.current_impact(), Impact::Offline);
        assert!(incident.is_public());
        assert!(incident.is_active());

        // Adding a later "none" update resolves the impact.
        incident.updates.push(update("u2", Impact::None, 1_700_000_500));
        assert_eq!(incident.current_impact(), Impact::None);
        assert!(incident.is_public());
        assert!(!incident.is_active(), "a 'none' impact is not active");

        // A hidden latest update takes it private.
        incident.updates.push(update("u3", Impact::Hidden, 1_700_000_900));
        assert!(!incident.is_public());
    }

    #[test]
    fn no_updates_means_hidden_and_inactive() {
        let mut incident = sample();
        incident.updates.clear();
        assert_eq!(incident.current_impact(), Impact::Hidden);
        assert!(!incident.is_public(), "an incident with no updates is a hidden draft");
        assert!(!incident.is_active());
        assert_eq!(incident.impact_since(), None);
    }

    #[test]
    fn impact_since_is_the_start_of_the_trailing_run() {
        let mut incident = sample();
        incident.updates = vec![
            update("a", Impact::Offline, 100),
            update("b", Impact::Offline, 200),
            update("c", Impact::Degraded, 300),
        ];
        // Current impact is degraded since 300 (the trailing run of equal impacts).
        assert_eq!(incident.current_impact(), Impact::Degraded);
        assert_eq!(incident.impact_since(), Some(ts(300)));

        // A continuous offline run reports the first offline timestamp.
        incident.updates = vec![
            update("a", Impact::Offline, 100),
            update("b", Impact::Offline, 200),
        ];
        assert_eq!(incident.impact_since(), Some(ts(100)));
    }

    #[test]
    fn missing_updates_default_to_hidden_failing_closed() {
        let incident: Incident = serde_json::from_str(
            r#"{"id":"x","title":"t","start_time":1700000000,"created_at":1700000000,"updated_at":1700000000}"#,
        )
        .unwrap();
        assert!(!incident.is_public(), "missing updates must fail closed (hidden)");
        assert!(incident.updates.is_empty());
        assert!(incident.affected_services.is_empty());
        assert_eq!(incident.description, "");
    }
}
