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
    cluster::{self, ClusterStateDigest, Membership, MembershipConfig, NodeID, Versioned},
    result::ProbeResult,
};
use crate::cluster::GossipStore;

// Maps a (NodeID, Probe Name) to a tuple of (Version, MsgPack Snapshot)
const CLUSTER_FIELDS_TABLE: TableDefinition<(u128, String), (u64, &[u8])> =
    TableDefinition::new("cluster_fields");
// Stores this instance's persistent identity so that a restart resumes the same NodeID (and keeps
// advertising its existing probe state) rather than appearing as a brand-new node.
const CLUSTER_IDENTITY_TABLE: TableDefinition<&str, u128> =
    TableDefinition::new("cluster_identity");
const NODE_ID_KEY: &str = "node_id";
const GENERATION_KEY: &str = "generation";

type ProbeState = Probe;

#[derive(Clone)]
pub struct State {
    config_path: PathBuf,
    config_last_modified: Arc<Mutex<std::time::SystemTime>>,

    config: Arc<RwLock<Arc<Config>>>,

    node_id: NodeID,
    database: Arc<Database>,

    /// The in-memory cluster membership registry. Peer addresses and link health are deliberately
    /// **not** persisted to the database — they are rebuilt from seed peers on restart — so this is
    /// shared (read-only for the API) rather than living in redb.
    members: Arc<Membership<NodeID, SocketAddr>>,
}

impl State {
    #[cfg(test)]
    pub async fn test(temp_dir: PathBuf) -> Self {
        // Construct through the real `new()` path so tests exercise identical setup (including
        // persisting the node identity), writing the in-memory test config to disk for it to load.
        let config = Config::test(&temp_dir);
        let config_path = temp_dir.join("config.yml");
        tokio::fs::write(&config_path, serde_yaml::to_string(&config).unwrap())
            .await
            .unwrap();

        let this = Self::new(&config_path).await.unwrap();

        let test_probe = &this.get_config().probes[0];

        this.members.record_inbound(
            &NodeID::new(),
            "127.0.0.1:12345".parse().unwrap(),
            std::time::Instant::now(),
        );
        this.update_probe_config(test_probe).await.unwrap();
        this.update_probe_state(&test_probe.name, &ProbeResult::test()).await.unwrap();

        this
    }

    pub async fn new<P: Into<PathBuf>>(config_path: P) -> Result<Self, Box<dyn Error>> {
        let config_path = config_path.into();
        let config = Config::load_from_path(&config_path).await?;

        let database = Arc::new(Database::create(config.state.clone())?);
        let node_id = Self::load_or_create_node_id(&database)?;
        let generation = Self::load_and_bump_generation(&database)?;
        let members = Arc::new(Membership::new(
            node_id,
            generation,
            config.cluster.advertised_addresses(),
            Self::membership_config(&config),
        ));

        Ok(Self {
            config_path,
            config_last_modified: Arc::new(Mutex::new(std::time::SystemTime::now())),

            config: Arc::new(RwLock::new(Arc::new(config))),

            node_id,
            database,
            members,
        })
    }

    /// Derives the in-memory membership/failure-detector tuning from the cluster configuration.
    fn membership_config(config: &Config) -> MembershipConfig {
        let cluster = &config.cluster;
        MembershipConfig {
            failure_detector_window: cluster.failure_detector_window,
            phi_prior: cluster.gossip_interval,
            phi_threshold: cluster.phi_threshold,
            gossip_factor: cluster.gossip_factor,
            dead_grace: cluster.dead_node_grace_period,
            max_addresses: cluster.max_member_addresses,
            // The floor for the "working" window; the registry scales it up with the cluster size
            // and gossip factor (see `Membership::working_window`).
            working_window: cluster.gossip_interval.saturating_mul(3),
            backoff_base: cluster.unhealthy_retry_base,
            backoff_max: cluster.unhealthy_retry_max,
            member_expiry: cluster.gc_peer_expiry,
        }
    }

    /// The shared, in-memory cluster membership registry. The gossip client uses it to track peers
    /// and link health; the API reads a redacted view of it via [`State::get_peers`].
    pub fn members(&self) -> Arc<Membership<NodeID, SocketAddr>> {
        self.members.clone()
    }

