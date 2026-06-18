//! Cron-state storage: the [`CronStore`] trait and its implementation over the [`State`] redb store,
//! plus the cluster [`Versioned`] / [`GlobalLwwEntity`] implementations for crons.
//!
//! Unlike probes (a per-observer record per node), a cron's check-in state is a **single global
//! record** keyed by the cron's name and resolved by whole-record last-writer-wins. A check-in
//! appends a run to the latest record this node holds and re-stamps the wall-clock version, so the
//! history accumulates on the node a cron checks in to; the rare case of the same cron checking in on
//! two nodes before gossip converges resolves by LWW (the later write wins, the other's run is
//! dropped) rather than being unioned.

use std::collections::HashMap;
use std::error::Error;

use grey_api::Cron;
use redb::{ReadableDatabase, ReadableTable, TableDefinition};

use crate::cluster::Versioned;
use crate::cron::CronCheckin;

use super::{CRON_TABLE, GlobalLwwEntity, LwwFieldValue, State};

impl Versioned for Cron {
    type Diff = Cron;

    fn version(&self) -> u64 {
        // Millisecond granularity (the precision `last_updated` serializes with), so two check-ins
        // within the same wall-clock second still produce distinct versions for the digest/diff.
        self.last_updated.timestamp_millis() as u64
    }

    fn diff(&self, version: u64) -> Option<Self::Diff> {
        // The whole record is the catch-up state under whole-record LWW.
        if self.version() > version {
            Some(self.clone())
        } else {
            None
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        // Whole-record last-writer-wins by version. The authoritative `(version, last_writer)`
        // tiebreak for equal versions is applied by the gossip store (`State::apply`), which knows the
        // incoming writer; this version-only form is the defensive fallback for the generic path.
        if diff.version() > self.version() {
            *self = diff.clone();
        }
    }
}

impl GlobalLwwEntity for Cron {
    type Key = &'static str;
    const TABLE: TableDefinition<'static, &'static str, LwwFieldValue> = CRON_TABLE;

    fn id_field(&self) -> String {
        self.name.clone()
    }
}

/// Storage operations for cron state (the cluster-replicated, gossiped check-in records).
#[allow(async_fn_in_trait)]
pub trait CronStore {
    /// The cluster-wide cron states keyed by cron name, with configuration echo fields re-stamped
    /// from local config.
    async fn get_cron_states(&self) -> Result<HashMap<String, Cron>, Box<dyn Error>>;

    /// Folds a check-in into the global record for the named cron. Returns `Ok(false)` when the cron
    /// is not present in the local configuration (a 404 for the caller).
    async fn record_cron_checkin(
        &self,
        name: &str,
        checkin: CronCheckin,
    ) -> Result<bool, Box<dyn Error>>;
}

impl CronStore for State {
    async fn get_cron_states(&self) -> Result<HashMap<String, Cron>, Box<dyn Error>> {
        let config = self.get_config();

        // Seed from local config so a cron renders before its first check-in.
        let mut crons: HashMap<String, Cron> = config
            .crons
            .iter()
            .map(|c| (c.name.clone(), c.to_cron()))
            .collect();

        let txn = self.database.begin_read()?;
        // The table only exists once something has been written to it; treat its absence as "no
        // cron state yet" rather than an error.
        if let Ok(table) = txn.open_table(CRON_TABLE) {
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (key, value) = entry;
                let name = key.value();
                let (_version, _last_writer, data) = value.value();
                if let Ok(snapshot) = rmp_serde::from_slice::<Cron>(data) {
                    // A single global record per cron — take it directly (no per-node pooling).
                    crons.insert(name.to_string(), snapshot);
                }
            }
        }

        // Configuration (cadence, thresholds, tags) is authoritative locally for display and
        // detection — re-stamp it so a peer's stale config can never override the operator's view.
        for cfg in config.crons.iter() {
            if let Some(cron) = crons.get_mut(&cfg.name) {
                cfg.stamp(cron);
            }
        }

        Ok(crons)
    }

