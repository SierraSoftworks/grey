//! Incident storage: the [`IncidentStore`] trait and its implementation over the [`State`] redb store.
//!
//! Incidents and incident updates are **two separate gossip-replicated entities**, each stored as a
//! single global row keyed by its own snowflake id (incidents by `u64`, updates by `u128` whose high
//! 64 bits are the parent incident id, so an incident's updates are a contiguous key range). Both
//! resolve by whole-entity last-writer-wins (see [`super::GlobalLwwEntity`] and the gossip apply path).
//! Edits are optimistic check-and-set against each entity's `version` (its wall-clock last-modified
//! time in milliseconds, surfaced as the HTTP ETag); deletes are propagating tombstones filtered from
//! all reads.

use std::error::Error;

use chrono::{DateTime, Utc};
use grey_api::{
    CreateIncident, CreateUpdate, Identifier, Incident, IncidentUpdate, IncidentUpdateId,
    IncidentView, IncidentsPage, PutIncident, PutUpdate,
};
use redb::{Durability, ReadableDatabase, ReadableTable, TableDefinition};
use tracing_batteries::prelude::*;

use crate::cluster::Versioned;

use super::{GlobalLwwEntity, INCIDENTS_TABLE, INCIDENT_UPDATES_TABLE, LwwFieldValue, State, WriteError};

/// The default page size for incident listings.
pub const DEFAULT_INCIDENT_PAGE: usize = 20;

/// The maximum serialized size of a single incident-update snapshot. A gossip datagram is not
/// fragmented per-entity, so an update larger than this could fail to replicate; reject it at ingest.
/// Comfortably below the UDP `MAX_DATAGRAM_SIZE` (65507) to leave room for encryption + framing.
pub const MAX_UPDATE_BYTES: usize = 48 * 1024;

/// Builds a snowflake half: the low 32 bits of the unix-second timestamp in the high half, 32 random
/// bits in the low half. Time-ordered (to the second) and collision-resistant without coordination.
fn snowflake_half(now: DateTime<Utc>) -> u64 {
    let secs = (now.timestamp() as u64) & 0xFFFF_FFFF;
    (secs << 32) | (rand::random::<u32>() as u64)
}

/// The result of a check-and-set mutation. `Updated` carries the mutated entity's new `version` (for
/// the response ETag) and a payload (the refreshed [`IncidentView`], or `()` for deletes).
pub enum CasOutcome<T> {
    Updated(u64, T),
    /// The supplied version did not match; carries the current stored version.
    VersionMismatch(u64),
    /// No (live) entity exists with the given id.
    NotFound,
}

impl Versioned for Incident {
    type Diff = Incident;

    fn version(&self) -> u64 {
        self.version
    }

    fn diff(&self, version: u64) -> Option<Self::Diff> {
        if self.version() > version {
            Some(self.clone())
        } else {
            None
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        if diff.version() > self.version() {
            *self = diff.clone();
        }
    }
}

impl GlobalLwwEntity for Incident {
    type Key = u64;
    const TABLE: TableDefinition<'static, u64, LwwFieldValue> = INCIDENTS_TABLE;

    fn id_field(&self) -> String {
        self.id.to_string()
    }
}

impl Versioned for IncidentUpdate {
    type Diff = IncidentUpdate;

    fn version(&self) -> u64 {
        self.version
    }

    fn diff(&self, version: u64) -> Option<Self::Diff> {
        if self.version() > version {
            Some(self.clone())
        } else {
            None
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        if diff.version() > self.version() {
            *self = diff.clone();
        }
    }
}

impl GlobalLwwEntity for IncidentUpdate {
    type Key = u128;
    const TABLE: TableDefinition<'static, u128, LwwFieldValue> = INCIDENT_UPDATES_TABLE;

    fn id_field(&self) -> String {
        self.id.to_string()
    }
}

/// Storage operations for incidents and their updates (cluster-replicated, gossiped global records).
#[allow(async_fn_in_trait)]
pub trait IncidentStore {
    /// A page of incidents, newest-first by id, each joined with its visible updates. When
    /// `include_hidden` is false, incidents whose current impact is hidden are omitted (the public
    /// view). `cursor` continues a previous page (incidents with an id strictly below it).
    async fn list_incidents(
        &self,
        include_hidden: bool,
        limit: usize,
        cursor: Option<Identifier>,
    ) -> Result<IncidentsPage, Box<dyn Error>>;

