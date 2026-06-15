//! Incident storage: the [`IncidentStore`] trait and its implementation over the [`State`] redb
//! store, kept separate from the underlying store itself. Incidents are keyed by a random `u32` and
//! serialized as JSON; the public id is that number rendered as dash-grouped base36.

use std::error::Error;

use grey_api::{Incident, IncidentUpdate};
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use tracing_batteries::prelude::*;

use super::State;

const INCIDENTS_TABLE: TableDefinition<u32, &[u8]> = TableDefinition::new("incidents");

/// Storage operations for incidents.
#[allow(async_fn_in_trait)]
pub trait IncidentStore {
    /// Lists stored incidents, most recent first. When `include_drafts` is false, incidents whose
    /// current impact is hidden are omitted — the public, unauthenticated view.
    async fn list_incidents(&self, include_drafts: bool) -> Result<Vec<Incident>, Box<dyn Error>>;

    /// Fetches a single incident by its (grouped base36) id.
    async fn get_incident(&self, id: &str) -> Result<Option<Incident>, Box<dyn Error>>;

    /// Creates an incident, assigning it a fresh, unused random id (retrying on the astronomically
    /// unlikely chance of a collision). The incident's `id` is overwritten with the generated
    /// grouped-base36 form, and the stored incident is returned.
    async fn create_incident(&self, incident: Incident) -> Result<Incident, Box<dyn Error>>;

    /// Replaces an existing incident, keyed by its id.
    async fn put_incident(&self, incident: &Incident) -> Result<(), Box<dyn Error>>;

    /// Deletes an incident, returning whether a record was actually removed.
    async fn delete_incident(&self, id: &str) -> Result<bool, Box<dyn Error>>;

    /// Appends a status update to an existing incident, keeping updates ordered by timestamp and
    /// refreshing `updated_at`. Returns whether the target incident exists.
    async fn add_incident_update(
        &self,
        id: &str,
        update: IncidentUpdate,
    ) -> Result<bool, Box<dyn Error>>;
}

