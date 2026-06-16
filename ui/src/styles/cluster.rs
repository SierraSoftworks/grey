use grey_api::{Peer, PeerHealth};

/// The colour class summarising overall cluster health: `error` if any member is offline, `warning`
/// if any is suspect, otherwise `good`.
pub fn cluster_class(members: &[Peer]) -> &'static str {
    if members.iter().any(|p| p.health == PeerHealth::Offline) {
        "error"
    } else if members.iter().any(|p| p.health == PeerHealth::Suspect) {
        "warning"
    } else {
        "good"
    }
}