    /// Fetches a single (live) incident with its visible updates.
    async fn get_incident(&self, id: Identifier) -> Result<Option<IncidentView>, Box<dyn Error>>;

    /// Creates an incident with a single opening update, minting snowflake ids for both. A
    /// `timestamp` on the input backdates the opening update, and with it the incident's id — so a
    /// retroactively declared outage sorts into the (id-ordered) list at the time it began rather
    /// than at the time it was declared.
    async fn create_incident(&self, input: CreateIncident)
    -> Result<IncidentView, Box<dyn Error>>;

    /// Replaces an incident's editable header fields (its title) if `expected_version` matches.
    async fn put_incident(
        &self,
        id: Identifier,
        expected_version: u64,
        edit: PutIncident,
    ) -> Result<CasOutcome<IncidentView>, Box<dyn Error>>;

    /// Tombstones an incident if `expected_version` matches.
    async fn delete_incident(
        &self,
        id: Identifier,
        expected_version: u64,
    ) -> Result<CasOutcome<()>, Box<dyn Error>>;

    /// Adds a new update to an existing incident, backdated to the input's `timestamp` when it carries
    /// one. `None` when the incident does not exist (a 404). The returned tuple is `(new update
    /// version, refreshed view)`.
    async fn create_update(
        &self,
        incident_id: Identifier,
        update: CreateUpdate,
    ) -> Result<Option<(u64, IncidentView)>, Box<dyn Error>>;

    /// Replaces an update's editable fields (its message, plus its timestamp when the edit supplies
    /// one) if `expected_version` matches.
    async fn put_update(
        &self,
        update_id: IncidentUpdateId,
        expected_version: u64,
        edit: PutUpdate,
    ) -> Result<CasOutcome<IncidentView>, Box<dyn Error>>;

    /// Tombstones an update if `expected_version` matches.
    async fn delete_update(
        &self,
        update_id: IncidentUpdateId,
        expected_version: u64,
    ) -> Result<CasOutcome<()>, Box<dyn Error>>;
}

/// Loads a live incident and its visible (non-tombstoned) updates within a read transaction.
fn read_view(
    txn: &redb::ReadTransaction,
    id: Identifier,
) -> Result<Option<IncidentView>, Box<dyn Error>> {
    let key = u64::from(id);
    let incident = match txn.open_table(INCIDENTS_TABLE) {
        Ok(table) => match table.get(key)? {
            Some(value) => {
                let (_v, _w, data) = value.value();
                let incident: Incident = rmp_serde::from_slice(data)?;
                if incident.deleted {
                    return Ok(None);
                }
                incident
            }
            None => return Ok(None),
        },
        Err(_) => return Ok(None),
    };

    let mut updates = Vec::new();
    if let Ok(table) = txn.open_table(INCIDENT_UPDATES_TABLE) {
        // Every update for this incident shares the high 64 bits of its u128 id.
        let lo = (key as u128) << 64;
        let hi = ((key as u128) + 1) << 64;
        for entry in table.range(lo..hi)?.filter_map(|r| r.ok()) {
            let (_k, value) = entry;
            let (_v, _w, data) = value.value();
            if let Ok(update) = rmp_serde::from_slice::<IncidentUpdate>(data) {
                if !update.deleted {
                    updates.push(update);
                }
            }
        }
    }

    Ok(Some(IncidentView::new(incident, updates)))
}

impl State {
    /// Reads the stored incident (including a tombstoned one) for a CAS, returning its current version.
    fn load_incident(
        table: &impl ReadableTable<u64, LwwFieldValue>,
        id: Identifier,
    ) -> Result<Option<Incident>, WriteError> {
        Ok(table
            .get(u64::from(id))?
            .map(|value| {
                let (_v, _w, data) = value.value();
                rmp_serde::from_slice::<Incident>(data)
            })
            .transpose()?)
    }

