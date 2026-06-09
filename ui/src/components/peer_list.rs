use crate::contexts::use_peers;
use grey_api::PeerHealth;
use yew::prelude::*;

/// Renders the cluster peers as a list, each with a coloured health indicator. Renders nothing when
/// no peers are known (for example on a standalone, non-clustered node).
#[function_component(PeerList)]
pub fn peer_list() -> Html {
    let peers_ctx = use_peers();

    if peers_ctx.peers.is_empty() {
        return html! {};
    }

    // Healthiest first, then by id for a stable order.
    let mut peers = peers_ctx.peers.clone();
    peers.sort_by(|a, b| {
        health_rank(a.health)
            .cmp(&health_rank(b.health))
            .then_with(|| a.id.cmp(&b.id))
    });

    let online = peers
        .iter()
        .filter(|p| p.health == PeerHealth::Online)
        .count();

    html! {
        <div class="section peer-list">
            <div class="service-title">
                <h2 class="service-name">{"Cluster Peers"}</h2>
                <span class="service-availability">{format!("{online}/{} online", peers.len())}</span>
            </div>
            {for peers.iter().map(|peer| {
                let class = peer.health.as_str();
                html! {
                    <div class="peer">
                        <div class="peer-identity">
                            <div
                                class={format!("peer-status-dot {class}")}
                                tooltip={health_label(peer.health)}
                            ></div>
                            <span class="peer-id">{&peer.id}</span>
                        </div>
                        <span class={format!("peer-health {class}")}>{health_label(peer.health)}</span>
                        <span class="peer-last-seen">{relative_time(peer.last_seen)}</span>
                    </div>
                }
            })}
        </div>
    }
}

/// Sort key placing healthier peers first.
fn health_rank(health: PeerHealth) -> u8 {
    match health {
        PeerHealth::Online => 0,
        PeerHealth::Transitive => 1,
        PeerHealth::Suspect => 2,
        PeerHealth::Offline => 3,
    }
}

fn health_label(health: PeerHealth) -> &'static str {
    match health {
        PeerHealth::Online => "Online",
        PeerHealth::Transitive => "Transitive",
        PeerHealth::Suspect => "Suspect",
        PeerHealth::Offline => "Offline",
    }
}

/// A compact "x ago" rendering of when the peer was last heard from.
fn relative_time(when: chrono::DateTime<chrono::Utc>) -> String {
    let seconds = chrono::Utc::now().signed_duration_since(when).num_seconds();
    if seconds < 5 {
        return "just now".to_string();
    }
    if seconds < 60 {
        return format!("{seconds}s ago");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", hours / 24)
}
