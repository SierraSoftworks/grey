use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};
use std::error::Error;
use grey_api::{Cron, Incident, IncidentUpdate, Probe};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use tracing::info;
use tracing_batteries::prelude::*;

use crate::{
    Config,
    cluster::{self, ClusterStateDigest, Membership, MembershipConfig, NodeID, Versioned},
};
use crate::cluster::GossipStore;

// Probe-state, cron-state and incident storage live in their own sub-modules, as traits implemented
// over this `State`; the gossip/cluster plumbing remains here in the core store.
mod crons;
mod incidents;
mod probes;
mod replicated;

pub use crons::CronStore;
pub use incidents::{CasOutcome, DEFAULT_INCIDENT_PAGE, IncidentStore};
pub use probes::ProbeStore;
pub use replicated::{GlobalLwwEntity, LwwFieldValue, ReplicatedEntity};

// Maps a (NodeID, Probe Name) to a tuple of (Version, MsgPack Snapshot). Shared with the probe and
// gossip sub-modules. Probes are the one per-observer entity: every node keeps its own observation of
// each probe under this key, and the gossip partition is the node component of the key.
const PROBES_TABLE: TableDefinition<(u128, String), (u64, &[u8])> =
    TableDefinition::new("probes");

// The global last-writer-wins entity tables: one row per entity id, valued by `(version, last_writer,
// msgpack)` (see `LwwFieldValue`). Unlike probes these are *not* per-observer — a single global
// record per cron / incident / incident-update, advertised under the `last_writer` partition. New
// table names (the legacy per-node `cron_fields` and JSON `incidents` tables are abandoned, not
// migrated).
pub(crate) const CRON_TABLE: TableDefinition<&str, LwwFieldValue> =
    TableDefinition::new("crons");


pub(crate) const INCIDENTS_TABLE: TableDefinition<u64, LwwFieldValue> =
    TableDefinition::new("incidents");
pub(crate) const INCIDENT_UPDATES_TABLE: TableDefinition<u128, LwwFieldValue> =
    TableDefinition::new("incidents.updates");

