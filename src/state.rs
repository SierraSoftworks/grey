use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use grey_api::{Mergeable, Probe};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use tracing::info;
use tracing_batteries::prelude::*;

use crate::{
    cluster::{self, ClusterStateDigest, NodeID, VersionedField},
    result::ProbeResult,
    Config,
};

// Maps a node's address to a tuple of its (NodeID, Last Seen Timestamp)
const CLUSTER_PEERS_TABLE: TableDefinition<String, (u128, u64)> =
    TableDefinition::new("cluster_peers");
// Maps a (NodeID, Probe Name) to a tuple of (Version, MsgPack Snapshot)
const CLUSTER_FIELDS_TABLE: TableDefinition<(u128, String), (u64, &[u8])> =
    TableDefinition::new("cluster_fields");

#[derive(Clone)]
pub struct State {
    config_path: PathBuf,
    config_last_modified: Arc<Mutex<std::time::SystemTime>>,

    config: Arc<RwLock<Arc<Config>>>,

    node_id: NodeID,
    database: Arc<Database>,
}

impl State {
    pub async fn new<P: Into<PathBuf>>(config_path: P) -> Result<Self, Box<dyn std::error::Error>> {
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

    pub async fn reload(&self) -> Result<(), Box<dyn std::error::Error>> {
        let last_modified = *self.config_last_modified.lock().unwrap();
        if let Some((config, modified)) =
            Config::load_if_modified_since(&self.config_path, last_modified).await?
        {
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
    ) -> Result<HashMap<String, Probe>, Box<dyn std::error::Error>> {
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
            if let Ok(snapshot) = rmp_serde::from_slice::<Probe>(data) {
                histories
                    .entry(probe_name.clone())
                    .and_modify(|existing: &mut Probe| {
                        existing.merge(&snapshot);
                    })
                    .or_insert_with(|| snapshot.clone());
            }
        }

        Ok(histories)
    }

    pub async fn update_probe_state(
        &self,
        probe_name: &str,
        probe_result: &ProbeResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let txn = self.database.begin_write()?;

        if let Some(probe) = self
            .get_config()
            .probes
            .iter()
            .find(|p| p.name == probe_name)
        {
            let result = {
                let mut table = txn.open_table(CLUSTER_FIELDS_TABLE)?;

                let (mut snapshot, version) = table
                    .get((self.node_id.into(), probe.name.clone()))?
                    .map(|existing| {
                        let (version, data) = existing.value();
                        match rmp_serde::from_slice::<Probe>(data) {
                            Ok(snapshot) => (snapshot, version),
                            Err(err) => {
                                warn!("Failed to deserialize probe snapshot for '{probe_name}', resetting the state: {:?}", err);
                                (probe.into(), version)
                            },
                        }
                    })
                    .unwrap_or_else(|| (probe.into(), 0));

                probe_result.apply(&mut snapshot);
                let new_data = rmp_serde::to_vec(&snapshot)?;
                table.insert(
                    (self.node_id.into(), probe.name.clone()),
                    (version + 1, new_data.as_slice()),
                )?;

                Ok(())
            };

            txn.commit()?;

            result
        } else {
            Err(format!("Probe '{probe_name}' is no longer present in the configuration, its history was not updated.").into())
        }
    }

    pub async fn gc(&self) -> Result<(), Box<dyn std::error::Error>> {
        let txn = self.database.begin_write()?;
        {
            let mut table_peers = txn.open_table(CLUSTER_PEERS_TABLE)?;
            let mut table_fields = txn.open_table(CLUSTER_FIELDS_TABLE)?;

            let history_expiry_threshold = chrono::Utc::now() - chrono::Duration::days(2);
            let peer_drop_threshold =
                (chrono::Utc::now() - chrono::Duration::minutes(30)).timestamp() as u64;

            table_peers.retain(|addr, (peer_id, last_seen)| {
                if last_seen >= peer_drop_threshold {
                    true
                } else {
                    info!(
                        "Removing stale peer {}: {} (last seen: {})",
                        NodeID::from(peer_id),
                        addr,
                        last_seen
                    );
                    false
                }
            })?;

            let mut dropped_probe_records = 0;
            table_fields.retain(|_, (_version, data)| {
                if rmp_serde::from_slice(data)
                    .map(|history: Probe| history.last_updated < history_expiry_threshold)
                    .unwrap_or(false)
                {
                    true
                } else {
                    dropped_probe_records += 1;
                    false
                }
            })?;

            info!("Dropped {} stale probe records", dropped_probe_records);
        }

        txn.commit()?;

        Ok(())
    }

    pub async fn gc_loop(&self) {
        loop {
            if let Err(err) = self.gc().await {
                warn!("Failed to perform state GC: {:?}", err);
            }

            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    }
}

impl cluster::GossipStore for State {
    type Peer = NodeID;
    type Address = SocketAddr;
    type State = Probe;

    async fn get_self_id(&self) -> Result<Self::Peer, Box<dyn std::error::Error>> {
        Ok(self.node_id)
    }

    async fn get_peer_addresses(&self) -> Result<Vec<Self::Address>, Box<dyn std::error::Error>> {
        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_PEERS_TABLE)?;
        Ok(table
            .iter()?
            .filter_map(|r| r.ok())
            .filter_map(|(addr, _info)| addr.value().parse().ok())
            .collect())
    }

    async fn get_digest(
        &self,
    ) -> Result<cluster::ClusterStateDigest<Self::Peer>, Box<dyn std::error::Error>> {
        let mut digest = ClusterStateDigest::new();

        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_FIELDS_TABLE)?;
        for (key, value) in table.iter()?.filter_map(|r| r.ok()) {
            let (node_id, _field) = key.value();
            let (version, _data) = value.value();
            digest.update(node_id.into(), version);
        }

        Ok(digest)
    }

    async fn get_delta(
        &self,
        digest: cluster::ClusterStateDigest<Self::Peer>,
    ) -> Result<cluster::ClusterStateDiff<Self::Peer, Self::State>, Box<dyn std::error::Error>>
    {
        let mut delta = cluster::ClusterStateDiff::new();

        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_FIELDS_TABLE)?;
        let iter = table.iter()?;
        for (key, value) in iter.filter_map(|r| r.ok()) {
            let (node_id, field) = key.value();
            let (version, data) = value.value();

            let peer: Self::Peer = node_id.into();

            if let Some(remote_version) = digest.get_max_version(&peer) {
                if version <= remote_version {
                    continue;
                }
            }

            let data = rmp_serde::from_slice(data)?;

            delta.update(
                peer.clone(),
                &field,
                VersionedField::new(data).with_version(version),
            );
        }

        Ok(delta)
    }

    async fn apply_diff(
        &self,
        diff: cluster::ClusterStateDiff<Self::Peer, Self::State>,
        address: Self::Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let txn = self.database.begin_write()?;
        {
            let mut table_peers = txn.open_table(CLUSTER_PEERS_TABLE)?;
            let mut table_fields = txn.open_table(CLUSTER_FIELDS_TABLE)?;

            for (peer, node_diff) in diff.into_inner() {
                let peer_id: u128 = peer.into();
                if peer != self.node_id {
                    table_peers.insert(
                        address.to_string(),
                        (peer_id, chrono::Utc::now().timestamp() as u64),
                    )?;
                }

                for (field, versioned) in node_diff.into_inner() {
                    table_fields.insert(
                        (peer_id, field.clone()),
                        (
                            versioned.version,
                            rmp_serde::to_vec(&versioned.value)?.as_slice(),
                        ),
                    )?;
                }
            }
        }

        txn.commit()?;

        Ok(())
    }
}
