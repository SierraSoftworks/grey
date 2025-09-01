use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};
use std::error::Error;
use grey_api::{Mergeable, Probe};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use tracing::{info, instrument};
use tracing_batteries::prelude::*;

use crate::{
    Config,
    cluster::{self, ClusterStateDigest, NodeID, Versioned},
    result::ProbeResult,
};
use crate::cluster::GossipStore;

// Maps a node's address to a tuple of its (NodeID, Last Seen Timestamp)
const CLUSTER_PEERS_TABLE: TableDefinition<String, (u128, u64)> =
    TableDefinition::new("cluster_peers");
// Maps a (NodeID, Probe Name) to a tuple of (Version, MsgPack Snapshot)
const CLUSTER_FIELDS_TABLE: TableDefinition<(u128, String), (u64, &[u8])> =
    TableDefinition::new("cluster_fields");

type ProbeState = Probe;

#[derive(Clone)]
pub struct State {
    config_path: PathBuf,
    config_last_modified: Arc<Mutex<std::time::SystemTime>>,

    config: Arc<RwLock<Arc<Config>>>,

    node_id: NodeID,
    database: Arc<Database>,
}

impl State {
    #[cfg(test)]
    pub async fn test(temp_dir: PathBuf) -> Self {
        let config_path = temp_dir.join("config.yml");
        let config = Config::test(&temp_dir);
        let this = Self {
            config_path,
            config_last_modified: Arc::new(Mutex::new(std::time::SystemTime::now())),
            config: Arc::new(RwLock::new(Arc::new(config))),
            node_id: NodeID::new(),
            database: Arc::new(Database::create(temp_dir.join("state.redb")).unwrap()),
        };

        let test_probe = &this.get_config().probes[0];

        this.heartbeat(NodeID::new(), "127.0.0.1:12345".parse().unwrap())
            .await
            .unwrap();
        this.update_probe_config(test_probe).await.unwrap();
        this.update_probe_state(&test_probe.name, &&ProbeResult::test()).await.unwrap();

        this
    }

    pub async fn new<P: Into<PathBuf>>(config_path: P) -> Result<Self, Box<dyn Error>> {
        let config_path = config_path.into();
        let config = Config::load_from_path(&config_path).await?;

        let database = Arc::new(Database::create(config.state.clone())?);

        Ok(Self {
            config_path,
            config_last_modified: Arc::new(Mutex::new(std::time::SystemTime::now())),

            config: Arc::new(RwLock::new(Arc::new(config))),

            node_id: NodeID::new(),
            database,
        })
    }

    pub async fn reload(&self) -> Result<(), Box<dyn Error>> {
        let last_modified = *self.config_last_modified.lock().unwrap();
        if let Some((config, modified)) =
            Config::load_if_modified_since(&self.config_path, last_modified).await?
        {
            info!("Configuration file changed, reloading.");
            *self.config.write().unwrap() = Arc::new(config);
            *self.config_last_modified.lock().unwrap() = modified;
        }

        Ok(())
    }

    pub fn get_config(&self) -> Arc<Config> {
        self.config.read().unwrap().clone()
    }

