use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The health status reported by an incident update. The vocabulary mirrors the probe status
/// language used elsewhere in the UI so the two can share styling.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum IncidentStatus {
    Healthy,
    Degraded,
    Offline,
    #[default]
    Unknown,
}

/// A single status update posted against an incident. `message` is markdown.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentUpdate {
    pub id: String,
    pub status: IncidentStatus,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,
    pub message: String,
}

/// An operator-recorded incident/event, persisted in the state database and surfaced on the UI
/// timeline. Times other than `start_time` are optional so an incident can be filled in as it
/// progresses (detected, mitigated, resolved).
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
    #[serde(default, with = "chrono::serde::ts_seconds_option")]
    pub detection_time: Option<DateTime<Utc>>,
    #[serde(default, with = "chrono::serde::ts_seconds_option")]
    pub mitigation_time: Option<DateTime<Utc>>,

    /// Optional service tags (matching probe `service` tags) this incident affects.
    #[serde(default)]
    pub affected_services: Vec<String>,

    /// When false the incident is hidden from unauthenticated viewers. Defaults to false so that a
    /// record missing the flag fails closed (hidden) rather than leaking.
    #[serde(default)]
    pub visible: bool,

    #[serde(default)]
    pub updates: Vec<IncidentUpdate>,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub updated_at: DateTime<Utc>,
}

impl Incident {
    /// The status from the most recent update (by timestamp), or `Unknown` when there are none.
    pub fn current_status(&self) -> IncidentStatus {
        self.updates
            .iter()
            .max_by_key(|u| u.timestamp)
            .map(|u| u.status)
            .unwrap_or_default()
    }

    /// Whether the incident is still ongoing (has no recorded `end_time`).
    pub fn is_ongoing(&self) -> bool {
        self.end_time.is_none()
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
    #[serde(default, with = "chrono::serde::ts_seconds_option")]
    pub detection_time: Option<DateTime<Utc>>,
    #[serde(default, with = "chrono::serde::ts_seconds_option")]
    pub mitigation_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub affected_services: Vec<String>,
    /// Whether the incident is visible to unauthenticated viewers. Defaults to visible on create.
    #[serde(default = "default_true")]
    pub visible: bool,
}

/// A status update posted against an incident. The server assigns the update's `id`, and defaults
/// the `timestamp` to the current time when omitted.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct NewIncidentUpdate {
    pub status: IncidentStatus,
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

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn incident_status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&IncidentStatus::Degraded).unwrap(),
            "\"degraded\""
        );
        assert_eq!(
            serde_json::from_str::<IncidentStatus>("\"offline\"").unwrap(),
            IncidentStatus::Offline
        );
    }

    #[test]
    fn incident_round_trips_with_epoch_timestamps() {
        let incident = Incident {
            id: "1700000000-abc".into(),
            title: "Database outage".into(),
            description: "Primary DB unreachable".into(),
            start_time: ts(1_700_000_000),
            end_time: None,
            detection_time: Some(ts(1_700_000_060)),
            mitigation_time: None,
            affected_services: vec!["api".into()],
            visible: true,
            updates: vec![IncidentUpdate {
                id: "u1".into(),
                status: IncidentStatus::Offline,
                timestamp: ts(1_700_000_100),
                message: "We are investigating.".into(),
            }],
            created_at: ts(1_700_000_000),
            updated_at: ts(1_700_000_100),
        };

        let json = serde_json::to_value(&incident).unwrap();
        // Timestamps are serialized as unix epoch seconds, matching the rest of the API.
        assert_eq!(json["startTime"].as_i64(), None, "fields are snake_case, not camelCase");
        assert_eq!(json["start_time"].as_i64(), Some(1_700_000_000));
        assert_eq!(json["end_time"], serde_json::Value::Null);

        let decoded: Incident = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, incident);
    }

    #[test]
    fn current_status_uses_latest_update_by_timestamp() {
        let mut incident = Incident {
            id: "i".into(),
            title: "t".into(),
            description: String::new(),
            start_time: ts(0),
            end_time: None,
            detection_time: None,
            mitigation_time: None,
            affected_services: vec![],
            visible: true,
            updates: vec![],
            created_at: ts(0),
            updated_at: ts(0),
        };
        assert_eq!(incident.current_status(), IncidentStatus::Unknown);

        incident.updates.push(IncidentUpdate {
            id: "a".into(),
            status: IncidentStatus::Offline,
            timestamp: ts(100),
            message: String::new(),
        });
        incident.updates.push(IncidentUpdate {
            id: "b".into(),
            status: IncidentStatus::Healthy,
            timestamp: ts(200),
            message: String::new(),
        });
        // Even if list order were shuffled, the latest timestamp wins.
        assert_eq!(incident.current_status(), IncidentStatus::Healthy);
    }

    #[test]
    fn defaults_fill_optional_fields_and_visibility_fails_closed() {
        let incident: Incident = serde_json::from_str(
            r#"{"id":"x","title":"t","start_time":1700000000,"created_at":1700000000,"updated_at":1700000000}"#,
        )
        .unwrap();
        assert!(!incident.visible, "missing visibility must fail closed (hidden)");
        assert!(incident.end_time.is_none());
        assert!(incident.affected_services.is_empty());
        assert!(incident.updates.is_empty());
        assert_eq!(incident.description, "");
    }
}
