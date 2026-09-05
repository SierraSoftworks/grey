//! Node-metadata storage: the [`NodeMetadataStore`] trait and its implementation over the [`State`]
//! redb store, plus the cluster [`Versioned`] / [`GlobalLwwEntity`] implementations for
//! [`NodeMetadata`].
//!
//! A node's metadata is the set of labels it publishes about itself — its hostname (read from the
//! operating system unless configured) and the Grey version it runs, overlaid with the operator's
//! `cluster.labels`. It is a
//! **single global record per node**, authored only by that node and resolved by last-writer-wins,
//! so it needs no per-observer pooling: peers simply hold the latest record each node advertised.
//!
//! Records age out on the probe expiry like every other replicated entity. A live node therefore
//! re-stamps its own record once it is older than half that expiry (see
//! [`NodeMetadataStore::refresh_node_metadata`], driven from the GC loop), so a running node's name
//! never disappears while a decommissioned node's does — alongside its probe records.

use std::collections::BTreeMap;
use std::error::Error;

use chrono::{DateTime, Utc};
use grey_api::{HOSTNAME_LABEL, NodeMetadata, VERSION_LABEL};
use redb::{Durability, ReadableDatabase, ReadableTable, TableDefinition};
use tracing_batteries::prelude::*;

use crate::cluster::Versioned;

use super::{GlobalLwwEntity, LwwFieldValue, NODE_METADATA_TABLE, State};

impl Versioned for NodeMetadata {
    type Diff = NodeMetadata;

    fn version(&self) -> u64 {
        // Millisecond granularity, matching the precision `last_updated` serializes with.
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
        // Whole-record last-writer-wins by version; the `(version, last_writer)` tiebreak lives in
        // the gossip store (`State::apply`), which knows the incoming writer.
        if diff.version() > self.version() {
            *self = diff.clone();
        }
    }
}

impl GlobalLwwEntity for NodeMetadata {
    type Key = &'static str;
    const TABLE: TableDefinition<'static, &'static str, LwwFieldValue> = NODE_METADATA_TABLE;

    fn id_field(&self) -> String {
        self.id.clone()
    }
}

/// Storage operations for node metadata (the labels each node publishes about itself).
#[allow(async_fn_in_trait)]
pub trait NodeMetadataStore {
    /// The labels this node should currently advertise: the operating-system hostname under
    /// `hostname` and the running Grey version under `version`, overlaid with the configured
    /// `cluster.labels` (which may override either).
    fn local_node_labels(&self) -> BTreeMap<String, String>;

    /// Publishes this node's own metadata record, rewriting it (with a fresh version, so it gossips)
    /// when the labels have changed or when the stored record is older than half the probe expiry.
    /// Returns whether a new version was written.
    async fn refresh_node_metadata(&self) -> Result<bool, Box<dyn Error>>;

    /// The metadata of every node known to this one — itself included — in identifier order.
    async fn get_node_metadata(&self) -> Result<Vec<NodeMetadata>, Box<dyn Error>>;

    /// The metadata of a single node by identifier, if known.
    async fn get_node_metadata_for(&self, id: &str) -> Result<Option<NodeMetadata>, Box<dyn Error>>;
}

/// The operating-system hostname, when it can be read and is non-empty.
fn os_hostname() -> Option<String> {
    let hostname = gethostname::gethostname().to_string_lossy().trim().to_string();
    (!hostname.is_empty()).then_some(hostname)
}

impl NodeMetadataStore for State {
    fn local_node_labels(&self) -> BTreeMap<String, String> {
        let config = self.get_config();
        let mut labels: BTreeMap<String, String> = os_hostname()
            .map(|hostname| (HOSTNAME_LABEL.to_string(), hostname))
            .into_iter()
            .collect();
        labels.insert(VERSION_LABEL.to_string(), crate::version!().to_string());
        for (key, value) in &config.cluster.labels {
            labels.insert(key.clone(), value.clone());
        }
        labels
    }

    async fn refresh_node_metadata(&self) -> Result<bool, Box<dyn Error>> {
        let id = self.node_id.to_string();
        let labels = self.local_node_labels();
        let now = Utc::now();
        // Half the expiry keeps a comfortable margin over the GC cadence, so a record is refreshed
        // several sweeps before it would otherwise be reaped.
        let stale_after = self.get_config().cluster.gc_probe_expiry / 2;
        let own_id: u128 = self.node_id.into();

        // Rare (startup, config reload, one GC pass), so it keeps immediate durability.
        self.write("refresh_node_metadata", Durability::Immediate, move |txn| {
            let mut table = txn.open_table(NODE_METADATA_TABLE)?;

            let existing = table.get(id.as_str())?.and_then(|row| {
                let (_version, _writer, data) = row.value();
                rmp_serde::from_slice::<NodeMetadata>(data).ok()
            });

            let unchanged = existing.as_ref().is_some_and(|current| {
                current.labels == labels
                    && (now - current.last_updated).to_std().unwrap_or_default() < stale_after
            });

            if unchanged {
                Ok(false)
            } else {
                // Never let the version go backwards (a clock step, or a peer having relayed a
                // record we stamped later), otherwise the rewrite would be shadowed by the old one.
                let last_updated = existing
                    .map(|current| current.last_updated + chrono::Duration::milliseconds(1))
                    .unwrap_or(now)
                    .max(now);
                let metadata = NodeMetadata::new(id.clone(), labels, last_updated);
                info!(name: "state.nodes.publish", { node.id = %id, node.labels = ?metadata.labels }, "Publishing this node's metadata");
                table.insert(
                    id.as_str(),
                    (metadata.version(), own_id, rmp_serde::to_vec_named(&metadata)?.as_slice()),
                )?;
                Ok(true)
            }
        })
        .await
    }