    /// Loads this instance's persistent [`NodeID`] from the database, generating and storing a fresh
    /// one on first run. Persisting the identity means a restart (via [`State::new`]) resumes the
    /// same node — continuing to advertise its probe state — instead of appearing as a new node
    /// whose old state must later be garbage-collected.
    fn load_or_create_node_id(database: &Database) -> Result<NodeID, Box<dyn Error>> {
        let read = database.begin_read()?;
        // `open_table` errors on a read transaction if the table has never been created, which is
        // the expected first-run case — fall through to generating a new identity.
        if let Ok(table) = read.open_table(CLUSTER_IDENTITY_TABLE)
            && let Some(existing) = table.get(NODE_ID_KEY)?
        {
            return Ok(NodeID::from(existing.value()));
        }
        drop(read);

        let node_id = NodeID::new();
        let id: u128 = node_id.into();
        let write = database.begin_write()?;
        {
            let mut table = write.open_table(CLUSTER_IDENTITY_TABLE)?;
            table.insert(NODE_ID_KEY, id)?;
        }
        write.commit()?;
        Ok(node_id)
    }

    /// Loads and increments this instance's persistent generation counter. The generation is a
    /// monotonic boot id: every start advances it, so a restarted node's membership record
    /// supersedes the stale one its peers still hold (whose heartbeat may be higher) without relying
    /// on any synchronised clock.
    fn load_and_bump_generation(database: &Database) -> Result<u64, Box<dyn Error>> {
        // The read-increment-write is performed within a single write transaction: redb serializes
        // write transactions, so two concurrent opens cannot read the same value and mint duplicate
        // generations.
        let write = database.begin_write()?;
        let next = {
            let mut table = write.open_table(CLUSTER_IDENTITY_TABLE)?;
            let current: u128 = table.get(GENERATION_KEY)?.map(|v| v.value()).unwrap_or(0);
            let next = current.saturating_add(1);
            table.insert(GENERATION_KEY, next)?;
            next
        };
        write.commit()?;
        Ok(next as u64)
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

    /// Returns a redacted view of the known cluster peers for the API/UI. Only the node identifier
    /// and last-seen timestamp are exposed — peer **addresses are never returned**, since the API has
    /// no access control and may be reachable on the public internet.
    pub async fn get_peers(&self) -> Result<Vec<grey_api::Peer>, Box<dyn Error>> {
        Ok(self
            .members
            .redacted_peers()
            .into_iter()
            .map(|(id, last_seen, health)| grey_api::Peer { id, last_seen, health })
            .collect())
    }

    #[instrument(name="state.gc", skip(self), fields(otel.kind = "internal", node.id=%self.node_id), err(Debug))]
    pub async fn gc(&self) -> Result<(), Box<dyn Error>> {
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
    type State = ProbeState;

    async fn id(&self) -> Result<Self::Id, Box<dyn Error>> {
        Ok(self.node_id)
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
        // Return the full delta; it is the transport's job to fit it into its frame and re-send any
        // entries that don't fit on a later round.
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

    fn probe_at(name: &str, when: chrono::DateTime<chrono::Utc>) -> ProbeState {
        Probe {
            name: name.into(),
            tags: HashMap::new(),
            last_updated: when,
            history: Vec::new(),
            observations: HashMap::new(),
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

    /// The node identity is persisted, so reopening the same state database yields the same NodeID
    /// — a restart resumes the node rather than appearing as a new one.
    #[test]
    fn node_id_persists_across_database_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.redb");

        let first = {
            let db = Database::create(&path).unwrap();
            State::load_or_create_node_id(&db).unwrap()
        };
        let second = {
            let db = Database::create(&path).unwrap();
            State::load_or_create_node_id(&db).unwrap()
        };

        assert_eq!(first, second, "NodeID must be stable across restarts");
    }

    /// The generation counter is persisted and incremented on every start, so a restarted node's
    /// membership record always supersedes the stale one its peers still hold.
    #[test]
    fn generation_increments_monotonically_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.redb");

        let g1 = {
            let db = Database::create(&path).unwrap();
            State::load_and_bump_generation(&db).unwrap()
        };
        let g2 = {
            let db = Database::create(&path).unwrap();
            State::load_and_bump_generation(&db).unwrap()
        };
        let g3 = {
            let db = Database::create(&path).unwrap();
            State::load_and_bump_generation(&db).unwrap()
        };

        assert_eq!((g1, g2, g3), (1, 2, 3), "generation must increase on each restart");
    }
}