    fn load_update(
        table: &impl ReadableTable<u128, LwwFieldValue>,
        id: IncidentUpdateId,
    ) -> Result<Option<IncidentUpdate>, WriteError> {
        Ok(table
            .get(u128::from(id))?
            .map(|value| {
                let (_v, _w, data) = value.value();
                rmp_serde::from_slice::<IncidentUpdate>(data)
            })
            .transpose()?)
    }
}

impl IncidentStore for State {
    async fn list_incidents(
        &self,
        include_hidden: bool,
        limit: usize,
        cursor: Option<Identifier>,
    ) -> Result<IncidentsPage, Box<dyn Error>> {
        let limit = limit.max(1);
        let txn = self.database.begin_read()?;

        let mut incidents: Vec<IncidentView> = Vec::new();
        let mut next_cursor = None;

        if let Ok(table) = txn.open_table(INCIDENTS_TABLE) {
            // Newest-first = descending id. A cursor continues with ids strictly below it.
            let upper = cursor.map(u64::from);
            let iter = match upper {
                Some(c) => table.range(..c)?,
                None => table.range::<u64>(..)?,
            };
            for entry in iter.rev().filter_map(|r| r.ok()) {
                let (key, value) = entry;
                let id = Identifier::from(key.value());
                let (_v, _w, data) = value.value();
                let incident: Incident = match rmp_serde::from_slice(data) {
                    Ok(i) => i,
                    Err(err) => {
                        warn!("Skipping an incident that failed to deserialize: {err:?}");
                        continue;
                    }
                };
                if incident.deleted {
                    continue;
                }
                let Some(view) = read_view(&txn, id)? else { continue };
                if !include_hidden && !view.is_public() {
                    continue;
                }
                if incidents.len() == limit {
                    // We already have a full page and found one more match — there is a next page.
                    next_cursor = incidents.last().map(|v| v.id());
                    break;
                }
                incidents.push(view);
            }
        }

        Ok(IncidentsPage { incidents, next_cursor })
    }

    async fn get_incident(&self, id: Identifier) -> Result<Option<IncidentView>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        read_view(&txn, id)
    }

    async fn create_incident(&self, input: CreateIncident) -> Result<IncidentView, Box<dyn Error>> {
        let now = Utc::now();
        // `version` is the gossip/ETag clock and always tracks wall-clock now; `started` is when the
        // event itself began, which an operator may backdate.
        let version = now.timestamp_millis() as u64;
        let started = input.timestamp.unwrap_or(now);
        let incident_id = Identifier::from(snowflake_half(started));
        let update = IncidentUpdate {
            id: IncidentUpdateId::compose(incident_id, snowflake_half(started)),
            impact: input.impact,
            timestamp: started,
            message: input.message,
            version,
            deleted: false,
        };
        let incident = Incident { id: incident_id, title: input.title, version, deleted: false };

        let update_bytes = rmp_serde::to_vec_named(&update)?;
        if update_bytes.len() > MAX_UPDATE_BYTES {
            return Err("incident update message is too large".into());
        }
        let own: u128 = self.node_id.into();

        let incident_bytes = rmp_serde::to_vec_named(&incident)?;
        let update_key = u128::from(update.id);
        self.write("create_incident", Durability::Immediate, move |txn| {
            {
                let mut incidents = txn.open_table(INCIDENTS_TABLE)?;
                incidents.insert(u64::from(incident_id), (version, own, incident_bytes.as_slice()))?;
            }
            {
                let mut updates = txn.open_table(INCIDENT_UPDATES_TABLE)?;
                updates.insert(update_key, (version, own, update_bytes.as_slice()))?;
            }
            Ok(())
        })
        .await?;

        Ok(IncidentView::new(incident, vec![update]))
    }

    async fn put_incident(
        &self,
        id: Identifier,
        expected_version: u64,
        edit: PutIncident,
    ) -> Result<CasOutcome<IncidentView>, Box<dyn Error>> {
        let own: u128 = self.node_id.into();
        let new_version = Utc::now().timestamp_millis() as u64;

        let outcome = self.write("put_incident", Durability::Immediate, move |txn| {
            let mut incidents = txn.open_table(INCIDENTS_TABLE)?;
            Ok(match Self::load_incident(&incidents, id)? {
                None => CasOutcome::NotFound,
                Some(existing) if existing.deleted => CasOutcome::NotFound,
                Some(existing) if existing.version != expected_version => {
                    CasOutcome::VersionMismatch(existing.version)
                }
                Some(_existing) => {
                    let updated = Incident { id, title: edit.title, version: new_version, deleted: false };
                    incidents.insert(u64::from(id), (new_version, own, rmp_serde::to_vec_named(&updated)?.as_slice()))?;
                    CasOutcome::Updated(new_version, ())
                }
            })
        })
        .await?;

        match outcome {
            CasOutcome::Updated(version, ()) => {
                let view = self.get_incident(id).await?.ok_or("incident vanished after update")?;
                Ok(CasOutcome::Updated(version, view))
            }
            CasOutcome::VersionMismatch(v) => Ok(CasOutcome::VersionMismatch(v)),
            CasOutcome::NotFound => Ok(CasOutcome::NotFound),
        }
    }