    pub async fn get_probe_states(
        &self,
    ) -> Result<HashMap<String, Probe>, Box<dyn Error>> {
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

    pub async fn update_probe_config(
        &self,
        probe: &crate::Probe,
    ) -> Result<(), Box<dyn Error>> {
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

    pub async fn update_probe_state(
        &self,
        probe_name: &str,
        probe_result: &ProbeResult,
    ) -> Result<(), Box<dyn Error>> {
        let txn = self.database.begin_write()?;

        if let Some(probe) = self
            .get_config()
            .probes
            .iter()
            .find(|p| p.name == probe_name)
        {
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

    pub async fn get_peers(&self) -> Result<Vec<grey_api::Peer>, Box<dyn Error>> {
        let mut peers = Vec::new();

        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_PEERS_TABLE)?;
        for entry in table.iter()?.filter_map(|r| r.ok()) {
            let (_addr, info) = entry;
            let (peer_id, last_seen) = info.value();
            let peer_id = NodeID::from(peer_id);
            let last_seen =
                chrono::DateTime::from_timestamp(last_seen as i64, 0).unwrap_or_default();
            peers.push(grey_api::Peer {
                id: peer_id.to_string(),
                last_seen,
            });
        }

        Ok(peers)
    }

    #[instrument(name="state.gc", skip(self), fields(otel.kind = "internal", node.id=%self.node_id), err(Debug))]
    pub async fn gc(&self) -> Result<(), Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        {
            let mut table_peers = txn.open_table(CLUSTER_PEERS_TABLE)?;
            let mut table_fields = txn.open_table(CLUSTER_FIELDS_TABLE)?;

            let history_expiry_threshold =
                chrono::Utc::now() - self.get_config().cluster.gc_probe_expiry;
            let peer_drop_threshold = chrono::Utc::now() - self.get_config().cluster.gc_peer_expiry;

            table_peers.retain(|addr, (peer_id, last_seen)| {
                let peer_id = NodeID::from(peer_id);
                let last_seen =
                    chrono::DateTime::from_timestamp(last_seen as i64, 0).unwrap_or_default();
                if last_seen >= peer_drop_threshold {
                    true
                } else {
                    info!(
                        name: "state.gc.peer",
                        {
                            peer.id = %peer_id,
                            peer.addr = %addr,
                            peer.last_seen = %last_seen,
                        },
                        "Removing stale peer from database."
                    );
                    false
                }
            })?;

            let mut dropped_probe_records = 0;
            table_fields.retain(|(_, probe_name), (version, _data)| {
                let last_updated = chrono::DateTime::from_timestamp(version as i64, 0).unwrap_or_default();
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

    pub async fn gc_loop(&self) {
        loop {
            if let Err(err) = self.gc().await {
                warn!("Failed to perform state GC: {:?}", err);
            }

            tokio::time::sleep(self.get_config().cluster.gc_interval).await;
        }
    }
}

impl GossipStore for State {
    type Id = NodeID;
    type Address = SocketAddr;
    type State = ProbeState;

    async fn id(&self) -> Result<Self::Id, Box<dyn Error>> {
        Ok(self.node_id)
    }

    async fn heartbeat(&self, peer: Self::Id, address: Self::Address) -> Result<(), Box<dyn Error>> {
        trace!(name: "state.heartbeat", { host.node_id = %self.node_id, peer.id = %peer, peer.address = %address }, "Registering address for peer.");
        let txn = self.database.begin_write()?;
        {
            let mut table_peers = txn.open_table(CLUSTER_PEERS_TABLE)?;
            table_peers.insert(
                address.to_string(),
                (peer.into(), chrono::Utc::now().timestamp() as u64),
            )?;
        }
        txn.commit()?;
        Ok(())
    }

    async fn get_peer_addresses(&self) -> Result<Vec<Self::Address>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_PEERS_TABLE)?;
        Ok(table
            .iter()?
            .filter_map(|r| r.ok())
            .filter_map(|(addr, _info)| addr.value().parse().ok())
            .collect())
    }

    async fn digest(
        &self,
    ) -> Result<ClusterStateDigest<Self::Id>, Box<dyn Error>> {
        let mut digest = ClusterStateDigest::new();

        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_FIELDS_TABLE)?;
        for (key, value) in table.iter()?.filter_map(|r| r.ok()) {
            let (node_id, _field) = key.value();
            let (version, _data) = value.value();
            digest.update(node_id.into(), version);
        }

        trace!(name: "state.digest", { host.node_id = %self.node_id, digest = %digest }, "Composed new cluster state digest.");

        Ok(digest)
    }

    async fn diff(
        &self,
        digest: ClusterStateDigest<Self::Id>,
    ) -> Result<cluster::ClusterStateDiff<Self::Id, Self::State>, Box<dyn Error>>
    {
        let mut delta = cluster::ClusterStateDiff::new();

        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_FIELDS_TABLE)?;
        let iter = table.iter()?;
        for (key, value) in iter.filter_map(|r| r.ok()) {
            let (node_id, probe) = key.value();
            let (version, data) = value.value();

            let peer: Self::Id = node_id.into();
            let remote_version = digest.get_max_version(&peer).unwrap_or_default();

            if version <= remote_version {
                continue;
            }

            let data: ProbeState = rmp_serde::from_slice(data)
                .map_err(|e| format!("Failed to parse probe state for diff: {e:?}"))?;
            if let Some(diff) = data.diff(remote_version) {
                delta.update(peer.clone(), probe, diff);
            }
        }

        trace!(name: "state.diff", { host.node_id = %self.node_id, digest = %digest, delta = ?delta }, "Composed new cluster state diff.");

        Ok(delta)
    }

    async fn apply(
        &self,
        diff: cluster::ClusterStateDiff<Self::Id, Self::State>,
    ) -> Result<(), Box<dyn Error>> {
        trace!(name: "state.apply", { host.node_id = %self.node_id, diff = ?diff }, "Applying cluster state diff.");
        let txn = self.database.begin_write()?;
        {
            let mut table_fields = txn.open_table(CLUSTER_FIELDS_TABLE)?;

            for (peer, node_diff) in diff.into_inner() {
                let peer_id: u128 = peer.into();

                for (probe_name, probe_state) in node_diff {
                    if let Ok(Some(mut existing)) = table_fields.get_mut((peer_id, probe_name.clone())) {
                        let (_version, data) = existing.value();

                        let mut current: ProbeState = rmp_serde::from_slice(data)
                            .map_err(|e| format!("Failed to parse existing probe state for update: {e:?}"))?;
                        current.apply(&probe_state);
                        existing.insert((current.version(), rmp_serde::to_vec_named(&current)
                            .map_err(|e| format!("Failed to serialize new probe state for update: {e:?}"))?.as_slice()))
                            .map_err(|e| format!("Failed to store new probe state: {e:?}"))?;
                    } else {
                        table_fields.insert(
                            (peer_id, probe_name.clone()),
                            (
                                probe_state.version(),
                                rmp_serde::to_vec_named(&probe_state)
                                    .map_err(|e| format!("Failed to serialize new probe state for insertion: {e:?}"))?.as_slice(),
                            ),
                        )
                            .map_err(|e| format!("Failed to store new probe state: {e:?}"))?;
                    }
                }
            }
        }

        txn.commit()?;

        Ok(())
    }
}

impl Versioned for Probe {
    type Diff = Probe;

    fn version(&self) -> u64 {
        self.last_updated.timestamp() as u64
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
            })
        } else {
            None
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        self.merge(diff);
    }
}
