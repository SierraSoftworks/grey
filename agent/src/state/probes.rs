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

use redb::Durability;

use super::{
    PROBES_TABLE, CRON_TABLE, DEFERRED, ProbeState, State,
    gc_lww_table,
};

/// Storage operations for probe state (the cluster-replicated, gossiped records).
#[allow(async_fn_in_trait)]
pub trait ProbeStore {
    /// The pooled, cluster-merged probe states keyed by probe name.
    async fn get_probe_states(&self) -> Result<HashMap<String, Probe>, Box<dyn Error>>;

    /// The names of every probe visible to this node: the configured set plus any name present in
    /// stored records (peers may observe probes this node no longer configures). Walks only the
    /// table's keys, so it is cheap regardless of record sizes.
    async fn get_probe_names(&self) -> Result<Vec<String>, Box<dyn Error>>;

    /// As [`ProbeStore::get_probe_states`], restricted to the named probes. Decoding record
    /// snapshots is the dominant cost of a scan, so callers that can tolerate a spread-out view
    /// (e.g. the notifier) batch the names from [`ProbeStore::get_probe_names`] through this to
    /// turn one large scan into several small ones.
    async fn get_probe_states_for(
        &self,
        names: &std::collections::HashSet<String>,
    ) -> Result<HashMap<String, Probe>, Box<dyn Error>>;

    /// Persists the configured probe metadata for this node.
    async fn update_probe_config(&self, probe: &crate::Probe) -> Result<(), Box<dyn Error>>;

    /// Reconciles this node's stored probe records against the current configuration, tombstoning
    /// the records of probes that have been removed (and clearing the tombstone from any that have
    /// returned).
    async fn reconcile_probe_config(&self) -> Result<(), Box<dyn Error>>;

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

impl State {
    /// The shared scan behind [`ProbeStore::get_probe_states`] and
    /// [`ProbeStore::get_probe_states_for`]: `filter` restricts which probe names are seeded and
    /// decoded (`None` means all). The key walk always covers the whole table — skipping the
    /// snapshot decode for filtered-out rows is what makes a restricted scan cheap.
    fn probe_states_filtered(
        &self,
        filter: Option<&std::collections::HashSet<String>>,
    ) -> Result<HashMap<String, Probe>, Box<dyn Error>> {
        let config = self.get_config();
        let included = |name: &str| filter.map(|f| f.contains(name)).unwrap_or(true);

        let mut histories = HashMap::new();
        for probe in config.probes.iter() {
            if included(&probe.name) {
                histories.insert(probe.name.clone(), probe.into());
            }
        }

        let txn = self.database.begin_read()?;
        // The table only exists once probe state has been written; treat its absence as "no stored
        // state yet" (returning just the config-seeded probes) rather than erroring, so a fresh node
        // — or the notifier's first pass before any sample is recorded — reads cleanly. Mirrors the
        // tolerance in `get_cron_states`.
        if let Ok(table) = txn.open_table(PROBES_TABLE) {
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (key, value) = entry;
                let (_node_id, probe_name) = key.value();
                if !included(&probe_name) {
                    continue;
                }
                let (_, data) = value.value();
                if let Ok(snapshot) = rmp_serde::from_slice::<ProbeState>(data) {
                    // A retired record is an observer's tombstone for a probe it no longer runs; it
                    // contributes nothing to the pool, so a probe disappears once every observer has
                    // dropped it (and stays visible while any node still probes it).
                    if snapshot.retired {
                        continue;
                    }

                    histories
                        .entry(probe_name.clone())
                        .and_modify(|existing: &mut ProbeState| {
                            existing.merge(&snapshot);
                        })
                        .or_insert_with(|| snapshot.clone());
                }
            }
        }