    async fn record_cron_checkin(
        &self,
        name: &str,
        checkin: CronCheckin,
    ) -> Result<bool, Box<dyn Error>> {
        let config = self.get_config();
        let Some(cfg) = config.crons.iter().find(|c| c.name == name) else {
            return Ok(false);
        };

        let txn = self.database.begin_write()?;
        {
            let mut table = txn.open_table(CRON_TABLE)?;

            // Append to the latest global record this node holds (so history accumulates), falling
            // back to a fresh config-seeded record.
            let mut cron = table
                .get(name)?
                .and_then(|existing| {
                    let (_version, _last_writer, data) = existing.value();
                    rmp_serde::from_slice::<Cron>(data).ok()
                })
                .unwrap_or_else(|| cfg.to_cron());

            // Keep config-echo current in case the YAML changed since the last check-in.
            cfg.stamp(&mut cron);
            checkin.apply(&mut cron);

            table.insert(
                name,
                (cron.version(), self.node_id.into(), rmp_serde::to_vec_named(&cron)?.as_slice()),
            )?;
        }
        txn.commit()?;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::config::CronConfig;
    use grey_api::CronStatus;
    use std::sync::Arc;
    use std::time::Duration;

    fn checkin(status: CronStatus, message: &str) -> CronCheckin {
        CronCheckin::new(status, message.into(), chrono::Utc::now())
    }

    async fn state_with_cron(dir: &std::path::Path) -> State {
        let state = State::test(dir.to_path_buf()).await;
        let mut config = Config::test(&dir.to_path_buf());
        config.crons = vec![CronConfig {
            name: "backup".into(),
            interval: Some(Duration::from_secs(60)),
            schedule: None,
            max_duration: None,
            grace: None,
            token: None,
            tags: HashMap::new(),
        }];
        *state.config.write().unwrap() = Arc::new(config);
        state
    }

    #[tokio::test]
    async fn checkin_records_runs_and_pools_with_config_echo() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_cron(dir.path()).await;

        // A check-in for an unconfigured cron is rejected (a 404 for the caller).
        assert!(
            !state
                .record_cron_checkin("nope", checkin(CronStatus::Succeeded, ""))
                .await
                .unwrap()
        );

        assert!(
            state
                .record_cron_checkin("backup", checkin(CronStatus::Running, "start"))
                .await
                .unwrap()
        );
        assert!(
            state
                .record_cron_checkin("backup", checkin(CronStatus::Succeeded, "done"))
                .await
                .unwrap()
        );

        let pooled = state.get_cron_states().await.unwrap();
        let backup = pooled.get("backup").expect("the cron is pooled");
        assert_eq!(backup.runs.len(), 1, "running + succeeded collapse into one run");
        assert_eq!(backup.runs[0].status, CronStatus::Succeeded);
        assert_eq!(
            backup.schedule,
            grey_api::CronSchedule::Every(Duration::from_secs(60)),
            "config echo is stamped"
        );
        assert!(backup.passing(chrono::Utc::now()));
        assert_eq!(backup.last_checkin.as_ref().unwrap().message, "done");
    }

    /// A cron record received from a peer through the gossip apply path is stored as the global record
    /// and surfaces in the cluster view even on a node that has no local check-ins for it.
    #[tokio::test]
    async fn cron_replicates_through_gossip_apply() {
        use crate::cluster::{ClusterStateDiff, GossipStore, NodeID};
        use crate::state::ReplicatedEntity;

        let dir = tempfile::tempdir().unwrap();
        let state = state_with_cron(dir.path()).await;

        let mut peer_cron = grey_api::Cron::from_config(
            "backup",
            HashMap::new(),
            grey_api::CronSchedule::Every(Duration::from_secs(60)),
            None,
            None,
        );
        CronCheckin::new(CronStatus::Succeeded, "from-peer".into(), chrono::Utc::now())
            .apply(&mut peer_cron);

        let peer = NodeID::new();
        let mut diff = ClusterStateDiff::new();
        diff.update(peer, "backup".to_string(), ReplicatedEntity::Cron(peer_cron));
        state.apply(diff).await.unwrap();

        let pooled = state.get_cron_states().await.unwrap();
        let backup = pooled.get("backup").expect("the gossiped cron is pooled");
        assert_eq!(backup.runs.len(), 1);
        assert_eq!(backup.last_checkin.as_ref().unwrap().message, "from-peer");
    }

    /// Whole-record last-writer-wins: a higher-versioned record replaces a lower-versioned one
    /// regardless of run contents (no union), and a stale incoming record is ignored.
    #[tokio::test]
    async fn cron_apply_is_last_writer_wins() {
        use crate::cluster::{ClusterStateDiff, GossipStore, NodeID};
        use crate::state::ReplicatedEntity;

        let dir = tempfile::tempdir().unwrap();
        let state = state_with_cron(dir.path()).await;

        // Establish a local record by recording a check-in.
        state
            .record_cron_checkin("backup", checkin(CronStatus::Succeeded, "local"))
            .await
            .unwrap();
        let local_version = state.get_cron_states().await.unwrap()["backup"].version();

        // A stale peer record (older version) must not overwrite the local one.
        let mut stale = grey_api::Cron::from_config(
            "backup",
            HashMap::new(),
            grey_api::CronSchedule::Every(Duration::from_secs(60)),
            None,
            None,
        );
        stale.last_updated = chrono::DateTime::from_timestamp_millis(local_version as i64 - 1_000).unwrap();
        stale.last_checkin = Some(grey_api::CheckIn { at: stale.last_updated, status: CronStatus::Failed, message: "stale".into() });
        let mut diff = ClusterStateDiff::new();
        diff.update(NodeID::new(), "backup".to_string(), ReplicatedEntity::Cron(stale));
        state.apply(diff).await.unwrap();
        assert_eq!(
            state.get_cron_states().await.unwrap()["backup"].last_checkin.as_ref().unwrap().message,
            "local",
            "a stale (lower-version) record must not win"
        );
    }
}
