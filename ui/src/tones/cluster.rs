use grey_api::{Peer, PeerHealth};
use yewi::Tone;

/// The tone summarising overall cluster health: `Danger` if any member is offline, `Warn` if any is
/// suspect or reports a degraded node, otherwise `Ok`.
pub fn cluster_tone(members: &[Peer]) -> Tone {
    if members.iter().any(|p| p.health == PeerHealth::Offline) {
        Tone::Danger
    } else if members
        .iter()
        .any(|p| p.health == PeerHealth::Suspect || p.node.as_ref().is_some_and(|n| !n.healthy()))
    {
        Tone::Warn
    } else {
        Tone::Ok
    }
}

/// The tone for one member's gossip health: green online, blue transitive, orange suspect, grey
/// offline.
pub fn peer_tone(health: PeerHealth) -> Tone {
    match health {
        PeerHealth::Online => Tone::Ok,
        PeerHealth::Transitive => Tone::Info,
        PeerHealth::Suspect => Tone::Warn,
        PeerHealth::Offline => Tone::Neutral,
    }
}
