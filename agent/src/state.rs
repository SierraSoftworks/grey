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

/// Largest `k` in `[0, n]` for which `fits(k)` holds, assuming `fits` is monotonic — `fits(0)` is
/// true and once it becomes false it stays false. Uses binary search, so it evaluates `fits`
/// only `O(log n)` times (each evaluation is one serialization attempt in [`bounded_delta`]).
fn largest_fitting_prefix(n: usize, mut fits: impl FnMut(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// Builds a [`cluster::ClusterStateDiff`] from `candidates` (each `(version, node, probe, diff)`),
/// including only as many of the *stalest* entries as fit within `budget` bytes once serialized.
///
/// Entries are sent stalest-first (lowest version) so the oldest un-propagated state drains first;
/// the remainder is carried by subsequent gossip rounds as the peer's digest advances. A single
/// entry larger than `budget` is still sent on its own (with a warning) so it cannot stall forever.
fn bounded_delta(
    mut candidates: Vec<(u64, NodeID, String, ProbeState)>,
    budget: usize,
) -> cluster::ClusterStateDiff<NodeID, ProbeState> {
    candidates.sort_by_key(|(version, _, _, _)| *version);

    // Binary-searched so we serialize O(log n) prefixes rather than re-measuring after every entry.
    let serialized_len = |count: usize| -> usize {
        let mut delta = cluster::ClusterStateDiff::<NodeID, ProbeState>::new();
        for (_, peer, probe, diff) in &candidates[..count] {
            delta.update(peer.clone(), probe.clone(), diff.clone());
        }
        rmp_serde::to_vec(&delta).map(|bytes| bytes.len()).unwrap_or(usize::MAX)
    };

    let mut take = largest_fitting_prefix(candidates.len(), |count| serialized_len(count) <= budget);
    if take == 0 && !candidates.is_empty() {
        let (_, peer, probe, _) = &candidates[0];
        warn!(
            name: "state.diff.oversized",
            { peer.id = %peer, probe.name = %probe, budget = budget },
            "Probe state exceeds the gossip datagram budget; sending it alone (it may be dropped by the network)."
        );
        take = 1;
    }

    let mut delta = cluster::ClusterStateDiff::<NodeID, ProbeState>::new();
    for (_, peer, probe, diff) in candidates.into_iter().take(take) {
        delta.update(peer, probe, diff);
    }
    delta
}

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

    fn generate_example_key(&self) -> String {
        use aes_gcm::{
            Aes256Gcm,
            aead::{KeyInit, OsRng},
        };
        use base64::prelude::*;

        let example_key = Aes256Gcm::generate_key(OsRng);
        let key: &[u8] = example_key.as_slice();
        BASE64_STANDARD.encode(key)
    }

    fn parse_secret_key(&self, secret: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
        use base64::prelude::*;

        let secret_bytes = BASE64_STANDARD
            .decode(secret.as_bytes())
            .unwrap_or_default();
        if secret_bytes.len() < 32 {
            return Err("Cluster secret key must contain 32-bytes of base64-encoded data.".into());
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&secret_bytes[..32]);
        Ok(key)
    }
}

impl cluster::EncryptionKeyProvider for State {
    type Key = [u8; 32];

    fn get_encryption_key(&self) -> Result<Self::Key, Box<dyn Error>> {
        let config = self.get_config();
        let secret = if config.cluster.secret.is_empty() {
            config.cluster.secrets.iter()
                // Encrypt with the second key in the list (the "current" key in the
                // documented rotation scheme), falling back to the first when only one
                // key is configured. See docs/guide/clustering.md ("Key Rotation").
                .nth(1)
                .or(config.cluster.secrets.first())
                .ok_or(format!("No secrets have been configured for the cluster, cannot encrypt gossip messages. You can use '{}' as a key if you need it.", self.generate_example_key()))?
        } else {
            &config.cluster.secret
        };

        self.parse_secret_key(secret)
    }

    fn get_decryption_keys(&self) -> Result<Vec<Self::Key>, Box<dyn Error>> {
        let config = self.get_config();
        let mut keys = Vec::new();
        let secret = if config.cluster.secret.is_empty() {
            None
        } else {
            Some(config.cluster.secret.clone())
        };

        for secret in secret.iter().chain(config.cluster.secrets.iter()) {
            if let Ok(key) = self.parse_secret_key(secret) {
                keys.push(key);
            } else {
                warn!("Failed to parse cluster secret key, skipping it.");
            }
        }
        if keys.is_empty() {
            Err("No valid cluster secret keys available for decryption.".into())
        } else {
            Ok(keys)
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
        max_delta_bytes: usize,
    ) -> Result<cluster::ClusterStateDiff<Self::Id, Self::State>, Box<dyn Error>>
    {
        let txn = self.database.begin_read()?;
        let table = txn.open_table(CLUSTER_FIELDS_TABLE)?;
        let iter = table.iter()?;

        // Collect every field the remote is behind on, computing each per-field diff exactly once.
        let mut candidates: Vec<(u64, Self::Id, String, Self::State)> = Vec::new();
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
                candidates.push((version, peer, probe, diff));
            }
        }

        // Bound the delta to what the caller's transport can carry in one message; the rest drains
        // over subsequent rounds so a large catch-up (e.g. after a partition) can't permanently
        // fail to deliver.
        let delta = bounded_delta(candidates, max_delta_bytes);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{Aes256Gcm, EncryptionKeyProvider, EncryptionProvider};
    use base64::prelude::*;

    #[test]
    fn largest_fitting_prefix_finds_boundary() {
        // Each entry "costs" 3; with a budget of 10 the largest fitting prefix is 3 (cost 9).
        assert_eq!(largest_fitting_prefix(5, |k| 3 * k <= 10), 3);
    }

    #[test]
    fn largest_fitting_prefix_all_or_none() {
        assert_eq!(largest_fitting_prefix(4, |_| true), 4);
        assert_eq!(largest_fitting_prefix(4, |k| k == 0), 0);
    }

    fn probe_state(name: &str, version_secs: i64) -> ProbeState {
        Probe {
            name: name.into(),
            tags: HashMap::new(),
            last_updated: chrono::DateTime::from_timestamp(version_secs, 0).unwrap(),
            history: Vec::new(),
            observations: HashMap::new(),
        }
    }

    fn serialized_size(entries: &[(NodeID, &str, ProbeState)]) -> usize {
        let mut delta = cluster::ClusterStateDiff::<NodeID, ProbeState>::new();
        for (peer, name, state) in entries {
            delta.update(*peer, (*name).to_string(), state.clone());
        }
        rmp_serde::to_vec(&delta).unwrap().len()
    }

    #[test]
    fn bounded_delta_sends_stalest_first_within_budget() {
        let node = NodeID::new();
        let candidates = vec![
            (30u64, node, "c".to_string(), probe_state("c", 30)),
            (10u64, node, "a".to_string(), probe_state("a", 10)),
            (20u64, node, "b".to_string(), probe_state("b", 20)),
        ];

        // A budget that fits exactly the two stalest entries (versions 10 and 20).
        let budget = serialized_size(&[
            (node, "a", probe_state("a", 10)),
            (node, "b", probe_state("b", 20)),
        ]);

        let inner = bounded_delta(candidates, budget).into_inner();
        let fields = inner.get(&node).expect("node present");
        assert_eq!(fields.len(), 2);
        assert!(fields.contains_key("a") && fields.contains_key("b"));
        assert!(
            !fields.contains_key("c"),
            "the freshest entry must be deferred to a later round"
        );
    }

    #[test]
    fn bounded_delta_includes_all_when_budget_large() {
        let node = NodeID::new();
        let candidates = vec![
            (10u64, node, "a".to_string(), probe_state("a", 10)),
            (20u64, node, "b".to_string(), probe_state("b", 20)),
        ];
        let inner = bounded_delta(candidates, 1_000_000).into_inner();
        assert_eq!(inner.get(&node).unwrap().len(), 2);
    }

    #[test]
    fn bounded_delta_sends_single_oversized_entry() {
        let node = NodeID::new();
        let candidates = vec![(10u64, node, "a".to_string(), probe_state("a", 10))];
        // A 1-byte budget fits nothing, but the single entry must still be sent so it cannot stall.
        let inner = bounded_delta(candidates, 1).into_inner();
        assert_eq!(inner.get(&node).unwrap().len(), 1);
    }

    fn b64_key(byte: u8) -> String {
        BASE64_STANDARD.encode([byte; 32])
    }

    async fn state_with_secrets(dir: &std::path::Path, secrets: Vec<String>) -> State {
        let state = State::test(dir.to_path_buf()).await;
        let mut config = Config::test(&dir.to_path_buf());
        config.cluster.secrets = secrets;
        *state.config.write().unwrap() = Arc::new(config);
        state
    }

    /// During a documented 3-key rotation the list is
    /// `[new (decrypt-only), current (encrypt+decrypt), old (decrypt-only)]`, so encryption must use
    /// the *second* key. A peer that has rotated one step forward (dropping `old`) still holds
    /// `current` for decryption and must be able to read the message.
    #[tokio::test]
    async fn encrypts_with_second_key_and_survives_rotation() {
        let new = b64_key(1);
        let current = b64_key(2);
        let old = b64_key(3);

        let dir_a = tempfile::tempdir().unwrap();
        let node_a = state_with_secrets(dir_a.path(), vec![new.clone(), current.clone(), old]).await;

        // Encryption uses the current (second) key, not the old (third) key.
        assert_eq!(
            node_a.get_encryption_key().unwrap(),
            node_a.parse_secret_key(&current).unwrap(),
            "expected encryption with the second (current) key"
        );

        // Node B has rotated forward and dropped `old`, but still accepts `current` for decryption.
        let newer = b64_key(0);
        let dir_b = tempfile::tempdir().unwrap();
        let node_b = state_with_secrets(dir_b.path(), vec![newer, new, current]).await;

        let provider = Aes256Gcm;
        let ciphertext = provider.encrypt(&node_a, b"probe-state").unwrap();
        let plaintext = provider.decrypt(&node_b, &ciphertext).unwrap();
        assert_eq!(plaintext, b"probe-state");
    }

    /// With a single configured key, encryption falls back to that key.
    #[tokio::test]
    async fn encrypts_with_only_key_when_single() {
        let only = b64_key(7);
        let dir = tempfile::tempdir().unwrap();
        let node = state_with_secrets(dir.path(), vec![only.clone()]).await;
        assert_eq!(
            node.get_encryption_key().unwrap(),
            node.parse_secret_key(&only).unwrap()
        );
    }
}
