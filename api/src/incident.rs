use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Identifier;

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

    /// A human-readable label for display (e.g. in status text and `<select>` options).
    pub fn label(self) -> &'static str {
        match self {
            Impact::Offline => "Offline",
            Impact::Degraded => "Degraded",
            Impact::None => "Operational",
            Impact::Hidden => "Hidden",
        }
    }

    /// The serialised token for this impact, matching the serde (`rename_all = "lowercase"`)
    /// representation — used as `<select>`/`<option>` values and anywhere a stable string is needed.
    pub fn as_str(self) -> &'static str {
        match self {
            Impact::Offline => "offline",
            Impact::Degraded => "degraded",
            Impact::None => "none",
            Impact::Hidden => "hidden",
        }
    }
}

impl std::str::FromStr for Impact {
    type Err = ();

    /// Parses an impact from its [`Impact::as_str`] token, falling back to `Hidden` for anything
    /// unrecognised (the safest default, since hidden incidents stay private).
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "offline" => Impact::Offline,
            "degraded" => Impact::Degraded,
            "none" => Impact::None,
            _ => Impact::Hidden,
        })
    }
}

/// A single update posted against an incident, identified by its position in the incident's `updates`
/// list. `message` is markdown; `impact` drives the incident's status from this point on the timeline.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentUpdate {
    pub impact: Impact,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,
    pub message: String,
}

/// An operator-recorded incident/event. Everything beyond its `id`, `title` and `version` is derived
/// from its `updates`: the start/end times, the current impact, and the created/updated times. The
/// `version` is a monotonically increasing counter used for optimistic concurrency (check-and-set via
/// the API's `If-Match`/`ETag`).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Incident {
    pub id: Identifier,
    pub title: String,
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub updates: Vec<IncidentUpdate>,
}

impl Incident {
    /// The updates sorted oldest-first by timestamp.
    pub fn sorted_updates(&self) -> Vec<&IncidentUpdate> {
        let mut updates: Vec<&IncidentUpdate> = self.updates.iter().collect();
        updates.sort_by_key(|u| u.timestamp);
        updates
    }

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

    /// Whether the incident is currently affecting service (its latest update is offline or degraded).
    pub fn is_active(&self) -> bool {
        matches!(self.current_impact(), Impact::Offline | Impact::Degraded)
    }

    /// When the incident began: its earliest update's timestamp.
    pub fn started_at(&self) -> Option<DateTime<Utc>> {
        self.updates.iter().map(|u| u.timestamp).min()
    }

    /// When the incident was last updated: its most recent update's timestamp.
    pub fn last_updated(&self) -> Option<DateTime<Utc>> {
        self.updates.iter().map(|u| u.timestamp).max()
    }

    /// When the incident ended: the latest update's timestamp once the current impact is `None`
    /// (resolved); otherwise `None` (still ongoing).
    pub fn ended_at(&self) -> Option<DateTime<Utc>> {
        if self.current_impact() == Impact::None {
            self.last_updated()
        } else {
            None
        }
    }

    /// When the current impact began: the start of the trailing run of updates sharing the current
    /// impact. `None` when there are no updates. Drives the "offline for 1h" header text.
    pub fn impact_since(&self) -> Option<DateTime<Utc>> {
        let current = self.current_impact();
        let mut since = None;
        for update in self.sorted_updates().into_iter().rev() {
            if update.impact == current {
                since = Some(update.timestamp);
            } else {
                break;
            }
        }
        since
    }
}

/// The body for creating an incident: a title plus its first update (impact + markdown message). The
/// server assigns the id, the update's timestamp, and the initial version.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CreateIncident {
    pub title: String,
    pub impact: Impact,
    pub message: String,
}

/// The body for replacing an incident (title + the full updates list). Applied as a check-and-set
/// against the incident's current `version` (sent as an `If-Match` header), so concurrent edits don't
/// silently clobber one another.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentEdit {
    pub title: String,
    #[serde(default)]
    pub updates: Vec<IncidentUpdate>,
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

    fn update(impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate {
            impact,
            timestamp: ts(secs),
            message: format!("update at {secs}"),
        }
    }

    fn sample(updates: Vec<IncidentUpdate>) -> Incident {
        Incident {
            id: Identifier::from(1_234_567u64),
            title: "Database outage".into(),
            version: 1,
            updates,
        }
    }

    #[test]
    fn impact_serializes_lowercase_and_ranks() {
        assert_eq!(serde_json::to_string(&Impact::Degraded).unwrap(), "\"degraded\"");
        assert_eq!(serde_json::to_string(&Impact::None).unwrap(), "\"none\"");
        assert_eq!(serde_json::from_str::<Impact>("\"hidden\"").unwrap(), Impact::Hidden);
        assert_eq!(Impact::default(), Impact::Hidden);
        assert!(Impact::Offline.rank() > Impact::Degraded.rank());
        assert!(Impact::None.rank() > Impact::Hidden.rank());
    }

    #[test]
    fn incident_round_trips_with_id_as_string_and_epoch_updates() {
        let incident = sample(vec![update(Impact::Offline, 1_700_000_100)]);
        let json = serde_json::to_value(&incident).unwrap();
        // id serializes as a grouped base36 string; updates carry epoch-second timestamps.
        assert_eq!(json["id"], serde_json::Value::String(incident.id.to_string()));
        assert_eq!(json["version"], 1);
        assert_eq!(json["updates"][0]["impact"], "offline");
        assert_eq!(json["updates"][0]["timestamp"], 1_700_000_100);
        // No removed fields linger.
        assert!(json.get("description").is_none());
        assert!(json.get("start_time").is_none());
        assert!(json.get("affected_services").is_none());

        let decoded: Incident = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, incident);
    }

    #[test]
    fn derived_times_and_impact_follow_updates() {
        let incident = sample(vec![
            update(Impact::Offline, 1_700_000_100),
            update(Impact::None, 1_700_000_500),
        ]);
        assert_eq!(incident.current_impact(), Impact::None);
        assert_eq!(incident.started_at(), Some(ts(1_700_000_100)));
        assert_eq!(incident.last_updated(), Some(ts(1_700_000_500)));
        assert_eq!(incident.ended_at(), Some(ts(1_700_000_500)), "a 'none' impact resolves it");
        assert!(incident.is_public());
        assert!(!incident.is_active());
    }

    #[test]
    fn no_updates_means_hidden_and_inactive() {
        let incident = sample(vec![]);
        assert_eq!(incident.current_impact(), Impact::Hidden);
        assert!(!incident.is_public(), "an incident with no updates is a hidden draft");
        assert!(!incident.is_active());
        assert_eq!(incident.started_at(), None);
        assert_eq!(incident.ended_at(), None);
        assert_eq!(incident.impact_since(), None);
    }

    #[test]
    fn impact_since_is_the_start_of_the_trailing_run() {
        let incident = sample(vec![
            update(Impact::Offline, 100),
            update(Impact::Offline, 200),
            update(Impact::Degraded, 300),
        ]);
        assert_eq!(incident.current_impact(), Impact::Degraded);
        assert_eq!(incident.impact_since(), Some(ts(300)));
        assert!(incident.is_active());
    }
}