    async fn delete_incident(
        &self,
        id: Identifier,
        expected_version: u64,
    ) -> Result<CasOutcome<()>, Box<dyn Error>> {
        let own: u128 = self.node_id.into();
        let new_version = Utc::now().timestamp_millis() as u64;

        self.write("delete_incident", Durability::Immediate, move |txn| {
            let mut incidents = txn.open_table(INCIDENTS_TABLE)?;
            Ok(match Self::load_incident(&incidents, id)? {
                None => CasOutcome::NotFound,
                Some(existing) if existing.deleted => CasOutcome::NotFound,
                Some(existing) if existing.version != expected_version => {
                    CasOutcome::VersionMismatch(existing.version)
                }
                Some(mut existing) => {
                    existing.deleted = true;
                    existing.version = new_version;
                    incidents.insert(u64::from(id), (new_version, own, rmp_serde::to_vec_named(&existing)?.as_slice()))?;
                    CasOutcome::Updated(new_version, ())
                }
            })
        })
        .await
    }

    async fn create_update(
        &self,
        incident_id: Identifier,
        update: CreateUpdate,
    ) -> Result<Option<(u64, IncidentView)>, Box<dyn Error>> {
        let now = Utc::now();
        let version = now.timestamp_millis() as u64;
        // An update may record a moment that has already passed (when mitigation completed, say)
        // rather than the moment it is posted.
        let occurred = update.timestamp.unwrap_or(now);
        let new_update = IncidentUpdate {
            id: IncidentUpdateId::compose(incident_id, snowflake_half(occurred)),
            impact: update.impact,
            timestamp: occurred,
            message: update.message,
            version,
            deleted: false,
        };
        let update_bytes = rmp_serde::to_vec_named(&new_update)?;
        if update_bytes.len() > MAX_UPDATE_BYTES {
            return Err("incident update message is too large".into());
        }
        let own: u128 = self.node_id.into();

        let update_key = u128::from(new_update.id);
        let inserted = self.write("create_update", Durability::Immediate, move |txn| {
            {
                // The incident must exist and be live.
                let incidents = txn.open_table(INCIDENTS_TABLE)?;
                match Self::load_incident(&incidents, incident_id)? {
                    Some(incident) if !incident.deleted => {}
                    _ => return Ok(false),
                }
            }
            let mut updates = txn.open_table(INCIDENT_UPDATES_TABLE)?;
            updates.insert(update_key, (version, own, update_bytes.as_slice()))?;
            Ok(true)
        })
        .await?;
        if !inserted {
            return Ok(None);
        }

        let view = self.get_incident(incident_id).await?.ok_or("incident vanished after adding update")?;
        Ok(Some((version, view)))
    }

    async fn put_update(
        &self,
        update_id: IncidentUpdateId,
        expected_version: u64,
        edit: PutUpdate,
    ) -> Result<CasOutcome<IncidentView>, Box<dyn Error>> {
        let own: u128 = self.node_id.into();
        let new_version = Utc::now().timestamp_millis() as u64;

        let outcome = self.write("put_update", Durability::Immediate, move |txn| {
            let mut updates = txn.open_table(INCIDENT_UPDATES_TABLE)?;
            Ok(match Self::load_update(&updates, update_id)? {
                None => CasOutcome::NotFound,
                Some(existing) if existing.deleted => CasOutcome::NotFound,
                Some(existing) if existing.version != expected_version => {
                    CasOutcome::VersionMismatch(existing.version)
                }
                Some(mut existing) => {
                    // `impact` is fixed once posted; the message is editable, and so is the timestamp
                    // (a backdated update recorded at the wrong moment can be corrected) whenever the
                    // edit supplies one.
                    existing.message = edit.message;
                    if let Some(timestamp) = edit.timestamp {
                        existing.timestamp = timestamp;
                    }
                    existing.version = new_version;
                    updates.insert(u128::from(update_id), (new_version, own, rmp_serde::to_vec_named(&existing)?.as_slice()))?;
                    CasOutcome::Updated(new_version, ())
                }
            })
        })
        .await?;

        match outcome {
            CasOutcome::Updated(version, ()) => {
                match self.get_incident(update_id.incident_id()).await? {
                    Some(view) => Ok(CasOutcome::Updated(version, view)),
                    // The parent incident was deleted concurrently; the update edit still applied.
                    None => Ok(CasOutcome::NotFound),
                }
            }
            CasOutcome::VersionMismatch(v) => Ok(CasOutcome::VersionMismatch(v)),
            CasOutcome::NotFound => Ok(CasOutcome::NotFound),
        }
    }

