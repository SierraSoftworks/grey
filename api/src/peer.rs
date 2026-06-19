use serde::{Deserialize, Serialize};

/// The aggregate health of a cluster peer as seen by this node — the best state across all of the
/// peer's known addresses. Rendered in the UI as a coloured indicator.
///
/// The variants are declared **healthiest-first**, so the derived [`Ord`] sorts peers from healthiest
/// to least healthy — exactly the order the UI lists them in. (Construction matches by condition, not
/// by this numeric order — see the agent's `From<Signals>` — so the ordering is free to be display
/// policy.)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Debug, Default)]
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

    /// A human-readable label for display next to the indicator.
    pub fn label(&self) -> &'static str {
        match self {
            PeerHealth::Online => "Online",
            PeerHealth::Transitive => "Transitive",
            PeerHealth::Suspect => "Suspect",
            PeerHealth::Offline => "Offline",
        }
    }
}

/// Information about cluster peers as returned by the admin `/api/v1/admin/cluster/peers` endpoint.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Peer {
    pub id: String,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_seen: chrono::DateTime<chrono::Utc>,

    /// The aggregate health of the peer. Older agents that predate this field deserialize it as
    /// [`PeerHealth::Offline`].
    #[serde(default)]
    pub health: PeerHealth,

    /// Whether this record describes the node which served the API response. The membership
    /// registry only tracks remote peers, so the serving node adds itself when answering;
    /// responses from older agents deserialize this as `false`.
    #[serde(default)]
    pub current: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_orders_healthiest_first() {
        assert!(PeerHealth::Online < PeerHealth::Transitive);
        assert!(PeerHealth::Transitive < PeerHealth::Suspect);
        assert!(PeerHealth::Suspect < PeerHealth::Offline);
    }

    #[test]
    fn health_labels_and_tokens() {
        for health in [
            PeerHealth::Online,
            PeerHealth::Transitive,
            PeerHealth::Suspect,
            PeerHealth::Offline,
        ] {
            assert!(!health.label().is_empty());
            assert!(!health.as_str().is_empty());
        }
        assert_eq!(PeerHealth::Online.label(), "Online");
        assert_eq!(PeerHealth::Offline.as_str(), "offline");
    }
}
