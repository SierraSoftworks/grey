//! Probe-state storage: the [`ProbeStore`] trait and its implementation over the [`State`] redb
//! store, plus the cluster [`Versioned`] implementation for probes. Kept separate from the
//! underlying store, mirroring the incident storage split.

use std::collections::HashMap;
use std::error::Error;

use grey_api::{Mergeable, Probe};
use redb::{ReadableDatabase, ReadableTable};
use tracing::{info, instrument};
use tracing_batteries::prelude::*;

use crate::cluster::Versioned;
use crate::result::ProbeResult;

use super::{CLUSTER_FIELDS_TABLE, ProbeState, State};

/// Storage operations for probe state (the cluster-replicated, gossiped records).
#[allow(async_fn_in_trait)]
pub trait ProbeStore {
    /// The pooled, cluster-merged probe states keyed by probe name.
    async fn get_probe_states(&self) -> Result<HashMap<String, Probe>, Box<dyn Error>>;

    /// Persists the configured probe metadata for this node.
    async fn update_probe_config(&self, probe: &crate::Probe) -> Result<(), Box<dyn Error>>;

    /// Applies a fresh probe result to this node's stored state for the named probe.
    async fn update_probe_state(
        &self,
        probe_name: &str,
        probe_result: ProbeResult,
    ) -> Result<(), Box<dyn Error>>;

    /// Drops probe records that have aged out beyond the configured expiry.
    async fn gc(&self) -> Result<(), Box<dyn Error>>;

    /// Runs [`ProbeStore::gc`] on the configured interval, forever.
    async fn gc_loop(&self);
}

impl ProbeStore for State {
    async fn get_probe_states(&self) -> Result<HashMap<String, Probe>, Box<dyn Error>> {
        let mut histories = HashMap::new();
        for probe in self.get_config().probes.iter() {
            histories.insert(probe.name.clone(), probe.into());
        }

        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_FIELDS_TABLE)?;
        let iter = table.iter()?;
        for entry in iter.filter_map(|r| r.ok()) {
            let (key, value) = entry;
            let (_node_id, probe_name) = key.value();
            let (_, data) = value.value();
            if let Ok(snapshot) = rmp_serde::from_slice::<ProbeState>(data) {
                histories
                    .entry(probe_name.clone())
                    .and_modify(|existing: &mut ProbeState| {
                        existing.merge(&snapshot);
                    })
                    .or_insert_with(|| snapshot.clone());
            }
        }

        Ok(histories)
    }

    async fn update_probe_config(&self, probe: &crate::Probe) -> Result<(), Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        {
            let mut table = txn.open_table(CLUSTER_FIELDS_TABLE)?;

            let mut snapshot = table
                .get((self.node_id.into(), probe.name.clone()))?
                .map(|existing| {
                    let (_version, data) = existing.value();
                    rmp_serde::from_slice::<ProbeState>(data).unwrap_or_else(|_| probe.into())
                })
                .unwrap_or_else(|| probe.into());

            let mut updated_probe: ProbeState = probe.into();
            updated_probe.last_updated = snapshot.last_updated + chrono::Duration::milliseconds(1);

            snapshot.merge(&updated_probe);

            table.insert(
                (self.node_id.into(), probe.name.clone()),
                (snapshot.version(), rmp_serde::to_vec_named(&snapshot)?.as_slice()),
            )?;
        }

        txn.commit()?;

        Ok(())
    }

    async fn update_probe_state(
        &self,
        probe_name: &str,
        probe_result: ProbeResult,
    ) -> Result<(), Box<dyn Error>> {
        let txn = self.database.begin_write()?;

        if let Some(probe) = self.get_config().probes.iter().find(|p| p.name == probe_name) {
            let result = {
                let mut table = txn.open_table(CLUSTER_FIELDS_TABLE)?;

                let (mut snapshot, _version) = table
                    .get((self.node_id.into(), probe.name.clone()))?
                    .map(|existing| {
                        let (version, data) = existing.value();
                        match rmp_serde::from_slice::<ProbeState>(data) {
                            Ok(snapshot) => (snapshot, version),
                            Err(err) => {
                                warn!("Failed to deserialize probe snapshot for '{probe_name}', resetting the state: {:?}", err);
                                (probe.into(), version)
                            },
                        }
                    })
                    .unwrap_or_else(|| (probe.into(), 0));

                probe_result.apply(self.node_id, &mut snapshot);

                let new_data = rmp_serde::to_vec_named(&snapshot)?;
                table.insert(
                    (self.node_id.into(), probe.name.clone()),
                    (snapshot.version(), new_data.as_slice()),
                )?;

                Ok(())
            };

            txn.commit()?;

            result
        } else {
            Err(format!("Probe '{probe_name}' is no longer present in the configuration, its history was not updated.").into())
        }
    }

    #[instrument(name="state.gc", skip(self), fields(otel.kind = "internal", node.id=%self.node_id), err(Debug))]
    async fn gc(&self) -> Result<(), Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        {
            let mut table_fields = txn.open_table(CLUSTER_FIELDS_TABLE)?;

            let history_expiry_threshold =
                chrono::Utc::now() - self.get_config().cluster.gc_probe_expiry;

            // Peer/membership records live entirely in memory (the registry expires them itself);
            // only probe state is persisted, so the GC sweep here is concerned with probes alone.
            let mut dropped_probe_records = 0;
            table_fields.retain(|(_, probe_name), (version, _data)| {
                // `version` is the probe's `last_updated` in milliseconds (see `Versioned for Probe`).
                let last_updated = chrono::DateTime::from_timestamp_millis(version as i64).unwrap_or_default();
                if last_updated >= history_expiry_threshold {
                    true
                } else {
                    info!(name: "state.gc.probe", { probe.name = %probe_name, %last_updated, expired_at=%history_expiry_threshold }, "Dropping stale probe record");
                    dropped_probe_records += 1;
                    false
                }
            })?;

            if dropped_probe_records > 0 {
                info!(name: "state.gc.summary", { dropped_probe_records = %dropped_probe_records }, "Dropped stale probe records");
            }
        }

        txn.commit()?;

        Ok(())
    }

    async fn gc_loop(&self) {
        loop {
            if let Err(err) = self.gc().await {
                warn!("Failed to perform state GC: {:?}", err);
            }

            tokio::time::sleep(self.get_config().cluster.gc_interval).await;
        }
    }
}