    async fn get_node_metadata(&self) -> Result<Vec<NodeMetadata>, Box<dyn Error>> {
        let mut nodes = Vec::new();

        let txn = self.database.begin_read()?;
        // The table only exists once something has been written to it; its absence means no node
        // has published metadata yet.
        if let Ok(table) = txn.open_table(NODE_METADATA_TABLE) {
            for entry in table.iter()?.filter_map(|r| r.ok()) {
                let (_key, value) = entry;
                let (_version, _last_writer, data) = value.value();
                if let Ok(metadata) = rmp_serde::from_slice::<NodeMetadata>(data) {
                    nodes.push(metadata);
                }
            }
        }

        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(nodes)
    }

    async fn get_node_metadata_for(&self, id: &str) -> Result<Option<NodeMetadata>, Box<dyn Error>> {
        let txn = self.database.begin_read()?;
        let Ok(table) = txn.open_table(NODE_METADATA_TABLE) else {
            return Ok(None);
        };
        Ok(table.get(id)?.and_then(|row| {
            let (_version, _last_writer, data) = row.value();
            rmp_serde::from_slice::<NodeMetadata>(data).ok()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::cluster::{ClusterStateDiff, ClusterStateDigest, GossipStore, NodeID};
    use crate::state::{ProbeStore, ReplicatedEntity};
    use std::collections::HashMap;

    async fn state_with_labels(dir: &std::path::Path, labels: &[(&str, &str)]) -> State {
        let state = State::test(dir.to_path_buf()).await;
        let mut config = Config::test(&dir.to_path_buf());
        config.cluster.labels = labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        state.set_config_for_test(config);
        state
    }

    fn peer_metadata(id: &str, hostname: &str, at: DateTime<Utc>) -> NodeMetadata {
        NodeMetadata::new(
            id,
            [(HOSTNAME_LABEL.to_string(), hostname.to_string())].into_iter().collect(),
            at,
        )
    }

    /// The local record carries the OS hostname (unless overridden), the running Grey version and
    /// the configured labels, is only rewritten when something changes, and picks up label changes
    /// with a newer version.
    #[tokio::test]
    async fn publishes_and_refreshes_the_local_record() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_labels(dir.path(), &[("region", "au-east")]).await;
        let own_id = state.node_id.to_string();

        assert!(state.refresh_node_metadata().await.unwrap(), "the first publish writes a record");
        let me = state.get_node_metadata_for(&own_id).await.unwrap().expect("the local record");
        assert_eq!(me.label("region"), Some("au-east"));
        assert_eq!(me.hostname().is_some(), os_hostname().is_some(), "the OS hostname is published when known");
        assert_eq!(me.grey_version(), Some(crate::version!()), "the running Grey version is published");
        let first_version = me.version();

        // Nothing changed: no rewrite, same version.
        assert!(!state.refresh_node_metadata().await.unwrap());
        assert_eq!(state.get_node_metadata_for(&own_id).await.unwrap().unwrap().version(), first_version);

        // A configured hostname overrides the OS one, and the change bumps the version so it gossips.
        let mut config = Config::test(&dir.path().to_path_buf());
        config.cluster.labels = HashMap::from([("hostname".to_string(), "grey-syd-1".to_string())]);
        state.set_config_for_test(config);
        assert!(state.refresh_node_metadata().await.unwrap());
        let me = state.get_node_metadata_for(&own_id).await.unwrap().unwrap();
        assert_eq!(me.display_name(), "grey-syd-1");
        assert_eq!(me.label("region"), None, "dropped labels disappear");
        assert!(me.version() > first_version);

        // The derived node view (which feeds the cluster page and node webhook events) is stamped
        // with the published labels.
        let nodes = state.get_nodes().await.unwrap();
        let me = nodes.iter().find(|n| n.id == own_id).expect("this node observes the test probe");
        assert_eq!(me.display_name(), "grey-syd-1");
        assert_eq!(me.labels.get("hostname").map(String::as_str), Some("grey-syd-1"));
    }

    /// A peer's record arriving through the gossip apply path is stored and listed; a stale
    /// (lower-version) record never overwrites a newer one; and a record advertised under a
    /// partition other than its own node is rejected.
    #[tokio::test]
    async fn replicates_through_gossip_with_lww_and_partition_checks() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_labels(dir.path(), &[]).await;
        let now = Utc::now();

        let peer = NodeID::new();
        let mut diff = ClusterStateDiff::new();
        diff.update(peer, peer.to_string(), ReplicatedEntity::NodeMetadata(peer_metadata(&peer.to_string(), "grey-lhr-1", now)));
        state.apply(diff).await.unwrap();
        assert_eq!(
            state.get_node_metadata_for(&peer.to_string()).await.unwrap().unwrap().display_name(),
            "grey-lhr-1"
        );

        // Stale record: ignored.
        let mut diff = ClusterStateDiff::new();
        diff.update(peer, peer.to_string(), ReplicatedEntity::NodeMetadata(peer_metadata(&peer.to_string(), "old-name", now - chrono::Duration::hours(1))));
        state.apply(diff).await.unwrap();
        assert_eq!(
            state.get_node_metadata_for(&peer.to_string()).await.unwrap().unwrap().display_name(),
            "grey-lhr-1"
        );

        // A record for `peer` arriving under an impostor's partition is dropped.
        let impostor = NodeID::new();
        let mut diff = ClusterStateDiff::new();
        diff.update(impostor, peer.to_string(), ReplicatedEntity::NodeMetadata(peer_metadata(&peer.to_string(), "hijacked", now + chrono::Duration::hours(1))));
        state.apply(diff).await.unwrap();
        assert_eq!(
            state.get_node_metadata_for(&peer.to_string()).await.unwrap().unwrap().display_name(),
            "grey-lhr-1"
        );

        // The listing covers the local node and the peer, in id order.
        state.refresh_node_metadata().await.unwrap();
        let listed = state.get_node_metadata().await.unwrap();
        let mut ids: Vec<_> = listed.iter().map(|m| m.id.clone()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&peer.to_string()) && ids.contains(&state.node_id.to_string()));
        ids.sort();
        assert_eq!(listed.iter().map(|m| m.id.clone()).collect::<Vec<_>>(), ids);
        assert_eq!(state.get_node_metadata_for("nope").await.unwrap(), None);
    }

    /// The local record is advertised in the digest under this node's partition and emitted by a
    /// diff against an empty digest, so peers learn it on their first exchange.
    #[tokio::test]
    async fn local_record_is_gossiped() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_labels(dir.path(), &[("cloud", "aws")]).await;
        state.refresh_node_metadata().await.unwrap();

        let digest = state.digest().await.unwrap();
        assert!(digest.get_max_version(&state.node_id).unwrap_or(0) > 0);

        let mut delta = state.diff(ClusterStateDigest::new()).await.unwrap().into_inner();
        let mine = delta.remove(&state.node_id).expect("our node's state in the diff");
        let published = mine.values().find_map(|e| match e {
            ReplicatedEntity::NodeMetadata(m) => Some(m.clone()),
            _ => None,
        });
        assert_eq!(published.expect("the metadata diff is emitted").label("cloud"), Some("aws"));
    }

