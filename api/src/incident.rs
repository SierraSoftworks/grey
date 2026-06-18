use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Identifier, IncidentUpdateId};

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

/// A single update posted against an incident — a **standalone, gossip-replicated entity** in its own
/// right (so adding/editing different updates never conflicts across the cluster). `message` is
/// markdown; `impact` drives the incident's status from this update's `timestamp` onward and is fixed
/// once posted. `version` is the wall-clock last-modified time in milliseconds — the gossip LWW clock
/// and the HTTP ETag — and `deleted` is a propagating tombstone (filtered from API responses).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentUpdate {
    pub id: IncidentUpdateId,
    pub impact: Impact,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,
    pub message: String,
    /// Last-modified wall-clock time in milliseconds: the gossip version and the HTTP ETag.
    #[serde(default)]
    pub version: u64,
    /// A propagating delete tombstone. Tombstoned updates are filtered from API responses and reaped
    /// by GC once aged out.
    #[serde(default)]
    pub deleted: bool,
}

impl IncidentUpdate {
    /// The parent incident this update belongs to (the high bits of its id).
    pub fn incident_id(&self) -> Identifier {
        self.id.incident_id()
    }
}

/// An operator-recorded incident/event header — a **standalone, gossip-replicated entity** holding
/// only its own mutable fields (`title`, the `deleted` tombstone). Its updates are separate entities
/// joined into an [`IncidentView`] for display. `version` is the wall-clock last-modified time in
/// milliseconds: the gossip LWW clock and the HTTP ETag.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Incident {
    pub id: Identifier,
    pub title: String,
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub deleted: bool,
}

/// An incident joined with its (visible, sorted-oldest-first) updates — the shape the API returns and
/// the UI renders. Everything beyond the incident's `id`/`title` is derived from `updates`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentView {
    #[serde(flatten)]
    pub incident: Incident,
    #[serde(default)]
    pub updates: Vec<IncidentUpdate>,
}

impl IncidentView {
    pub fn new(incident: Incident, mut updates: Vec<IncidentUpdate>) -> Self {
        updates.sort_by_key(|u| (u.timestamp, u.id));
        Self { incident, updates }
    }

    pub fn id(&self) -> Identifier {
        self.incident.id
    }

    pub fn title(&self) -> &str {
        &self.incident.title
    }

    /// The updates sorted oldest-first (already maintained by [`IncidentView::new`]).
    pub fn sorted_updates(&self) -> &[IncidentUpdate] {
        &self.updates
    }

    /// The incident's current impact: that of its most recent update, or `Hidden` when it has none.
    pub fn current_impact(&self) -> Impact {
        self.updates
            .iter()
            .max_by_key(|u| (u.timestamp, u.id))
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
        for update in self.updates.iter().rev() {
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
/// server assigns the ids and timestamps.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CreateIncident {
    pub title: String,
    pub impact: Impact,
    pub message: String,
}

/// The body for replacing an incident's editable header fields (its title). Applied as a check-and-set
/// against the incident's `version` (sent as an `If-Match` header).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PutIncident {
    pub title: String,
}

/// The body for adding a new update to an incident (the server assigns the id and timestamp).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CreateUpdate {
    pub impact: Impact,
    pub message: String,
}

/// The body for replacing an existing update's editable field (its message). Applied as a
/// check-and-set against the update's `version`; the `impact` is fixed once posted.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PutUpdate {
    pub message: String,
}

/// A page of incidents, newest-first, each carrying its updates. `next_cursor` is the id to pass as
/// `?cursor=` for the following page, or `None` when the last page has been returned.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IncidentsPage {
    pub incidents: Vec<IncidentView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Identifier>,
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

    fn update(incident: Identifier, snow: u64, impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate {
            id: IncidentUpdateId::compose(incident, snow),
            impact,
            timestamp: ts(secs),
            message: format!("update at {secs}"),
            version: (secs * 1000) as u64,
            deleted: false,
        }
    }

    fn view(updates: Vec<IncidentUpdate>) -> IncidentView {
        IncidentView::new(
            Incident { id: Identifier::from(1_234_567u64), title: "Database outage".into(), version: 1, deleted: false },
            updates,
        )
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
    fn view_flattens_incident_and_carries_updates() {
        let id = Identifier::from(1_234_567u64);
        let v = view(vec![update(id, 1, Impact::Offline, 1_700_000_100)]);
        let json = serde_json::to_value(&v).unwrap();
        // The incident fields are flattened alongside the updates array.
        assert_eq!(json["id"], serde_json::Value::String(id.to_string()));
        assert_eq!(json["title"], "Database outage");
        assert_eq!(json["updates"][0]["impact"], "offline");
        assert_eq!(json["updates"][0]["timestamp"], 1_700_000_100);

        let decoded: IncidentView = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn derived_times_and_impact_follow_updates() {
        let id = Identifier::from(1_234_567u64);
        let v = view(vec![
            update(id, 1, Impact::Offline, 1_700_000_100),
            update(id, 2, Impact::None, 1_700_000_500),
        ]);
        assert_eq!(v.current_impact(), Impact::None);
        assert_eq!(v.started_at(), Some(ts(1_700_000_100)));
        assert_eq!(v.last_updated(), Some(ts(1_700_000_500)));
        assert_eq!(v.ended_at(), Some(ts(1_700_000_500)), "a 'none' impact resolves it");
        assert!(v.is_public());
        assert!(!v.is_active());
    }

    #[test]
    fn no_updates_means_hidden_and_inactive() {
        let v = view(vec![]);
        assert_eq!(v.current_impact(), Impact::Hidden);
        assert!(!v.is_public(), "an incident with no updates is a hidden draft");
        assert!(!v.is_active());
        assert_eq!(v.started_at(), None);
        assert_eq!(v.ended_at(), None);
        assert_eq!(v.impact_since(), None);
    }

    #[test]
    fn impact_since_is_the_start_of_the_trailing_run() {
        let id = Identifier::from(1_234_567u64);
        let v = view(vec![
            update(id, 1, Impact::Offline, 100),
            update(id, 2, Impact::Offline, 200),
            update(id, 3, Impact::Degraded, 300),
        ]);
        assert_eq!(v.current_impact(), Impact::Degraded);
        assert_eq!(v.impact_since(), Some(ts(300)));
        assert!(v.is_active());
    }
}
