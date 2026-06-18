//! The extensible set of state entities replicated through the cluster gossip protocol.
//!
//! Generalising the gossip `Value` from a bare [`grey_api::Probe`] to [`ReplicatedEntity`] is what
//! lets crons (and, later, other entities such as incidents) ride the same scuttlebutt anti-entropy
//! path. Each diff entry self-describes its type via the enum variant, so a node knows which store to
//! route an incoming update to; the per-peer digest and the oldest-first diff ordering are unchanged.

use grey_api::{Cron, Probe};
use serde::{Deserialize, Serialize};

use crate::cluster::Versioned;

/// A single replicated state entity. The gossip layer is generic over `Value: Versioned`; this enum
/// is the concrete `Value` for the agent's [`super::State`], replacing the former bare `Probe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicatedEntity {
    Probe(Probe),
    Cron(Cron),
}

impl Versioned for ReplicatedEntity {
    // The diff of an entity is another entity (the same shape carrying the catch-up state), mirroring
    // `Versioned for Probe` where `Diff = Probe`.
    type Diff = ReplicatedEntity;

    fn version(&self) -> u64 {
        match self {
            ReplicatedEntity::Probe(probe) => probe.version(),
            ReplicatedEntity::Cron(cron) => cron.version(),
        }
    }

    fn diff(&self, version: u64) -> Option<Self::Diff> {
        match self {
            ReplicatedEntity::Probe(probe) => probe.diff(version).map(ReplicatedEntity::Probe),
            ReplicatedEntity::Cron(cron) => cron.diff(version).map(ReplicatedEntity::Cron),
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        match (self, diff) {
            (ReplicatedEntity::Probe(probe), ReplicatedEntity::Probe(incoming)) => {
                probe.apply(incoming)
            }
            (ReplicatedEntity::Cron(cron), ReplicatedEntity::Cron(incoming)) => cron.apply(incoming),
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
        // Matching variants merge (a newer record advances `last_updated`).
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
