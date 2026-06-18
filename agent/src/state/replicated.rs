//! The extensible set of state entities replicated through the cluster gossip protocol.
//!
//! Generalising the gossip `Value` from a bare [`grey_api::Probe`] to [`ReplicatedEntity`] is what
//! lets crons and incidents ride the same scuttlebutt anti-entropy path. Each diff entry
//! self-describes its type via the enum variant, so a node knows which store to route an incoming
//! update to; the per-peer digest and the oldest-first diff ordering are unchanged.
//!
//! Two families share this enum but are keyed and merged differently:
//! - **Per-observer** ([`Probe`]): stored under `(node_id, name)`, the gossip partition is the node
//!   component of that key, and records merge via their CRDT [`Versioned::apply`].
//! - **Global last-writer-wins** ([`GlobalLwwEntity`]: [`Cron`], [`Incident`], [`IncidentUpdate`]):
//!   stored as a single row keyed by the entity id alone, the gossip partition is the entity's
//!   `last_writer` (carried in the redb value, not the key), and conflicts resolve by the total order
//!   `(version, last_writer)`.

use grey_api::{Cron, Incident, IncidentUpdate, Probe};
use redb::TableDefinition;
use serde::{Deserialize, Serialize};

use crate::cluster::Versioned;

/// The redb value shared by every global-LWW entity table: `(version, last_writer, msgpack snapshot)`.
/// `version` is the entity's wall-clock last-modified time in milliseconds; `last_writer` is the node
/// that produced this version — the gossip partition the row is advertised under, and the LWW
/// tiebreaker. (Probes keep the older `(version, snapshot)` value shape; their partition is in the
/// key.)
pub type LwwFieldValue = (u64, u128, &'static [u8]);

/// An entity replicated under the entity-keyed, last-writer-wins model: exactly one redb row per
/// entity id, the gossip partition is `last_writer`, and conflicts resolve by `(version,
/// last_writer)`. The associated [`GlobalLwwEntity::Key`] + [`GlobalLwwEntity::TABLE`] let the
/// generic read-path gossip helpers (`digest_lww`/`emit_lww_table_diffs`) iterate any of the tables
/// regardless of key type (`String` for crons, `u64` for incidents, `u128` for incident updates);
/// the write path (`apply` + the stores) builds keys concretely from each entity's own id.
pub trait GlobalLwwEntity:
    Versioned<Diff = Self> + Serialize + serde::de::DeserializeOwned + Clone + Sized
{
    /// The redb primary key for this entity's own table.
    type Key: redb::Key + 'static;

    /// This entity's table, keyed by [`GlobalLwwEntity::Key`], valued by [`LwwFieldValue`].
    const TABLE: TableDefinition<'static, Self::Key, LwwFieldValue>;

    /// The gossip field string for this entity (its own id rendered as text) — the second coordinate
    /// of the `(partition, field)` diff entry.
    fn id_field(&self) -> String;
}

/// A single replicated state entity. The gossip layer is generic over `Value: Versioned`; this enum
/// is the concrete `Value` for the agent's [`super::State`], replacing the former bare `Probe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicatedEntity {
    Probe(Probe),
    Cron(Cron),
    Incident(Incident),
    IncidentUpdate(IncidentUpdate),
}

impl Versioned for ReplicatedEntity {
    // The diff of an entity is another entity (the same shape carrying the catch-up state), mirroring
    // `Versioned for Probe` where `Diff = Probe`.
    type Diff = ReplicatedEntity;

    fn version(&self) -> u64 {
        match self {
            ReplicatedEntity::Probe(probe) => probe.version(),
            ReplicatedEntity::Cron(cron) => cron.version(),
            ReplicatedEntity::Incident(incident) => incident.version(),
            ReplicatedEntity::IncidentUpdate(update) => update.version(),
        }
    }

    fn diff(&self, version: u64) -> Option<Self::Diff> {
        match self {
            ReplicatedEntity::Probe(probe) => probe.diff(version).map(ReplicatedEntity::Probe),
            ReplicatedEntity::Cron(cron) => cron.diff(version).map(ReplicatedEntity::Cron),
            ReplicatedEntity::Incident(incident) => {
                incident.diff(version).map(ReplicatedEntity::Incident)
            }
            ReplicatedEntity::IncidentUpdate(update) => {
                update.diff(version).map(ReplicatedEntity::IncidentUpdate)
            }
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        match (self, diff) {
            (ReplicatedEntity::Probe(probe), ReplicatedEntity::Probe(incoming)) => {
                probe.apply(incoming)
            }
            (ReplicatedEntity::Cron(cron), ReplicatedEntity::Cron(incoming)) => cron.apply(incoming),
            (ReplicatedEntity::Incident(incident), ReplicatedEntity::Incident(incoming)) => {
                incident.apply(incoming)
            }
            (
                ReplicatedEntity::IncidentUpdate(update),
                ReplicatedEntity::IncidentUpdate(incoming),
            ) => update.apply(incoming),
            // A single (node, field) entry never changes entity type, so a mismatched pair cannot
            // occur in practice; ignore it defensively rather than panicking on malformed input.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    fn at(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn probe(updated: i64) -> Probe {
        Probe {
            name: "p".into(),
            tags: HashMap::new(),
            last_updated: at(updated),
            history: vec![],
            observations: HashMap::new(),
            streak: Default::default(),
        }
    }

    fn cron(updated: i64) -> Cron {
        let mut c = Cron::from_config(
            "c",
            HashMap::new(),
            grey_api::CronSchedule::Every(Duration::from_secs(60)),
            None,
            None,
        );
        c.last_updated = at(updated);
        c
    }

    #[test]
    fn version_and_diff_delegate_to_the_inner_entity() {
        let p = ReplicatedEntity::Probe(probe(1_700));
        assert_eq!(p.version(), probe(1_700).version());
        assert!(matches!(p.diff(0), Some(ReplicatedEntity::Probe(_))));
        assert!(p.diff(p.version()).is_none(), "no diff when not newer than the request");

        let c = ReplicatedEntity::Cron(cron(1_700));
        assert_eq!(c.version(), cron(1_700).version());
        assert!(matches!(c.diff(0), Some(ReplicatedEntity::Cron(_))));
    }

    #[test]
    fn apply_merges_matching_variants_and_ignores_mismatches() {
        // Matching variants resolve by LWW (a newer record advances `last_updated`/version).
        let mut p = ReplicatedEntity::Probe(probe(1_000));
        p.apply(&ReplicatedEntity::Probe(probe(2_000)));
        assert_eq!(p.version(), probe(2_000).version());

        let mut c = ReplicatedEntity::Cron(cron(1_000));
        c.apply(&ReplicatedEntity::Cron(cron(2_000)));
        assert_eq!(c.version(), cron(2_000).version());

        // A mismatched pair is a defensive no-op: a cron diff never touches a probe.
        let mut p = ReplicatedEntity::Probe(probe(1_000));
        p.apply(&ReplicatedEntity::Cron(cron(9_999)));
        assert_eq!(p.version(), probe(1_000).version());
    }
}