        // The alerting debounce (the streak recovery window) is authoritative locally for display
        // and detection — re-stamp it so a peer's stale config can never override the operator's
        // view. Mirrors the cron config-echo in `get_cron_states`.
        for probe in config.probes.iter() {
            if let Some(pooled) = histories.get_mut(&probe.name) {
                pooled.debounce = Some(probe.alerting.debounce_std());
                pooled.quorum = Some(probe.alerting.quorum.unwrap_or(config.cluster.quorum));
            }
        }
        // Probes this node doesn't run itself still take the cluster-wide quorum.
        for pooled in histories.values_mut() {
            pooled.quorum.get_or_insert(config.cluster.quorum);
        }

        Ok(histories)
    }
}

impl ProbeStore for State {
    async fn get_probe_states(&self) -> Result<HashMap<String, Probe>, Box<dyn Error>> {
        self.probe_states_filtered(None)
    }

    async fn get_probe_names(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let config = self.get_config();
        let mut names: std::collections::HashSet<String> =
            config.probes.iter().map(|p| p.name.clone()).collect();

        let txn = self.database.begin_read()?;
        if let Ok(table) = txn.open_table(PROBES_TABLE) {
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (key, _value) = entry;
                let (_node_id, probe_name) = key.value();
                names.insert(probe_name);
            }
        }