// Stores this instance's persistent identity so that a restart resumes the same NodeID (and keeps
// advertising its existing probe state) rather than appearing as a brand-new node.
const INSTANCE_METADATA_TABLE: TableDefinition<&str, u128> =
    TableDefinition::new("instance_metadata");
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
        this.update_probe_state(&test_probe.name, crate::result::ProbeResult::test()).await.unwrap();

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
            phi_prior: cluster.gossip_interval,
            phi_threshold: cluster.phi_threshold,
            gossip_factor: cluster.gossip_factor,
            // The floor for the "working" window; the registry scales it up with the cluster size
            // and gossip factor (see `Membership::working_window`).
            working_window: cluster.gossip_interval.saturating_mul(3),
            reply_timeout: cluster.reply_timeout,
            peer_expiry: cluster.gc_peer_expiry,
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
        if let Ok(table) = read.open_table(INSTANCE_METADATA_TABLE)
            && let Some(existing) = table.get(NODE_ID_KEY)?
        {
            return Ok(NodeID::from(existing.value()));
        }
        drop(read);

        let node_id = NodeID::new();
        let id: u128 = node_id.into();
        let write = database.begin_write()?;
        {
            let mut table = write.open_table(INSTANCE_METADATA_TABLE)?;
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
            let mut table = write.open_table(INSTANCE_METADATA_TABLE)?;
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

    /// Returns a redacted view of the known cluster peers for the API/UI. Only the node identifier
    /// and last-seen timestamp are exposed — peer **addresses are never returned**, since the API has
    /// no access control and may be reachable on the public internet.
    pub async fn get_peers(&self) -> Result<Vec<grey_api::Peer>, Box<dyn Error>> {
        let mut peers: Vec<grey_api::Peer> = self
            .members
            .redacted_peers()
            .into_iter()
            .map(|(id, last_seen, health)| grey_api::Peer {
                id,
                last_seen,
                health,
                current: false,
            })
            .collect();

        // The registry only tracks remote peers, so add the serving node itself — the UI
        // renders the full member set, tagging this record as the current node.
        peers.push(grey_api::Peer {
            id: self.node_id.to_string(),
            last_seen: chrono::Utc::now(),
            health: grey_api::PeerHealth::Online,
            current: true,
        });

        Ok(peers)
    }

    fn generate_example_key(&self) -> String {
        use base64::prelude::*;

        let example_key: [u8; 32] = rand::random();
        BASE64_STANDARD.encode(example_key)
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

/// The redb value type shared by every entity table: `(version, msgpack snapshot)`.
type FieldValue = (u64, &'static [u8]);

/// Folds an incoming entity diff into `table` at `(node_id, name)` — merging it into the existing
/// record via [`Versioned::apply`] when one is present, or inserting it fresh otherwise. Shared by
/// the probe and cron arms of [`GossipStore::apply`] so the redb upsert plumbing lives in one place.
fn merge_into_table<T>(
    table: &mut redb::Table<(u128, String), FieldValue>,
    node_id: u128,
    name: &str,
    incoming: &T,
) -> Result<(), Box<dyn Error>>
where
    T: Versioned<Diff = T> + serde::Serialize + serde::de::DeserializeOwned,
{
    if let Ok(Some(mut existing)) = table.get_mut((node_id, name.to_string())) {
        let (_version, data) = existing.value();
        let mut current: T = rmp_serde::from_slice(data)
            .map_err(|e| format!("Failed to parse existing state for update: {e:?}"))?;
        current.apply(incoming);
        let bytes = rmp_serde::to_vec_named(&current)
            .map_err(|e| format!("Failed to serialize state for update: {e:?}"))?;
        existing
            .insert((current.version(), bytes.as_slice()))
            .map_err(|e| format!("Failed to store updated state: {e:?}"))?;
    } else {
        let bytes = rmp_serde::to_vec_named(incoming)
            .map_err(|e| format!("Failed to serialize state for insertion: {e:?}"))?;
        table
            .insert((node_id, name.to_string()), (incoming.version(), bytes.as_slice()))
            .map_err(|e| format!("Failed to store new state: {e:?}"))?;
    }
    Ok(())
}

/// Emits the per-`(node, field)` diffs for one entity table into `delta`: every record newer than the
/// peer's advertised version, wrapped into a [`ReplicatedEntity`] (the variant identifies the entity
/// type, so the field key is just the bare entity name). Shared by the probe and cron passes of
/// [`GossipStore::diff`].
fn emit_table_diffs<T>(
    txn: &redb::ReadTransaction,
    table_def: TableDefinition<(u128, String), FieldValue>,
    digest: &ClusterStateDigest<NodeID>,
    delta: &mut cluster::ClusterStateDiff<NodeID, ReplicatedEntity>,
    wrap: impl Fn(T) -> ReplicatedEntity,
) -> Result<(), Box<dyn Error>>
where
    T: Versioned<Diff = T> + serde::de::DeserializeOwned,
{
    let Ok(table) = txn.open_table(table_def) else {
        return Ok(());
    };
    for (key, value) in table.iter()?.filter_map(|r| r.ok()) {
        let (node_id, name) = key.value();
        let (version, data) = value.value();

        let peer: NodeID = node_id.into();
        let remote_version = digest.get_max_version(&peer).unwrap_or_default();
        if version <= remote_version {
            continue;
        }

        let data: T = rmp_serde::from_slice(data)
            .map_err(|e| format!("Failed to parse state for diff: {e:?}"))?;
        if let Some(diff) = data.diff(remote_version) {
            delta.update(peer, name.to_string(), wrap(diff));
        }
    }
    Ok(())
}

// --- Global last-writer-wins gossip helpers --------------------------------------------------------
//
// These mirror the probe helpers above for the [`GlobalLwwEntity`] family. The read-path helpers
// (`digest_lww`/`emit_lww_table_diffs`) are generic over the entity's [`GlobalLwwEntity::Key`]: they
// only iterate the table and read the `(version, last_writer, snapshot)` value, taking the gossip
// partition from `last_writer` (in the value) and the field key from the deserialized entity's id —
// they never build or look up a key, so they need no redb key-borrow gymnastics. The write path
// (`apply`) builds keys concretely per entity type and shares only [`lww_supersedes`].

/// Whether an incoming `(version, last_writer)` supersedes the stored one under the LWW total order.
/// The `last_writer` tiebreaker makes equal-millisecond writes converge deterministically across
/// nodes; a freshly-seen entity (no existing row) is always accepted.
fn lww_supersedes(existing: Option<(u64, u128)>, incoming: (u64, u128)) -> bool {
    match existing {
        Some(current) => incoming > current,
        None => true,
    }
}

/// Drops rows from a global-LWW table whose `version` (a `last_modified` in ms) has aged past
/// `threshold`, reaping both stale records and converged delete tombstones. Returns the count dropped.
/// Generic over the key type, since it only retains by the value's version and never inspects the key.
pub(crate) fn gc_lww_table<K: redb::Key + 'static>(
    txn: &redb::WriteTransaction,
    def: TableDefinition<K, LwwFieldValue>,
    threshold: chrono::DateTime<chrono::Utc>,
) -> Result<u64, Box<dyn Error>> {
    let mut table = txn.open_table(def)?;
    let mut dropped = 0u64;
    table.retain(|_key, (version, _writer, _data)| {
        let last_updated =
            chrono::DateTime::from_timestamp_millis(version as i64).unwrap_or_default();
        if last_updated >= threshold {
            true
        } else {
            dropped += 1;
            false
        }
    })?;
    Ok(dropped)
}

/// Folds each row of one global-LWW table into `digest` under its `last_writer` partition.
fn digest_lww<E: GlobalLwwEntity>(
    txn: &redb::ReadTransaction,
    digest: &mut ClusterStateDigest<NodeID>,
) -> Result<(), Box<dyn Error>> {
    if let Ok(table) = txn.open_table(E::TABLE) {
        for (_key, value) in table.iter()?.filter_map(|r| r.ok()) {
            let (version, last_writer, _data) = value.value();
            digest.update(NodeID::from(last_writer), version);
        }
    }
    Ok(())
}

/// Emits the diffs for one global-LWW table: every row newer than the peer's advertised version for
/// that row's `last_writer` partition, keyed by the entity's own id field.
fn emit_lww_table_diffs<E: GlobalLwwEntity>(
    txn: &redb::ReadTransaction,
    digest: &ClusterStateDigest<NodeID>,
    delta: &mut cluster::ClusterStateDiff<NodeID, ReplicatedEntity>,
    wrap: impl Fn(E) -> ReplicatedEntity,
) -> Result<(), Box<dyn Error>> {
    let Ok(table) = txn.open_table(E::TABLE) else {
        return Ok(());
    };
    for (_key, value) in table.iter()?.filter_map(|r| r.ok()) {
        let (version, last_writer, data) = value.value();

        let peer = NodeID::from(last_writer);
        let remote_version = digest.get_max_version(&peer).unwrap_or_default();
        if version <= remote_version {
            continue;
        }

        let entity: E = rmp_serde::from_slice(data)
            .map_err(|e| format!("Failed to parse global-LWW state for diff: {e:?}"))?;
        if let Some(diff) = entity.diff(remote_version) {
            let field = diff.id_field();
            delta.update(peer, field, wrap(diff));
        }
    }
    Ok(())
}

impl GossipStore for State {
    type Id = NodeID;
    type State = ReplicatedEntity;

    async fn id(&self) -> Result<Self::Id, Box<dyn Error>> {
        Ok(self.node_id)
    }

    async fn digest(
        &self,
    ) -> Result<ClusterStateDigest<Self::Id>, Box<dyn Error>> {
        let mut digest = ClusterStateDigest::new();

        let txn = self.database.begin_read()?;
        // Every entity table shares the per-peer version space (the scuttlebutt digest is a single
        // max-version per node). Probes are per-observer (partition = the node component of the key);
        // the global-LWW entities take their partition from each row's `last_writer`.
        if let Ok(table) = txn.open_table(PROBES_TABLE) {
            for (key, value) in table.iter()?.filter_map(|r| r.ok()) {
                let (node_id, _field) = key.value();
                let (version, _data) = value.value();
                digest.update(node_id.into(), version);
            }
        }
        digest_lww::<Cron>(&txn, &mut digest)?;
        digest_lww::<Incident>(&txn, &mut digest)?;
        digest_lww::<IncidentUpdate>(&txn, &mut digest)?;

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
        emit_table_diffs::<Probe>(&txn, PROBES_TABLE, &digest, &mut delta, ReplicatedEntity::Probe)?;
        emit_lww_table_diffs::<Cron>(&txn, &digest, &mut delta, ReplicatedEntity::Cron)?;
        emit_lww_table_diffs::<Incident>(&txn, &digest, &mut delta, ReplicatedEntity::Incident)?;
        emit_lww_table_diffs::<IncidentUpdate>(&txn, &digest, &mut delta, ReplicatedEntity::IncidentUpdate)?;

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
            let mut probe_table = txn.open_table(PROBES_TABLE)?;
            let mut cron_table = txn.open_table(CRON_TABLE)?;
            let mut incident_table = txn.open_table(INCIDENTS_TABLE)?;
            let mut update_table = txn.open_table(INCIDENT_UPDATES_TABLE)?;

            let own_id: u128 = self.node_id.into();

            for (peer, node_diff) in diff.into_inner() {
                let peer_id: u128 = peer.into();

                for (field, entity) in node_diff {
                    // The `ReplicatedEntity` variant routes the update to the right table. Probes are
                    // per-observer (folded via the CRDT `merge_into_table` and keyed by the diff's
                    // node); the global-LWW entities are keyed by their own id and resolved by the
                    // `(version, last_writer)` total order, where the diff's partition (`peer_id`) is
                    // the incoming `last_writer`.
                    match entity {
                        ReplicatedEntity::Probe(incoming) => {
                            merge_into_table(&mut probe_table, peer_id, &field, &incoming)?;

                            // Fold the cluster's streak into this node's own record as it arrives,
                            // so the converged "passing since" history survives the eventual garbage
                            // collection of the originating peer's record. Done here — O(1) per
                            // received diff — it replaces a per-sample scan over every peer record.
                            // The version is left untouched so this doesn't itself re-trigger gossip;
                            // the inherited streak rides along on the node's next sampled update.
                            if peer_id != own_id
                                && !incoming.streak.is_empty()
                                && let Ok(Some(mut own)) = probe_table.get_mut((own_id, field.clone()))
                            {
                                let (version, data) = own.value();
                                let mut own_state: ProbeState = rmp_serde::from_slice(data)
                                    .map_err(|e| format!("Failed to parse own probe state for streak inheritance: {e:?}"))?;
                                let before = own_state.streak.clone();
                                own_state.streak.join(&incoming.streak);
                                if own_state.streak != before {
                                    own.insert((version, rmp_serde::to_vec_named(&own_state)
                                        .map_err(|e| format!("Failed to serialize own probe state for streak inheritance: {e:?}"))?.as_slice()))
                                        .map_err(|e| format!("Failed to store own probe state: {e:?}"))?;
                                }
                            }
                        }
                        ReplicatedEntity::Cron(incoming) => {
                            let existing = cron_table
                                .get(incoming.name.as_str())?
                                .map(|g| { let (v, w, _) = g.value(); (v, w) });
                            if lww_supersedes(existing, (incoming.version(), peer_id)) {
                                let bytes = rmp_serde::to_vec_named(&incoming)
                                    .map_err(|e| format!("Failed to serialize cron for update: {e:?}"))?;
                                cron_table.insert(incoming.name.as_str(), (incoming.version(), peer_id, bytes.as_slice()))?;
                            }
                        }
                        ReplicatedEntity::Incident(incoming) => {
                            let key: u64 = incoming.id.into();
                            let existing = incident_table
                                .get(key)?
                                .map(|g| { let (v, w, _) = g.value(); (v, w) });
                            if lww_supersedes(existing, (incoming.version(), peer_id)) {
                                let bytes = rmp_serde::to_vec_named(&incoming)
                                    .map_err(|e| format!("Failed to serialize incident for update: {e:?}"))?;
                                incident_table.insert(key, (incoming.version(), peer_id, bytes.as_slice()))?;
                            }
                        }
                        ReplicatedEntity::IncidentUpdate(incoming) => {
                            let key: u128 = incoming.id.into();
                            let existing = update_table
                                .get(key)?
                                .map(|g| { let (v, w, _) = g.value(); (v, w) });
                            if lww_supersedes(existing, (incoming.version(), peer_id)) {
                                let bytes = rmp_serde::to_vec_named(&incoming)
                                    .map_err(|e| format!("Failed to serialize incident update for update: {e:?}"))?;
                                update_table.insert(key, (incoming.version(), peer_id, bytes.as_slice()))?;
                            }
                        }
                    }
                }
            }
        }

        txn.commit()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{Aes256Gcm, EncryptionKeyProvider, EncryptionProvider};
    use base64::prelude::*;
    use std::collections::HashMap;

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

    /// A record received from a peer carries the cluster's streak register; the gossip
    /// `apply` path folds it both into our copy of that peer's record and into our own
    /// record, so pooling surfaces the inherited coverage — this is what lets rolling
    /// restarts keep the "passing since" history without any per-peer bookkeeping.
    #[tokio::test]
    async fn streak_is_inherited_through_gossip() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;
        let probe = state.get_config().probes[0].clone();

        // Truncated to milliseconds, since that's the precision the markers survive
        // serialization with.
        let now = chrono::DateTime::from_timestamp_millis(chrono::Utc::now().timestamp_millis()).unwrap();
        let streak_start = now - chrono::Duration::days(3);

        // A peer's record attests three days of coverage.
        let peer = NodeID::new();
        let mut peer_record = probe_at(&probe.name, now);
        peer_record.streak.observe(true, streak_start);

        // Deliver the peer's record through the normal gossip apply path. This stores the
        // peer's record and folds its streak into this node's own record in one step.
        let mut diff = cluster::ClusterStateDiff::new();
        diff.update(peer, probe.name.clone(), ReplicatedEntity::Probe(peer_record));
        state.apply(diff).await.unwrap();

        let pooled = state.get_probe_states().await.unwrap();
        let pooled_probe = pooled.get(&probe.name).expect("the probe to be pooled");
        assert!(pooled_probe.passing());
        assert_eq!(pooled_probe.streak.since_at(now), Some(streak_start));

        // The apply also joined the peer's register into this node's own stored
        // record, so the claim survives even if the peer's record is eventually
        // garbage-collected.
        let txn = state.database.begin_read().unwrap();
        let table = txn.open_table(PROBES_TABLE).unwrap();
        let entry = table.get((state.node_id.into(), probe.name.clone())).unwrap().unwrap();
        let (_version, data) = entry.value();
        let own_record: ProbeState = rmp_serde::from_slice(data).unwrap();
        assert_eq!(own_record.streak.covered_since, Some(streak_start));
    }

    /// `digest` summarises both entity tables, and `diff` against an empty digest emits this node's
    /// probe *and* cron records — exercising the gossip read path for both entity types.
    #[tokio::test]
    async fn digest_and_diff_cover_both_probe_and_cron_tables() {
        use crate::config::CronConfig;
        use crate::cron::CronCheckin;
        use grey_api::CronStatus;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        // `State::test` already recorded a probe sample for this node; add a configured cron + a
        // check-in so both tables hold state for the local node.
        let mut config = Config::test(&dir.path().to_path_buf());
        config.crons = vec![CronConfig {
            name: "backup".into(),
            interval: Some(std::time::Duration::from_secs(60)),
            schedule: None,
            max_duration: None,
            grace: None,
            token: None,
            tags: HashMap::new(),
            visible: crate::config::default_visible_filter(),
        }];
        *state.config.write().unwrap() = Arc::new(config);
        state
            .record_cron_checkin(
                "backup",
                CronCheckin::new(CronStatus::Succeeded, "ok".into(), chrono::Utc::now()),
            )
            .await
            .unwrap();

        // The digest summarises this node with a non-zero max version (across both tables).
        let digest = state.digest().await.unwrap();
        assert!(digest.get_max_version(&state.node_id).unwrap_or(0) > 0);

        // Diffing against an empty digest emits both the probe and the cron record for this node.
        let mut delta = state.diff(ClusterStateDigest::new()).await.unwrap().into_inner();
        let node_diff = delta.remove(&state.node_id).expect("our node's state in the diff");
        assert!(
            node_diff.values().any(|e| matches!(e, ReplicatedEntity::Probe(_))),
            "the probe diff should be emitted"
        );
        assert!(
            node_diff.values().any(|e| matches!(e, ReplicatedEntity::Cron(_))),
            "the cron diff should be emitted"
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