impl Versioned for Probe {
    type Diff = Probe;

    fn version(&self) -> u64 {
        // Millisecond granularity: two updates within the same wall-clock second produce distinct
        // versions, so the second one is not silently skipped by the digest/diff comparison.
        self.last_updated.timestamp_millis() as u64
    }

    fn diff(&self, version: u64) -> Option<Self::Diff>
    where
        Self: Sized,
    {
        if self.version() > version {
            Some(Self {
                name: self.name.clone(),
                tags: self.tags.clone(),
                last_updated: self.last_updated,
                history: self
                    .history
                    .iter()
                    .filter(|h| h.start_time > self.last_updated - chrono::Duration::hours(2))
                    .cloned()
                    .collect(),
                observations: self.observations.clone(),
                streak: self.streak.clone(),
            })
        } else {
            None
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        self.merge(diff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::cluster::NodeID;
    use std::sync::Arc;

    fn probe_at(name: &str, when: chrono::DateTime<chrono::Utc>) -> ProbeState {
        Probe {
            name: name.into(),
            tags: HashMap::new(),
            last_updated: when,
            history: Vec::new(),
            observations: HashMap::new(),
            streak: grey_api::Streak::default(),
        }
    }

    /// Two updates within the same wall-clock second must produce distinct versions, so the later
    /// one is diffable rather than silently skipped.
    #[test]
    fn version_has_millisecond_granularity() {
        let base = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let earlier = probe_at("p", base);
        let later = probe_at("p", base + chrono::Duration::milliseconds(1));

        assert!(later.version() > earlier.version(), "a 1ms-newer update must advance the version");
        assert!(later.diff(earlier.version()).is_some(), "the newer update must be diffable");
        assert!(earlier.diff(earlier.version()).is_none(), "an unchanged probe has nothing to diff");
    }

    /// GC must interpret the stored version as milliseconds; otherwise a millisecond timestamp read
    /// as seconds lands ~50000 years in the future and probes would never expire.
    #[tokio::test]
    async fn gc_expires_probes_using_millisecond_versions() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        let mut config = Config::test(&dir.path().to_path_buf());
        config.cluster.gc_probe_expiry = std::time::Duration::from_secs(60);
        *state.config.write().unwrap() = Arc::new(config);

        let node = NodeID::new();
        let stale_ms = (chrono::Utc::now() - chrono::Duration::hours(1)).timestamp_millis() as u64;
        let fresh_ms = chrono::Utc::now().timestamp_millis() as u64;

        {
            let txn = state.database.begin_write().unwrap();
            {
                let mut table = txn.open_table(CLUSTER_FIELDS_TABLE).unwrap();
                let stale = rmp_serde::to_vec_named(&probe_at("stale", chrono::Utc::now())).unwrap();
                let fresh = rmp_serde::to_vec_named(&probe_at("fresh", chrono::Utc::now())).unwrap();
                table.insert((node.into(), "stale".to_string()), (stale_ms, stale.as_slice())).unwrap();
                table.insert((node.into(), "fresh".to_string()), (fresh_ms, fresh.as_slice())).unwrap();
            }
            txn.commit().unwrap();
        }

        state.gc().await.unwrap();

        let txn = state.database.begin_read().unwrap();
        let table = txn.open_table(CLUSTER_FIELDS_TABLE).unwrap();
        assert!(
            table.get((node.into(), "fresh".to_string())).unwrap().is_some(),
            "a recent probe must be retained"
        );
        assert!(
            table.get((node.into(), "stale".to_string())).unwrap().is_none(),
            "an hour-old probe must expire under a 60s expiry (i.e. version read as milliseconds)"
        );
    }
}
