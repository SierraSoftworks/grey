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

// Incidents are stored as JSON (keyed by incident id) rather than MessagePack so that future schema
// changes can be applied as plain `serde_json::Value` transforms by the migration runner.
const INCIDENTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("incidents");
// Holds the single, global schema version for the state database (the migration marker).
const META_TABLE: TableDefinition<&str, u64> = TableDefinition::new("meta");
const SCHEMA_VERSION_KEY: &str = "schema_version";

/// A single forward migration of the state database. Each migration runs inside its own write
/// transaction together with the version bump, so a failure rolls the database back to the prior
/// version (see [`State::migrate_with`]).
type Migration = fn(&redb::WriteTransaction) -> Result<(), Box<dyn Error>>;

/// The ordered history of state-database migrations. The stored schema version is simply the number
/// of these that have been applied; new migrations are **appended** here and never reordered or
/// removed.
const MIGRATIONS: &[Migration] = &[
    m001_create_incidents, // version 0 -> 1
];

/// Establishes the incidents table. Opening it in a write transaction creates it; this is the
/// baseline migration that brings a database up to schema version 1.
fn m001_create_incidents(txn: &redb::WriteTransaction) -> Result<(), Box<dyn Error>> {
    txn.open_table(INCIDENTS_TABLE)?;
    Ok(())
}

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
        // Bring the state database schema up to date before anything reads or writes it. The only
        // way this fails is a migration that genuinely cannot be applied, in which case we refuse to
        // start rather than run against an unexpected schema.
        Self::migrate(&database)?;
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

    /// Runs any pending state-database [`MIGRATIONS`] as part of initialization. Called from
    /// [`State::new`] immediately after the database is opened.
    fn migrate(database: &Database) -> Result<(), Box<dyn Error>> {
        Self::migrate_with(database, MIGRATIONS)
    }

    /// The migration runner, parameterized over the migration list so tests can exercise failure and
    /// rollback behaviour. The stored `schema_version` records how many migrations have already been
    /// applied; only migrations beyond it run, and each is committed together with its version bump
    /// in a single transaction so a failure leaves the database exactly at the prior version. A
    /// database already at (or ahead of) the latest version is left untouched and starts normally —
    /// the only failure path is a migration that cannot be applied.
    fn migrate_with(database: &Database, migrations: &[Migration]) -> Result<(), Box<dyn Error>> {
        let current = {
            let read = database.begin_read()?;
            // A read transaction errors when the table has never been created — the fresh-database
            // case — which we treat as version 0 (before every migration).
            match read.open_table(META_TABLE) {
                Ok(table) => table.get(SCHEMA_VERSION_KEY)?.map(|v| v.value()).unwrap_or(0),
                Err(_) => 0,
            }
        };

        if current >= migrations.len() as u64 {
            return Ok(());
        }

        for (index, migration) in migrations.iter().enumerate().skip(current as usize) {
            let version = index as u64 + 1;
            let write = database.begin_write()?;

            if let Err(e) = migration(&write) {
                // Discard the partial migration so neither its changes nor the version bump persist.
                let _ = write.abort();
                return Err(
                    format!("State database migration to version {version} failed: {e}").into(),
                );
            }

            {
                let mut meta = write.open_table(META_TABLE)?;
                meta.insert(SCHEMA_VERSION_KEY, version)?;
            }
            write.commit()?;
            info!(name: "state.migrate", { schema.version = version }, "Applied state database migration to version {version}.");
        }

        Ok(())
    }

    /// Lists stored incidents, most recent first. When `include_hidden` is false, incidents that are
    /// not marked visible are omitted — this is the public, unauthenticated view.
    pub async fn list_incidents(
        &self,
        include_hidden: bool,
    ) -> Result<Vec<grey_api::Incident>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        let mut incidents = Vec::new();
        if let Ok(table) = txn.open_table(INCIDENTS_TABLE) {
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (_key, value) = entry;
                match serde_json::from_slice::<grey_api::Incident>(value.value()) {
                    Ok(incident) if include_hidden || incident.visible => incidents.push(incident),
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

    /// Fetches a single incident by id.
    pub async fn get_incident(
        &self,
        id: &str,
    ) -> Result<Option<grey_api::Incident>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        if let Ok(table) = txn.open_table(INCIDENTS_TABLE)
            && let Some(value) = table.get(id)?
        {
            return Ok(Some(serde_json::from_slice::<grey_api::Incident>(
                value.value(),
            )?));
        }
        Ok(None)
    }

    /// Creates or replaces an incident.
    pub async fn put_incident(&self, incident: &grey_api::Incident) -> Result<(), Box<dyn Error>> {
        let data = serde_json::to_vec(incident)?;
        let txn = self.database.begin_write()?;
        {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            table.insert(incident.id.as_str(), data.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Deletes an incident, returning whether a record was actually removed.
    pub async fn delete_incident(&self, id: &str) -> Result<bool, Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        let existed = {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            table.remove(id)?.is_some()
        };
        txn.commit()?;
        Ok(existed)
    }

    /// Appends a status update to an existing incident, keeping updates ordered by timestamp and
    /// refreshing `updated_at`. Returns whether the target incident exists.
    pub async fn add_incident_update(
        &self,
        id: &str,
        update: grey_api::IncidentUpdate,
    ) -> Result<bool, Box<dyn Error>> {
        let txn = self.database.begin_write()?;
        let found = {
            let mut table = txn.open_table(INCIDENTS_TABLE)?;
            // Read the existing record into an owned value so the read guard is released before we
            // insert the updated record back into the same table.
            let existing: Option<grey_api::Incident> = table
                .get(id)?
                .map(|value| serde_json::from_slice::<grey_api::Incident>(value.value()))
                .transpose()?;

            match existing {
                Some(mut incident) => {
                    incident.updates.push(update);
                    incident.updates.sort_by_key(|u| u.timestamp);
                    incident.updated_at = chrono::Utc::now();
                    let data = serde_json::to_vec(&incident)?;
                    table.insert(id, data.as_slice())?;
                    true
                }
                None => false,
            }
        };
        txn.commit()?;
        Ok(found)
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

            let own_id: u128 = self.node_id.into();

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

                    // Fold the cluster's streak into this node's own record as it arrives,
                    // so the converged "passing since" history survives the eventual garbage
                    // collection of the originating peer's record. Done here — O(1) per
                    // received diff — it replaces a per-sample scan over every peer record.
                    // The version is left untouched so this doesn't itself re-trigger gossip;
                    // the inherited streak rides along on the node's next sampled update.
                    if peer_id != own_id
                        && !probe_state.streak.is_empty()
                        && let Ok(Some(mut own)) = table_fields.get_mut((own_id, probe_name.clone()))
                    {
                        let (version, data) = own.value();
                        let mut own_state: ProbeState = rmp_serde::from_slice(data)
                            .map_err(|e| format!("Failed to parse own probe state for streak inheritance: {e:?}"))?;
                        let before = own_state.streak.clone();
                        own_state.streak.join(&probe_state.streak);
                        if own_state.streak != before {
                            own.insert((version, rmp_serde::to_vec_named(&own_state)
                                .map_err(|e| format!("Failed to serialize own probe state for streak inheritance: {e:?}"))?.as_slice()))
                                .map_err(|e| format!("Failed to store own probe state: {e:?}"))?;
                        }
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
    use crate::cluster::{Aes256Gcm, EncryptionKeyProvider, EncryptionProvider};
    use base64::prelude::*;

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
        diff.update(peer, probe.name.clone(), peer_record);
        state.apply(diff).await.unwrap();

        let pooled = state.get_probe_states().await.unwrap();
        let pooled_probe = pooled.get(&probe.name).expect("the probe to be pooled");
        assert!(pooled_probe.passing());
        assert_eq!(pooled_probe.streak.since_at(now), Some(streak_start));

        // The apply also joined the peer's register into this node's own stored
        // record, so the claim survives even if the peer's record is eventually
        // garbage-collected.
        let txn = state.database.begin_read().unwrap();
        let table = txn.open_table(CLUSTER_FIELDS_TABLE).unwrap();
        let entry = table.get((state.node_id.into(), probe.name.clone())).unwrap().unwrap();
        let (_version, data) = entry.value();
        let own_record: ProbeState = rmp_serde::from_slice(data).unwrap();
        assert_eq!(own_record.streak.covered_since, Some(streak_start));
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

    fn sample_incident(id: &str, visible: bool, start_secs: i64) -> grey_api::Incident {
        let ts = chrono::DateTime::from_timestamp(start_secs, 0).unwrap();
        grey_api::Incident {
            id: id.into(),
            title: format!("Incident {id}"),
            description: "details".into(),
            start_time: ts,
            end_time: None,
            detection_time: None,
            mitigation_time: None,
            affected_services: vec![],
            visible,
            updates: vec![],
            created_at: ts,
            updated_at: ts,
        }
    }

    fn sample_update(
        id: &str,
        status: grey_api::IncidentStatus,
        secs: i64,
    ) -> grey_api::IncidentUpdate {
        grey_api::IncidentUpdate {
            id: id.into(),
            status,
            timestamp: chrono::DateTime::from_timestamp(secs, 0).unwrap(),
            message: "update".into(),
        }
    }

    fn read_schema_version(db: &Database) -> Option<u64> {
        let read = db.begin_read().unwrap();
        match read.open_table(META_TABLE) {
            Ok(table) => table.get(SCHEMA_VERSION_KEY).unwrap().map(|v| v.value()),
            Err(_) => None,
        }
    }

    #[tokio::test]
    async fn incident_crud_and_visibility_filter() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::test(dir.path().to_path_buf()).await;

        state.put_incident(&sample_incident("a", true, 300)).await.unwrap();
        state.put_incident(&sample_incident("b", false, 200)).await.unwrap();
        state.put_incident(&sample_incident("c", true, 100)).await.unwrap();

        // The public view hides invisible incidents; the admin view shows everything, newest first.
        let public = state.list_incidents(false).await.unwrap();
        assert_eq!(public.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["a", "c"]);
        let all = state.list_incidents(true).await.unwrap();
        assert_eq!(all.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);

        // Fetch by id round-trips; an unknown id is None.
        assert_eq!(state.get_incident("b").await.unwrap().unwrap().id, "b");
        assert!(state.get_incident("missing").await.unwrap().is_none());

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

        // Add out of order; storage keeps updates sorted by timestamp.
        assert!(state
            .add_incident_update("a", sample_update("u2", grey_api::IncidentStatus::Healthy, 200))
            .await
            .unwrap());
        assert!(state
            .add_incident_update("a", sample_update("u1", grey_api::IncidentStatus::Offline, 100))
            .await
            .unwrap());

        let incident = state.get_incident("a").await.unwrap().unwrap();
        assert_eq!(
            incident.updates.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(),
            vec!["u1", "u2"]
        );
        assert_eq!(incident.current_status(), grey_api::IncidentStatus::Healthy);

        // Updating a non-existent incident reports false.
        assert!(!state
            .add_incident_update("missing", sample_update("x", grey_api::IncidentStatus::Unknown, 1))
            .await
            .unwrap());
    }

    #[test]
    fn migrate_brings_fresh_db_to_latest_version_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(dir.path().join("state.redb")).unwrap();

        State::migrate(&db).unwrap();
        let version = read_schema_version(&db);
        assert_eq!(version, Some(MIGRATIONS.len() as u64));

        // The incidents table exists after migrating.
        {
            let read = db.begin_read().unwrap();
            assert!(read.open_table(INCIDENTS_TABLE).is_ok());
        }

        // Re-running is a no-op.
        State::migrate(&db).unwrap();
        assert_eq!(read_schema_version(&db), version);
    }

    #[test]
    fn migrate_failure_aborts_and_leaves_prior_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(dir.path().join("state.redb")).unwrap();

        fn boom(_txn: &redb::WriteTransaction) -> Result<(), Box<dyn Error>> {
            Err("intentional failure".into())
        }
        // The first migration succeeds and commits version 1; the second fails.
        let migrations: &[Migration] = &[m001_create_incidents, boom];

        let result = State::migrate_with(&db, migrations);
        assert!(result.is_err(), "a failing migration must abort initialization");
        assert_eq!(
            read_schema_version(&db),
            Some(1),
            "the failed migration must not advance the version (atomic rollback)"
        );
    }

    #[test]
    fn migrate_does_not_fail_when_db_is_newer_than_binary() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(dir.path().join("state.redb")).unwrap();

        // Simulate a database written by a future binary.
        {
            let write = db.begin_write().unwrap();
            {
                let mut meta = write.open_table(META_TABLE).unwrap();
                meta.insert(SCHEMA_VERSION_KEY, 999u64).unwrap();
            }
            write.commit().unwrap();
        }

        State::migrate(&db).expect("a database newer than the binary must still start");
        assert_eq!(read_schema_version(&db), Some(999), "a newer version must be left untouched");
    }
}