    /// A live node's record is re-stamped before it can expire, while a departed peer's record is
    /// reaped by the GC on the probe expiry.
    #[tokio::test]
    async fn gc_refreshes_the_local_record_and_expires_departed_peers() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_labels(dir.path(), &[]).await;
        let mut config = Config::test(&dir.path().to_path_buf());
        config.cluster.gc_probe_expiry = std::time::Duration::from_secs(60);
        state.set_config_for_test(config);

        let peer = NodeID::new();
        let stale = Utc::now() - chrono::Duration::hours(1);
        let mut diff = ClusterStateDiff::new();
        diff.update(peer, peer.to_string(), ReplicatedEntity::NodeMetadata(peer_metadata(&peer.to_string(), "gone", stale)));
        state.apply(diff).await.unwrap();

        // Seed an own record that is past the refresh threshold (half the expiry) but not yet expired.
        let own_id = state.node_id.to_string();
        let aging = Utc::now() - chrono::Duration::seconds(45);
        {
            let txn = state.database.begin_write().unwrap();
            {
                let mut table = txn.open_table(NODE_METADATA_TABLE).unwrap();
                let record = NodeMetadata::new(own_id.clone(), state.local_node_labels(), aging);
                table
                    .insert(own_id.as_str(), (record.version(), state.node_id.into(), rmp_serde::to_vec_named(&record).unwrap().as_slice()))
                    .unwrap();
            }
            txn.commit().unwrap();
        }

        state.gc().await.unwrap();

        let me = state.get_node_metadata_for(&own_id).await.unwrap().expect("the local record survives");
        assert!(me.last_updated > aging, "the local record must be re-stamped by the sweep");
        assert!(state.get_node_metadata_for(&peer.to_string()).await.unwrap().is_none(), "a departed peer's record expires");
    }
}