    async fn delete_update(
        &self,
        update_id: IncidentUpdateId,
        expected_version: u64,
    ) -> Result<CasOutcome<()>, Box<dyn Error>> {
        let own: u128 = self.node_id.into();
        let new_version = Utc::now().timestamp_millis() as u64;

        self.write("delete_update", Durability::Immediate, move |txn| {
            let mut updates = txn.open_table(INCIDENT_UPDATES_TABLE)?;
            Ok(match Self::load_update(&updates, update_id)? {
                None => CasOutcome::NotFound,
                Some(existing) if existing.deleted => CasOutcome::NotFound,
                Some(existing) if existing.version != expected_version => {
                    CasOutcome::VersionMismatch(existing.version)
                }
                Some(mut existing) => {
                    existing.deleted = true;
                    existing.version = new_version;
                    updates.insert(u128::from(update_id), (new_version, own, rmp_serde::to_vec_named(&existing)?.as_slice()))?;
                    CasOutcome::Updated(new_version, ())
                }
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::Impact;

    /// A whole-second timestamp `hours` in the past. Update timestamps ride the wire (and the gossip
    /// snapshot) as unix seconds, so tests that compare a stored time use second precision.
    fn hours_ago(hours: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(Utc::now().timestamp() - hours * 3600, 0).unwrap()
    }

    /// The opening update for a freshly declared (not backdated) incident.
    fn opening(title: &str, impact: Impact, message: &str) -> CreateIncident {
        CreateIncident { title: title.into(), impact, message: message.into(), timestamp: None }
    }

    async fn test_state(dir: &std::path::Path) -> State {
        State::test(dir.to_path_buf()).await
    }

    #[tokio::test]
    async fn create_get_list_and_visibility() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        let public = state.create_incident(opening("Public", Impact::Offline, "down")).await.unwrap();
        let hidden = state.create_incident(opening("Hidden", Impact::Hidden, "draft")).await.unwrap();

        assert_ne!(public.id(), hidden.id());
        assert_eq!(public.updates.len(), 1);
        assert!(public.is_public());
        assert!(!hidden.is_public());

        // Round-trips by id (admin get returns hidden too).
        assert_eq!(state.get_incident(public.id()).await.unwrap().unwrap().title(), "Public");

        // The public view hides the hidden incident; the admin view shows both.
        let visible = state.list_incidents(false, 10, None).await.unwrap();
        assert_eq!(visible.incidents.iter().map(|v| v.title().to_string()).collect::<Vec<_>>(), vec!["Public"]);
        assert_eq!(state.list_incidents(true, 10, None).await.unwrap().incidents.len(), 2);
    }

    #[tokio::test]
    async fn an_incident_can_be_declared_after_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        // An outage noticed late: declared now, but recorded as having started two hours ago.
        let started = hours_ago(2);
        let backdated = state
            .create_incident(CreateIncident {
                title: "Backdated".into(),
                impact: Impact::Offline,
                message: "down since 2h ago".into(),
                timestamp: Some(started),
            })
            .await
            .unwrap();
        assert_eq!(backdated.started_at(), Some(started));

        // Its id is minted from the start time, so it sorts into the (newest-first) list where the
        // event actually happened rather than at the top.
        let fresh = state.create_incident(opening("Fresh", Impact::Offline, "down now")).await.unwrap();
        let listed = state.list_incidents(true, 10, None).await.unwrap();
        assert_eq!(
            listed.incidents.iter().map(|v| v.id()).collect::<Vec<_>>(),
            vec![fresh.id(), backdated.id()],
            "the incident declared for two hours ago sorts behind the one starting now"
        );

        // Its gossip/ETag version is still wall-clock now, not the backdated time — replication and
        // check-and-set must not be dragged into the past.
        assert!(backdated.incident.version as i64 >= started.timestamp_millis() + 3_600_000);
    }

    #[tokio::test]
    async fn updates_can_be_recorded_at_a_past_moment_and_corrected() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        let started = hours_ago(3);
        let created = state
            .create_incident(CreateIncident {
                title: "Outage".into(),
                impact: Impact::Offline,
                message: "down".into(),
                timestamp: Some(started),
            })
            .await
            .unwrap();

        // Mitigation completed an hour ago — record it at that moment, not at the moment we post it.
        let mitigated = hours_ago(1);
        let (version, view) = state
            .create_update(
                created.id(),
                CreateUpdate {
                    impact: Impact::None,
                    message: "mitigated".into(),
                    timestamp: Some(mitigated),
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(view.current_impact(), Impact::None);
        assert_eq!(view.ended_at(), Some(mitigated), "the incident ended when mitigation completed");
        assert_eq!(view.started_at(), Some(started));

        // The time was an hour out: correct it, leaving the message alone.
        let corrected = hours_ago(2);
        let outcome = state
            .put_update(
                view.updates.last().unwrap().id,
                version,
                PutUpdate { message: "mitigated".into(), timestamp: Some(corrected) },
            )
            .await
            .unwrap();
        assert!(matches!(outcome, CasOutcome::Updated(..)));
        let view = state.get_incident(created.id()).await.unwrap().unwrap();
        assert_eq!(view.ended_at(), Some(corrected));

        // An edit without a timestamp leaves the (corrected) one in place.
        let latest = view.updates.last().unwrap();
        let outcome = state
            .put_update(
                latest.id,
                latest.version,
                PutUpdate { message: "mitigation complete".into(), timestamp: None },
            )
            .await
            .unwrap();
        assert!(matches!(outcome, CasOutcome::Updated(..)));
        let view = state.get_incident(created.id()).await.unwrap().unwrap();
        assert_eq!(view.ended_at(), Some(corrected));
        assert_eq!(view.updates.last().unwrap().message, "mitigation complete");
    }

    #[tokio::test]
    async fn put_and_delete_are_check_and_set() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        let created = state.create_incident(opening("Outage", Impact::Offline, "down")).await.unwrap();
        let v0 = created.incident.version;

        // Correct version -> updated + bumped version.
        let outcome = state.put_incident(created.id(), v0, PutIncident { title: "Outage (edited)".into() }).await.unwrap();
        let v1 = match outcome { CasOutcome::Updated(v, view) => { assert_eq!(view.title(), "Outage (edited)"); v }, _ => panic!("expected Updated") };
        assert!(v1 >= v0);

        // Stale version -> conflict reporting the current version.
        assert!(matches!(
            state.put_incident(created.id(), v0, PutIncident { title: "x".into() }).await.unwrap(),
            CasOutcome::VersionMismatch(v) if v == v1
        ));

        // Delete tombstones it: it disappears from reads and a re-delete is NotFound.
        assert!(matches!(state.delete_incident(created.id(), v1).await.unwrap(), CasOutcome::Updated(_, ())));
        assert!(state.get_incident(created.id()).await.unwrap().is_none());
        assert!(matches!(state.delete_incident(created.id(), v1).await.unwrap(), CasOutcome::NotFound));
    }

    #[tokio::test]
    async fn updates_are_independent_entities() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        let created = state.create_incident(opening("Outage", Impact::Offline, "down")).await.unwrap();
        let incident_version = created.incident.version;

        // Adding an update does NOT bump the incident entity's version (they are separate entities).
        let (uv, view) = state.create_update(created.id(), CreateUpdate { impact: Impact::None, message: "fixed".into(), timestamp: None }).await.unwrap().unwrap();
        assert_eq!(view.updates.len(), 2);
        assert!(
            view.updates.iter().any(|u| u.impact == Impact::Offline)
                && view.updates.iter().any(|u| u.impact == Impact::None),
            "both updates present"
        );
        assert_eq!(view.incident.version, incident_version, "incident version unchanged by an added update");

        // The new (None) update has its own version and is editable by CAS.
        let new_update = view.updates.iter().find(|u| u.impact == Impact::None).unwrap().clone();
        assert_eq!(new_update.version, uv);
        let edited = state.put_update(new_update.id, uv, PutUpdate { message: "resolved".into(), timestamp: None }).await.unwrap();
        match edited {
            CasOutcome::Updated(_, v) => {
                let msg = v.updates.iter().find(|u| u.id == new_update.id).unwrap().message.clone();
                assert_eq!(msg, "resolved");
            }
            _ => panic!("expected Updated"),
        }

        // Removing an update tombstones it: it drops out of the view.
        assert!(matches!(state.delete_update(new_update.id, _latest_version(&state, created.id(), new_update.id).await).await.unwrap(), CasOutcome::Updated(_, ())));
        let after = state.get_incident(created.id()).await.unwrap().unwrap();
        assert_eq!(after.updates.len(), 1);
        assert_eq!(after.current_impact(), Impact::Offline);

        // Adding an update to a missing incident is a 404 (None).
        assert!(state.create_update(Identifier::from(424242u64), CreateUpdate { impact: Impact::None, message: "x".into(), timestamp: None }).await.unwrap().is_none());
    }

    // Helper: the current stored version of an update (it bumps on each edit).
    async fn _latest_version(state: &State, incident: Identifier, update: IncidentUpdateId) -> u64 {
        state.get_incident(incident).await.unwrap().unwrap()
            .updates.iter().find(|u| u.id == update).unwrap().version
    }

    #[tokio::test]
    async fn pagination_walks_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        // Create several incidents; their snowflake ids are time-ordered.
        let mut ids = Vec::new();
        for i in 0..5 {
            let v = state.create_incident(opening(&format!("Incident {i}"), Impact::Offline, "x")).await.unwrap();
            ids.push(v.id());
        }

        let page1 = state.list_incidents(true, 2, None).await.unwrap();
        assert_eq!(page1.incidents.len(), 2);
        assert!(page1.next_cursor.is_some());

        let page2 = state.list_incidents(true, 2, page1.next_cursor).await.unwrap();
        assert_eq!(page2.incidents.len(), 2);

        // No overlap between pages, and ids descend across the boundary.
        let p1_last = page1.incidents.last().unwrap().id();
        let p2_first = page2.incidents.first().unwrap().id();
        assert!(u64::from(p2_first) < u64::from(p1_last));
    }

    /// An incident and its update received from a peer through the gossip apply path surface in the
    /// view even on a node with no local writes.
    #[tokio::test]
    async fn incident_replicates_through_gossip_apply() {
        use crate::cluster::{ClusterStateDiff, GossipStore, NodeID};
        use crate::state::ReplicatedEntity;

        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        let now = Utc::now();
        let version = now.timestamp_millis() as u64;
        let id = Identifier::from(0x6000_0000_0000_0001u64);
        let incident = Incident { id, title: "Replicated".into(), version, deleted: false };
        let update = IncidentUpdate {
            id: IncidentUpdateId::compose(id, 1),
            impact: Impact::Degraded,
            timestamp: now,
            message: "from-peer".into(),
            version,
            deleted: false,
        };

        let peer = NodeID::new();
        let mut diff = ClusterStateDiff::new();
        diff.update(peer, incident.id_field(), ReplicatedEntity::Incident(incident));
        diff.update(peer, update.id_field(), ReplicatedEntity::IncidentUpdate(update));
        state.apply(diff).await.unwrap();

        let view = state.get_incident(id).await.unwrap().expect("the gossiped incident surfaces");
        assert_eq!(view.title(), "Replicated");
        assert_eq!(view.updates.len(), 1);
        assert_eq!(view.current_impact(), Impact::Degraded);
    }

    /// Last-writer-wins on apply: a higher-versioned record replaces, a stale one is ignored.
    #[tokio::test]
    async fn apply_is_last_writer_wins() {
        use crate::cluster::{ClusterStateDiff, GossipStore, NodeID};
        use crate::state::ReplicatedEntity;

        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        let created = state.create_incident(opening("Original", Impact::Offline, "x")).await.unwrap();
        let id = created.id();
        let base = created.incident.version;

        // A stale peer edit (older version) must not win.
        let stale = Incident { id, title: "Stale".into(), version: base - 1_000, deleted: false };
        let mut diff = ClusterStateDiff::new();
        diff.update(NodeID::new(), stale.id_field(), ReplicatedEntity::Incident(stale));
        state.apply(diff).await.unwrap();
        assert_eq!(state.get_incident(id).await.unwrap().unwrap().title(), "Original");

        // A newer peer edit wins.
        let newer = Incident { id, title: "Newer".into(), version: base + 1_000, deleted: false };
        let mut diff = ClusterStateDiff::new();
        diff.update(NodeID::new(), newer.id_field(), ReplicatedEntity::Incident(newer));
        state.apply(diff).await.unwrap();
        assert_eq!(state.get_incident(id).await.unwrap().unwrap().title(), "Newer");
    }

    /// Equal-version writes from two nodes converge on the same record regardless of apply order: the
    /// `(version, last_writer)` total order breaks the tie deterministically (higher node id wins).
    #[tokio::test]
    async fn apply_breaks_version_ties_by_writer() {
        use crate::cluster::{ClusterStateDiff, GossipStore, NodeID};
        use crate::state::ReplicatedEntity;

        let id = Identifier::from(0x7000_0000_0000_0001u64);
        let version = 1_700_000_000_000u64;
        let low = NodeID::from(1u128);
        let high = NodeID::from(2u128);
        let from_low = Incident { id, title: "low".into(), version, deleted: false };
        let from_high = Incident { id, title: "high".into(), version, deleted: false };

        // Apply low-then-high on one node and high-then-low on another: both converge on the higher
        // writer's record.
        for order in [[low, high], [high, low]] {
            let dir = tempfile::tempdir().unwrap();
            let state = test_state(dir.path()).await;
            for writer in order {
                let incident = if writer == high { from_high.clone() } else { from_low.clone() };
                let mut diff = ClusterStateDiff::new();
                diff.update(writer, incident.id_field(), ReplicatedEntity::Incident(incident));
                state.apply(diff).await.unwrap();
            }
            assert_eq!(
                state.get_incident(id).await.unwrap().unwrap().title(),
                "high",
                "the higher writer id wins an equal-version tie regardless of order"
            );
        }
    }

    /// A full gossip round (digest → diff → apply) carries an incident and its update from the node
    /// that authored them to a peer, and that peer relays them onward to a third node under the
    /// original writer's partition (the masking-regression guard).
    #[tokio::test]
    async fn incident_syncs_through_a_full_gossip_round() {
        use crate::cluster::GossipStore;

        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let a = test_state(dir_a.path()).await;
        let b = test_state(dir_b.path()).await;

        let created = a.create_incident(opening("Round", Impact::Offline, "down")).await.unwrap();
        a.create_update(created.id(), CreateUpdate { impact: Impact::None, message: "up".into(), timestamp: None }).await.unwrap();

        // B pulls from A: A diffs against B's digest, B applies the delta.
        let delta = a.diff(b.digest().await.unwrap()).await.unwrap();
        b.apply(delta).await.unwrap();

        let view = b.get_incident(created.id()).await.unwrap().expect("incident replicated to B");
        assert_eq!(view.title(), "Round");
        assert_eq!(view.updates.len(), 2);

        // B relays A's records onward to a fresh node C (advertised under A's partition), so anti-
        // entropy completes transitively rather than only between the original author and a peer.
        let dir_c = tempfile::tempdir().unwrap();
        let c = test_state(dir_c.path()).await;
        let delta = b.diff(c.digest().await.unwrap()).await.unwrap();
        c.apply(delta).await.unwrap();
        assert_eq!(
            c.get_incident(created.id()).await.unwrap().unwrap().updates.len(),
            2,
            "B relays A's incident + update to C"
        );
    }

    /// Incidents are persistent records: unlike probes and crons, GC never ages them out, so the
    /// outage history survives indefinitely no matter how old a record is.
    #[tokio::test]
    async fn gc_keeps_incidents_indefinitely() {
        use crate::cluster::{ClusterStateDiff, GossipStore, NodeID};
        use crate::state::{ProbeStore, ReplicatedEntity};

        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;

        // A fresh, locally-created incident.
        let fresh = state.create_incident(opening("Fresh", Impact::Offline, "x")).await.unwrap();

        // An incident whose version is far older than any probe/cron expiry, delivered via gossip.
        let aged_id = Identifier::from(0x7000_0000_0000_0002u64);
        let aged_version = (chrono::Utc::now() - chrono::Duration::days(400)).timestamp_millis() as u64;
        let aged = Incident { id: aged_id, title: "Aged".into(), version: aged_version, deleted: false };
        let mut diff = ClusterStateDiff::new();
        diff.update(NodeID::new(), aged.id_field(), ReplicatedEntity::Incident(aged));
        state.apply(diff).await.unwrap();

        state.gc().await.unwrap();

        // Both survive: GC does not age out incident records (a non-deleted aged row remaining proves
        // the incident table is left untouched by the sweep).
        assert!(state.get_incident(fresh.id()).await.unwrap().is_some(), "GC keeps the fresh incident");
        assert!(
            state.get_incident(aged_id).await.unwrap().is_some(),
            "GC keeps an arbitrarily old incident (persistent record)"
        );
    }
}