impl IncidentStore for State {
    async fn list_incidents(&self, include_drafts: bool) -> Result<Vec<Incident>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        let mut incidents = Vec::new();
        if let Ok(table) = txn.open_table(INCIDENTS_TABLE) {
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (_key, value) = entry;
                match serde_json::from_slice::<Incident>(value.value()) {
                    Ok(incident) if include_drafts || incident.is_public() => {
                        incidents.push(incident)
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!("Skipping an incident record that failed to deserialize: {err:?}")
                    }
                }
            }
        }
        incidents.sort_by(|a, b| b.start_time.cmp(&a.start_time));
        Ok(incidents)
    }

    async fn get_incident(&self, id: &str) -> Result<Option<Incident>, Box<dyn Error>> {
        let Some(key) = grey_api::decode_incident_id(id) else {
            return Ok(None);
        };
        let txn = self.database.begin_read()?;
        if let Ok(table) = txn.open_table(INCIDENTS_TABLE)
            && let Some(value) = table.get(key)?
        {
            return Ok(Some(serde_json::from_slice::<Incident>(value.value())?));
        }
        Ok(None)
    }

    async fn create_incident(&self, mut incident: Incident) -> Result<Incident, Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;

            let mut key = rand::random::<u32>();
            let mut attempts = 0;
            while table.get(key)?.is_some() {
                key = rand::random::<u32>();
                attempts += 1;
                if attempts > 1000 {
                    return Err("could not allocate a unique incident id".into());
                }
            }

            incident.id = grey_api::encode_incident_id(key);
            let data = serde_json::to_vec(&incident)?;
            table.insert(key, data.as_slice())?;
        }
        txn.commit()?;
        Ok(incident)
    }

    async fn put_incident(&self, incident: &Incident) -> Result<(), Box<dyn Error>> {
        let key = grey_api::decode_incident_id(&incident.id)
            .ok_or_else(|| format!("invalid incident id: {}", incident.id))?;
        let data = serde_json::to_vec(incident)?;
        let txn = self.database.begin_write()?;
        {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            table.insert(key, data.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    async fn delete_incident(&self, id: &str) -> Result<bool, Box<dyn Error>> {
        let Some(key) = grey_api::decode_incident_id(id) else {
            return Ok(false);
        };
        let txn = self.database.begin_write()?;
        let existed = {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            table.remove(key)?.is_some()
        };
        txn.commit()?;
        Ok(existed)
    }

    async fn add_incident_update(
        &self,
        id: &str,
        update: IncidentUpdate,
    ) -> Result<bool, Box<dyn Error>> {
        let Some(key) = grey_api::decode_incident_id(id) else {
            return Ok(false);
        };
        let txn = self.database.begin_write()?;
        let found = {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            // Read the existing record into an owned value so the read guard is released before we
            // insert the updated record back into the same table.
            let existing: Option<Incident> = table
                .get(key)?
                .map(|value| serde_json::from_slice::<Incident>(value.value()))
                .transpose()?;

            match existing {
                Some(mut incident) => {
                    incident.updates.push(update);
                    incident.updates.sort_by_key(|u| u.timestamp);
                    incident.updated_at = chrono::Utc::now();
                    let data = serde_json::to_vec(&incident)?;
                    table.insert(key, data.as_slice())?;
                    true
                }
                None => false,
            }
        };
        txn.commit()?;
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::Impact;

    fn sample_incident(id: &str, public: bool, start_secs: i64) -> Incident {
        let ts = chrono::DateTime::from_timestamp(start_secs, 0).unwrap();
        Incident {
            id: id.into(),
            title: format!("Incident {id}"),
            description: "details".into(),
            start_time: ts,
            end_time: None,
            affected_services: vec![],
            // No updates -> hidden (a draft); an offline update makes it public.
            updates: if public {
                vec![sample_update(&format!("{id}-u"), Impact::Offline, start_secs)]
            } else {
                vec![]
            },
            created_at: ts,
            updated_at: ts,
        }
    }

    fn sample_update(id: &str, impact: Impact, secs: i64) -> IncidentUpdate {
        IncidentUpdate {
            id: id.into(),
            impact,
            timestamp: chrono::DateTime::from_timestamp(secs, 0).unwrap(),
            message: "update".into(),
        }
    }

    #[tokio::test]
    async fn incident_crud_and_visibility_filter() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        // The ids "a"/"b"/"c" are valid base36, so they double as their own redb keys here.
        state.put_incident(&sample_incident("a", true, 300)).await.unwrap();
        state.put_incident(&sample_incident("b", false, 200)).await.unwrap();
        state.put_incident(&sample_incident("c", true, 100)).await.unwrap();

        // The public view hides drafts; the admin view shows everything, newest first.
        let public = state.list_incidents(false).await.unwrap();
        assert_eq!(public.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["a", "c"]);
        let all = state.list_incidents(true).await.unwrap();
        assert_eq!(all.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);

        // Fetch by id round-trips; an unknown id is None.
        assert_eq!(state.get_incident("b").await.unwrap().unwrap().id, "b");
        assert!(state.get_incident("missing-xyz").await.unwrap().is_none());

        // Deletion is reported and idempotent.
        assert!(state.delete_incident("b").await.unwrap());
        assert!(!state.delete_incident("b").await.unwrap());
        assert!(state.get_incident("b").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn add_incident_update_appends_in_timestamp_order() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        state.put_incident(&sample_incident("a", true, 0)).await.unwrap();

        // Add out of order; storage keeps updates sorted by timestamp. (start update is "a-u" @ 0.)
        assert!(state.add_incident_update("a", sample_update("u2", Impact::None, 200)).await.unwrap());
        assert!(state.add_incident_update("a", sample_update("u1", Impact::Offline, 100)).await.unwrap());

        let incident = state.get_incident("a").await.unwrap().unwrap();
        assert_eq!(
            incident.updates.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(),
            vec!["a-u", "u1", "u2"]
        );
        assert_eq!(incident.current_impact(), Impact::None);

        assert!(!state.add_incident_update("missing-xyz", sample_update("x", Impact::None, 1)).await.unwrap());
    }

    #[tokio::test]
    async fn create_incident_assigns_unique_decodable_ids() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        let mut blank = sample_incident("ignored", false, 0);
        blank.id = String::new();
        let a = state.create_incident(blank.clone()).await.unwrap();
        let b = state.create_incident(blank).await.unwrap();

        assert_ne!(a.id, b.id, "each incident must get a distinct id");
        assert!(grey_api::decode_incident_id(&a.id).is_some(), "id must decode as base36");
        assert_eq!(state.get_incident(&a.id).await.unwrap().unwrap().id, a.id);
    }
}