        Ok(names.into_iter().collect())
    }

    async fn get_probe_states_for(
        &self,
        names: &std::collections::HashSet<String>,
    ) -> Result<HashMap<String, Probe>, Box<dyn Error>> {
        self.probe_states_filtered(Some(names))
    }

    async fn update_probe_config(&self, probe: &crate::Probe) -> Result<(), Box<dyn Error>> {
        let own_id: u128 = self.node_id.into();
        let probe = probe.clone();
        self.write("update_probe_config", Durability::Immediate, move |txn| {
            let mut table = txn.open_table(PROBES_TABLE)?;

            let mut snapshot = table
                .get((own_id, probe.name.clone()))?
                .map(|existing| {
                    let (_version, data) = existing.value();
                    rmp_serde::from_slice::<ProbeState>(data).unwrap_or_else(|_| (&probe).into())
                })
                .unwrap_or_else(|| (&probe).into());

            let mut updated_probe: ProbeState = (&probe).into();
            updated_probe.last_updated = snapshot.last_updated + chrono::Duration::milliseconds(1);

            snapshot.merge(&updated_probe);

            table.insert(
                (own_id, probe.name.clone()),
                (snapshot.version(), rmp_serde::to_vec_named(&snapshot)?.as_slice()),
            )?;

            Ok(())
        })
        .await
    }

    #[instrument(name="state.probes.reconcile", skip(self), fields(otel.kind = "internal", node.id=%self.node_id), err(Debug))]
    async fn reconcile_probe_config(&self) -> Result<(), Box<dyn Error>> {
        let config = self.get_config();
        let own_id: u128 = self.node_id.into();

        self.write("reconcile_probe_config", Durability::Immediate, move |txn| {
            let mut table = txn.open_table(PROBES_TABLE)?;

            let mut updates: Vec<(String, ProbeState)> = Vec::new();
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (key, value) = entry;
                let (node_id, probe_name) = key.value();
                // Only this node's own observations are ours to retire; a peer's record is retired by
                // the peer itself and reaches us through gossip.
                if node_id != own_id {
                    continue;
                }

                let (_version, data) = value.value();
                let Ok(mut snapshot) = rmp_serde::from_slice::<ProbeState>(data) else {
                    continue;
                };

                let retired = !config.probes.iter().any(|p| p.name == probe_name);
                if snapshot.retired == retired {
                    continue;
                }

                snapshot.retired = retired;
                // `last_updated` serializes with second precision, so advance by a whole second when
                // the clock hasn't moved on yet — otherwise the version wouldn't change and peers
                // would never pick the tombstone up.
                snapshot.last_updated = chrono::Utc::now()
                    .max(snapshot.last_updated + chrono::Duration::seconds(1));
                // A tombstoned record stops receiving results (the only other place an own record
                // is pruned), so shed its aged-out history here rather than gossiping it around
                // until the record itself expires.
                snapshot.prune_history();
                updates.push((probe_name, snapshot));
            }

            for (probe_name, snapshot) in updates {
                info!(name: "state.probes.reconcile", { probe.name = %probe_name, probe.retired = snapshot.retired }, "Reconciled stored probe record against the configuration");
                table.insert(
                    (own_id, probe_name),
                    (snapshot.version(), rmp_serde::to_vec_named(&snapshot)?.as_slice()),
                )?;
            }

            Ok(())
        })
        .await
    }

    async fn update_probe_state(
        &self,
        probe_name: &str,
        probe_result: ProbeResult,
    ) -> Result<(), Box<dyn Error>> {
        let Some(probe) = self.get_config().probes.iter().find(|p| p.name == probe_name).cloned() else {
            return Err(format!("Probe '{probe_name}' is no longer present in the configuration, its history was not updated.").into());
        };

        let node_id = self.node_id;
        let own_id: u128 = node_id.into();
        // The hottest write in the process (one per probe sample), so it commits with deferred
        // durability; see `DEFERRED` for the trade-off.
        self.write("update_probe_state", DEFERRED, move |txn| {
            let mut table = txn.open_table(PROBES_TABLE)?;

            let (mut snapshot, _version) = table
                .get((own_id, probe.name.clone()))?
                .map(|existing| {
                    let (version, data) = existing.value();
                    match rmp_serde::from_slice::<ProbeState>(data) {
                        Ok(snapshot) => (snapshot, version),
                        Err(err) => {
                            warn!("Failed to deserialize probe snapshot for '{}', resetting the state: {:?}", probe.name, err);
                            ((&probe).into(), version)
                        },
                    }
                })
                .unwrap_or_else(|| ((&probe).into(), 0));

            probe_result.apply(node_id, &mut snapshot);
            // A node's own records never pass through `merge` (the receive path), so this is
            // where their history retention is enforced; without it they grow by one bucket per
            // hour for as long as the probe keeps running.
            snapshot.prune_history();

            let new_data = rmp_serde::to_vec_named(&snapshot)?;
            table.insert(
                (own_id, probe.name.clone()),
                (snapshot.version(), new_data.as_slice()),
            )?;

            Ok(())
        })
        .await
    }

    #[instrument(name="state.gc", skip(self), fields(otel.kind = "internal", node.id=%self.node_id), err(Debug))]
    async fn gc(&self) -> Result<(), Box<dyn Error>> {
        let history_expiry_threshold =
            chrono::Utc::now() - self.get_config().cluster.gc_probe_expiry;

        // Immediate durability here doubles as a periodic flush of the deferred hot-path commits.
        self.write("gc", Durability::Immediate, move |txn| {
            let mut table_fields = txn.open_table(PROBES_TABLE)?;

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

            // Crons age out on the same expiry as probes: a record whose writes (and re-gossip)
            // have stopped for long enough is dropped, which is also what eventually reaps converged
            // delete tombstones. Incidents and incident updates are deliberately *not* swept here —
            // they form the historical record of outages and are retained indefinitely (their delete
            // tombstones likewise persist rather than being reaped on the probe-expiry cadence).
            let dropped_crons = gc_lww_table(txn, CRON_TABLE, history_expiry_threshold)?;

            if dropped_crons > 0 {
                info!(name: "state.gc.summary", { dropped_crons = %dropped_crons }, "Dropped stale cron records");
            }

            info!(
                name: "state.gc.pass",
                {
                    dropped_probe_records,
                    dropped_crons,
                    expired_at = %history_expiry_threshold,
                },
                "Completed state garbage collection pass.",
            );

            Ok(())
        })
        .await
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
                debounce: self.debounce,
                retired: self.retired,
                observers: self.observers.clone(),
                quorum: self.quorum,
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
            debounce: None,
            retired: false,
            observers: Default::default(),
            quorum: None,
        }
    }

    /// A probe dropped from the configuration disappears from the pooled view (and so from the UI)
    /// even though it has recorded history, and its record is tombstoned rather than deleted so the
    /// removal propagates to peers holding a copy.
    #[tokio::test]
    async fn removing_a_probe_from_the_config_retires_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        let probe_name = state.get_config().probes[0].name.clone();

        assert!(
            state.get_probe_states().await.unwrap().contains_key(&probe_name),
            "the configured probe must be visible before it is removed"
        );

        let mut config = Config::test(&dir.path().to_path_buf());
        config.probes.clear();
        state.set_config_for_test(config);
        state.reconcile_probe_config().await.unwrap();

        assert!(
            !state.get_probe_states().await.unwrap().contains_key(&probe_name),
            "a probe removed from the configuration must not be returned, even with stored history"
        );

        let txn = state.database.begin_read().unwrap();
        let table = txn.open_table(PROBES_TABLE).unwrap();
        let entry = table
            .get((state.node_id.into(), probe_name.clone()))
            .unwrap()
            .expect("the tombstone must be retained so it can be gossiped");
        let (_version, data) = entry.value();
        let stored: ProbeState = rmp_serde::from_slice(data).unwrap();
        assert!(stored.retired, "the record must be tombstoned rather than deleted");
    }

    /// Retirement is per-observer: a peer that still runs the probe keeps it visible here (the
    /// headless-worker topology), and re-adding it locally clears the tombstone without losing the
    /// history that was recorded before the removal.
    #[tokio::test]
    async fn retirement_is_scoped_to_this_node_and_reversible() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        let probe_name = state.get_config().probes[0].name.clone();

        let empty = {
            let mut config = Config::test(&dir.path().to_path_buf());
            config.probes.clear();
            config
        };
        state.set_config_for_test(empty);
        state.reconcile_probe_config().await.unwrap();

        // A peer still observing the probe keeps it in the pooled view.
        let peer = NodeID::new();
        let mut diff = crate::cluster::ClusterStateDiff::new();
        diff.update(
            peer,
            probe_name.clone(),
            crate::state::ReplicatedEntity::Probe(probe_at(&probe_name, chrono::Utc::now())),
        );
        crate::cluster::GossipStore::apply(&state, diff).await.unwrap();
        assert!(
            state.get_probe_states().await.unwrap().contains_key(&probe_name),
            "a peer that still runs the probe must keep it visible"
        );

        // Restoring the configuration revives this node's own record.
        state.set_config_for_test(Config::test(&dir.path().to_path_buf()));
        state.reconcile_probe_config().await.unwrap();

        let txn = state.database.begin_read().unwrap();
        let table = txn.open_table(PROBES_TABLE).unwrap();
        let entry = table.get((state.node_id.into(), probe_name.clone())).unwrap().unwrap();
        let (_version, data) = entry.value();
        let stored: ProbeState = rmp_serde::from_slice(data).unwrap();
        assert!(!stored.retired, "re-adding the probe must clear the tombstone");
        assert!(
            !stored.history.is_empty(),
            "the history recorded before the removal must survive the round trip"
        );
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

    /// Each node's record carries only its own observer entry; pooling unions them, and the pooled
    /// health follows the configured quorum rather than any single observer's failure.
    #[tokio::test]
    async fn pooled_health_follows_the_quorum_of_observers() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        let probe_name = state.get_config().probes[0].name.clone();
        let now = chrono::Utc::now();
        let window = grey_api::Streak::default_recovery_window();

        // This node has been observing a sustained failure (samples closer together than the
        // window, so they form one continuous episode older than the debounce).
        for minutes_ago in [16, 12, 8, 4, 0] {
            let result = ProbeResult {
                start_time: now - chrono::Duration::minutes(minutes_ago),
                pass: false,
                ..ProbeResult::test()
            };
            state.update_probe_state(&probe_name, result).await.unwrap();
        }

        let own_id = state.node_id.to_string();
        let pooled = state.get_probe_states().await.unwrap();
        let probe = &pooled[&probe_name];
        assert_eq!(probe.observers.keys().collect::<Vec<_>>(), vec![&own_id], "only this node's entry is recorded");
        assert_eq!(probe.quorum, Some(grey_api::Quorum::Majority), "the cluster default is stamped");
        assert!(!probe.healthy_at(now, window), "a lone observer is its own majority");

        // Two peers observe the probe passing: the failing node is now outvoted.
        for _ in 0..2 {
            let peer = NodeID::new();
            let mut record = probe_at(&probe_name, now);
            record.observers.insert(
                peer.to_string(),
                grey_api::ObserverState {
                    streak: grey_api::Streak { covered_since: Some(now - chrono::Duration::days(1)), ..Default::default() },
                    last_updated: now,
                },
            );
            let mut diff = crate::cluster::ClusterStateDiff::new();
            diff.update(peer, probe_name.clone(), crate::state::ReplicatedEntity::Probe(record));
            crate::cluster::GossipStore::apply(&state, diff).await.unwrap();
        }

        let pooled = state.get_probe_states().await.unwrap();
        let probe = &pooled[&probe_name];
        assert_eq!(probe.observers.len(), 3);
        assert_eq!(probe.quorum_size(), 2);
        assert!(probe.healthy_at(now, window), "one failing observer of three is below the majority");

        // Receiving peers' records must not pollute this node's own entry with their observers.
        let txn = state.database.begin_read().unwrap();
        let table = txn.open_table(PROBES_TABLE).unwrap();
        let entry = table.get((state.node_id.into(), probe_name.clone())).unwrap().unwrap();
        let (_version, data) = entry.value();
        let own: ProbeState = rmp_serde::from_slice(data).unwrap();
        assert_eq!(own.observers.len(), 1, "the own record only carries this node's observer entry");

        // The derived node view sees this node disagreeing with the cluster on its only probe.
        let nodes = state.get_nodes().await.unwrap();
        let me = nodes.iter().find(|n| n.id == own_id).expect("this node observes the probe");
        assert_eq!(me.status, grey_api::NodeStatus::Degraded);
        assert_eq!((me.disagreeing, me.total), (1, 1));

        // ...and the peers view carries it.
        let peers = state.get_peers().await.unwrap();
        let me = peers.iter().find(|p| p.current).unwrap();
        assert_eq!(me.node.as_ref().map(|n| n.status), Some(grey_api::NodeStatus::Degraded));
        let listed: Vec<_> = peers.iter().filter(|p| !p.current && p.node.is_some()).collect();
        assert_eq!(listed.len(), 2, "observers without membership records are still listed");
        assert!(listed.iter().all(|p| p.health == grey_api::PeerHealth::Offline));
    }

    /// A per-probe `alerting.quorum` overrides the cluster default when stamping the pooled view.
    #[tokio::test]
    async fn per_probe_quorum_overrides_the_cluster_default() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        let mut config = Config::test(&dir.path().to_path_buf());
        config.cluster.quorum = grey_api::Quorum::Percent(60);
        config.probes[0].alerting.quorum = Some(grey_api::Quorum::Count(1));
        let probe_name = config.probes[0].name.clone();
        state.set_config_for_test(config);

        let pooled = state.get_probe_states().await.unwrap();
        assert_eq!(pooled[&probe_name].quorum, Some(grey_api::Quorum::Count(1)));
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
                let mut table = txn.open_table(PROBES_TABLE).unwrap();
                let stale = rmp_serde::to_vec_named(&probe_at("stale", chrono::Utc::now())).unwrap();
                let fresh = rmp_serde::to_vec_named(&probe_at("fresh", chrono::Utc::now())).unwrap();
                table.insert((node.into(), "stale".to_string()), (stale_ms, stale.as_slice())).unwrap();
                table.insert((node.into(), "fresh".to_string()), (fresh_ms, fresh.as_slice())).unwrap();
            }
            txn.commit().unwrap();
        }

        state.gc().await.unwrap();

        let txn = state.database.begin_read().unwrap();
        let table = txn.open_table(PROBES_TABLE).unwrap();
        assert!(
            table.get((node.into(), "fresh".to_string())).unwrap().is_some(),
            "a recent probe must be retained"
        );
        assert!(
            table.get((node.into(), "stale".to_string())).unwrap().is_none(),
            "an hour-old probe must expire under a 60s expiry (i.e. version read as milliseconds)"
        );
    }

    /// A node's own records never pass through `merge` (the receive path), so applying a probe
    /// result is where their history retention must be enforced — without it they grow by one
    /// bucket per hour for as long as the probe runs (67 days of buckets was observed in
    /// production before this was pruned here).
    #[tokio::test]
    async fn applying_a_result_prunes_aged_history_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        let probe_name = state.get_config().probes[0].name.clone();
        let own: u128 = state.node_id.into();

        // Seed this node's own record with history far beyond the retention window, emulating a
        // long-running node's accumulated state.
        {
            let txn = state.database.begin_write().unwrap();
            {
                let mut table = txn.open_table(PROBES_TABLE).unwrap();
                let (version, mut snapshot) = table
                    .get((own, probe_name.clone()))
                    .unwrap()
                    .map(|existing| {
                        let (version, data) = existing.value();
                        (version, rmp_serde::from_slice::<ProbeState>(data).unwrap())
                    })
                    .expect("State::test records a probe result");

                let now = chrono::Utc::now();
                let mut history: Vec<grey_api::ProbeHistoryBucket> = (49..149)
                    .rev()
                    .map(|age_hours| grey_api::ProbeHistoryBucket {
                        start_time: now - chrono::Duration::hours(age_hours),
                        pass: true,
                        message: String::new(),
                        validations: HashMap::new(),
                        observations: HashMap::new(),
                    })
                    .collect();
                history.append(&mut snapshot.history);
                snapshot.history = history;

                table
                    .insert(
                        (own, probe_name.clone()),
                        (version, rmp_serde::to_vec_named(&snapshot).unwrap().as_slice()),
                    )
                    .unwrap();
            }
            txn.commit().unwrap();
        }

        state
            .update_probe_state(&probe_name, crate::result::ProbeResult::test())
            .await
            .unwrap();

        let probes = state.get_probe_states().await.unwrap();
        let probe = probes.get(&probe_name).expect("the probe remains pooled");
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(48);
        assert!(
            !probe.history.is_empty(),
            "recent history must survive the prune"
        );
        assert!(
            probe.history.iter().all(|h| h.start_time > cutoff),
            "buckets older than the retention window must be pruned when a result is applied (oldest retained: {:?})",
            probe.history.first().map(|h| h.start_time)
        );
    }

    /// The batched notifier scan must see exactly what the full scan sees: the name inventory
    /// covers configured and stored probes, and a filtered read returns the same pooled records
    /// as the unfiltered one, restricted to the requested names.
    #[tokio::test]
    async fn filtered_probe_scans_match_the_full_scan() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        let configured = state.get_config().probes[0].name.clone();

        // A record stored by a peer for a probe this node doesn't configure must appear too.
        {
            let txn = state.database.begin_write().unwrap();
            {
                let mut table = txn.open_table(PROBES_TABLE).unwrap();
                let peer_probe = probe_at("peer-only", chrono::Utc::now());
                let version = peer_probe.version();
                table
                    .insert(
                        (NodeID::new().into(), "peer-only".to_string()),
                        (version, rmp_serde::to_vec_named(&peer_probe).unwrap().as_slice()),
                    )
                    .unwrap();
            }
            txn.commit().unwrap();
        }

        let mut names = state.get_probe_names().await.unwrap();
        names.sort();
        let mut expected = vec![configured.clone(), "peer-only".to_string()];
        expected.sort();
        assert_eq!(names, expected);

        let full = state.get_probe_states().await.unwrap();
        for name in &names {
            let subset = state
                .get_probe_states_for(&std::iter::once(name.clone()).collect())
                .await
                .unwrap();
            assert_eq!(subset.len(), 1, "a single-name filter returns exactly that probe");
            assert_eq!(
                rmp_serde::to_vec_named(&subset[name]).unwrap(),
                rmp_serde::to_vec_named(&full[name]).unwrap(),
                "the filtered record for '{name}' must match the full scan's"
            );
        }

        assert!(
            state
                .get_probe_states_for(&std::collections::HashSet::new())
                .await
                .unwrap()
                .is_empty(),
            "an empty filter yields an empty map"
        );
    }
}
