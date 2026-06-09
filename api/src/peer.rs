use serde::{Deserialize, Serialize};

/// The aggregate health of a cluster peer as seen by this node — the best state across all of the
/// peer's known addresses. Rendered in the UI as a coloured indicator.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum PeerHealth {
    /// A direct, confirmed two-way link to the peer (green).
    Online,
    /// The peer is alive in the cluster — its gossip heartbeats are advancing — but we have no
    /// confirmed direct link to it: we reach it only transitively, or our messages to it are going
    /// unanswered (blue).
    Transitive,
    /// The peer's heartbeats have slowed or stopped recently; it may be failing (orange).
    Suspect,
    /// The peer has not been heard from for a long time and is considered offline (grey).
    #[default]
    Offline,
}

impl PeerHealth {
    /// A lowercase identifier, used as a CSS class for the UI status indicator.
    pub fn as_str(&self) -> &'static str {
        match self {
            PeerHealth::Online => "online",
            PeerHealth::Transitive => "transitive",
            PeerHealth::Suspect => "suspect",
            PeerHealth::Offline => "offline",
        }
    }
}

/// Information about cluster peers as returned by the /api/v1/cluster/peers endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Peer {
    pub id: String,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_seen: chrono::DateTime<chrono::Utc>,

    /// The aggregate health of the peer. Older agents that predate this field deserialize it as
    /// [`PeerHealth::Offline`].
    #[serde(default)]
    pub health: PeerHealth,
}
