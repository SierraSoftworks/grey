//! Incident storage: the [`IncidentStore`] trait and its implementation over the [`State`] redb
//! store, kept separate from the underlying store itself. Incidents are keyed by the numeric value of
//! their [`Identifier`] and serialized as JSON. Edits are applied as optimistic check-and-set
//! operations against the incident's `version`.

use std::error::Error;

use grey_api::{Identifier, Incident, IncidentEdit, IncidentUpdate};
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use tracing_batteries::prelude::*;

use super::State;

const INCIDENTS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("incidents");

/// The result of a check-and-set [`IncidentStore::replace_incident`].
pub enum CasOutcome {
    /// The incident was replaced; carries the new incident (with its bumped version).
    Updated(Incident),
    /// The supplied version did not match; carries the current stored version.
    VersionMismatch(u64),
    /// No incident exists with the given id.
    NotFound,
}

/// Storage operations for incidents.
#[allow(async_fn_in_trait)]
pub trait IncidentStore {
    /// Lists stored incidents, most recently started first. When `include_hidden` is false, incidents
    /// whose current impact is hidden are omitted — the public, unauthenticated view.
    async fn list_incidents(&self, include_hidden: bool) -> Result<Vec<Incident>, Box<dyn Error>>;

    /// Fetches a single incident by id.
    async fn get_incident(&self, id: Identifier) -> Result<Option<Incident>, Box<dyn Error>>;

    /// Creates an incident with a single opening update, assigning a fresh unused id and version 1.
    async fn create_incident(
        &self,
        title: String,
        initial: IncidentUpdate,
    ) -> Result<Incident, Box<dyn Error>>;

    /// Replaces an incident's title and updates if `expected_version` matches the stored version,
    /// bumping the version on success (optimistic concurrency).
    async fn replace_incident(
        &self,
        id: Identifier,
        expected_version: u64,
        edit: IncidentEdit,
    ) -> Result<CasOutcome, Box<dyn Error>>;

    /// Deletes an incident, returning whether a record was actually removed.
    async fn delete_incident(&self, id: Identifier) -> Result<bool, Box<dyn Error>>;
}

impl IncidentStore for State {
    async fn list_incidents(&self, include_hidden: bool) -> Result<Vec<Incident>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        let mut incidents = Vec::new();
        if let Ok(table) = txn.open_table(INCIDENTS_TABLE) {
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (_key, value) = entry;
                match serde_json::from_slice::<Incident>(value.value()) {
                    Ok(incident) if include_hidden || incident.is_public() => {
                        incidents.push(incident)
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!("Skipping an incident record that failed to deserialize: {err:?}")
                    }
                }
            }
        }
        // Most recently active first; incidents with no updates sort last.
        incidents.sort_by(|a, b| b.last_updated().cmp(&a.last_updated()));
        Ok(incidents)
    }

    async fn get_incident(&self, id: Identifier) -> Result<Option<Incident>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        if let Ok(table) = txn.open_table(INCIDENTS_TABLE)
            && let Some(value) = table.get(u64::from(id))?
        {
            return Ok(Some(serde_json::from_slice::<Incident>(value.value())?));
        }
        Ok(None)
    }

    async fn create_incident(
        &self,
        title: String,
        initial: IncidentUpdate,
    ) -> Result<Incident, Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        let incident = {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;

            let mut key = rand::random::<u64>();
            let mut attempts = 0;
            while table.get(key)?.is_some() {
                key = rand::random::<u64>();
                attempts += 1;
                if attempts > 1000 {
                    return Err("could not allocate a unique incident id".into());
                }
            }

            let incident = Incident {
                id: Identifier::from(key),
                title,
                version: 1,
                updates: vec![initial],
            };
            table.insert(key, serde_json::to_vec(&incident)?.as_slice())?;
            incident
        };
        txn.commit()?;
        Ok(incident)
    }

    async fn replace_incident(
        &self,
        id: Identifier,
        expected_version: u64,
        edit: IncidentEdit,
    ) -> Result<CasOutcome, Box<dyn Error>> {
        let key = u64::from(id);
        let txn = self.database.begin_write()?;
        let outcome = {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            let existing: Option<Incident> = table
                .get(key)?
                .map(|value| serde_json::from_slice::<Incident>(value.value()))
                .transpose()?;

            match existing {
                None => CasOutcome::NotFound,
                Some(existing) if existing.version != expected_version => {
                    CasOutcome::VersionMismatch(existing.version)
                }
                Some(existing) => {
                    let updated = Incident {
                        id,
                        title: edit.title,
                        version: existing.version + 1,
                        updates: edit.updates,
                    };
                    table.insert(key, serde_json::to_vec(&updated)?.as_slice())?;
                    CasOutcome::Updated(updated)
                }
            }
        };
        txn.commit()?;
        Ok(outcome)
    }

    async fn delete_incident(&self, id: Identifier) -> Result<bool, Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        let existed = {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            table.remove(u64::from(id))?.is_some()
        };
        txn.commit()?;
        Ok(existed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::{IncidentEdit, Impact};

    fn update(impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate {
            impact,
            timestamp: chrono::DateTime::from_timestamp(secs, 0).unwrap(),
            message: "update".into(),
        }
    }

    #[tokio::test]
    async fn create_get_list_delete_and_visibility() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        let public = state.create_incident("Public".into(), update(Impact::Offline, 200)).await.unwrap();
        let hidden = state.create_incident("Hidden".into(), update(Impact::Hidden, 100)).await.unwrap();

        // Fresh ids are distinct and version starts at 1.
        assert_ne!(public.id, hidden.id);
        assert_eq!(public.version, 1);

        // Round-trips by id.
        assert_eq!(state.get_incident(public.id).await.unwrap().unwrap().title, "Public");

        // The public view hides the hidden incident; the admin view shows both.
        let visible = state.list_incidents(false).await.unwrap();
        assert_eq!(visible.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["Public"]);
        assert_eq!(state.list_incidents(true).await.unwrap().len(), 2);

        assert!(state.delete_incident(public.id).await.unwrap());
        assert!(!state.delete_incident(public.id).await.unwrap());
        assert!(state.get_incident(public.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn replace_is_check_and_set() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        let incident = state.create_incident("Outage".into(), update(Impact::Offline, 100)).await.unwrap();
        let edit = IncidentEdit {
            title: "Outage (resolved)".into(),
            updates: vec![update(Impact::Offline, 100), update(Impact::None, 200)],
        };

        // Correct version -> updated, version bumped, content replaced.
        let outcome = state.replace_incident(incident.id, incident.version, edit.clone()).await.unwrap();
        let updated = match outcome {
            CasOutcome::Updated(i) => i,
            _ => panic!("expected Updated"),
        };
        assert_eq!(updated.version, 2);
        assert_eq!(updated.title, "Outage (resolved)");
        assert_eq!(updated.current_impact(), Impact::None);

        // Stale version -> conflict reporting the current version; the store is unchanged.
        let stale = state.replace_incident(incident.id, incident.version, edit).await.unwrap();
        assert!(matches!(stale, CasOutcome::VersionMismatch(2)));
        assert_eq!(state.get_incident(incident.id).await.unwrap().unwrap().version, 2);

        // Unknown id -> not found.
        assert!(matches!(
            state.replace_incident(Identifier::from(424242u64), 1, IncidentEdit { title: "x".into(), updates: vec![] }).await.unwrap(),
            CasOutcome::NotFound
        ));
    }
}
