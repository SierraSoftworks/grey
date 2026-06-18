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
