use crate::contexts::use_peers;
use crate::styles::cluster_class;
use grey_api::{Peer, PeerHealth};
use yew::prelude::*;

/// A "Cluster" entry for the header status area: a coloured indicator summarising the health of
/// the cluster, with a popover (hanging below and to the left) listing every member — including
/// the node serving this page, which is tagged as the current one. Renders nothing when no
/// members are known (for example when talking to an older agent which doesn't report itself).
#[function_component(ClusterStatus)]
pub fn cluster_status() -> Html {
    let peers_ctx = use_peers();

    if peers_ctx.peers.is_empty() {
        return html! {};
    }

    // Current node first, then healthiest, then by id for a stable order.
    let mut members = peers_ctx.peers.clone();
    members.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then_with(|| health_rank(a.health).cmp(&health_rank(b.health)))
            .then_with(|| a.id.cmp(&b.id))
    });

    let online = members
        .iter()
        .filter(|p| p.health == PeerHealth::Online)
        .count();

    let level_class = cluster_class(&members);

    html! {
        // tabindex makes the chip focusable so the popover also opens via keyboard/touch
        // (the stylesheet shows it on :hover and :focus-within).
        <div class={format!("status-indicator cluster-status {level_class}")} tabindex="0">
            <div class="status-dot active"></div>
            <span class="status-text">{"Cluster"}</span>

            <div class="cluster-popover">
                <div class="cluster-popover__content">
                    <div class="cluster-popover__title">
                        <span>{"Cluster Members"}</span>
                        <span class="cluster-popover__summary">{format!("{online}/{} online", members.len())}</span>
                    </div>
                    {for members.iter().map(render_member)}
                </div>
            </div>
        </div>
    }
}

fn render_member(peer: &Peer) -> Html {
    let class = peer.health.as_str();
    html! {
        <div class="peer">
            <div class="peer__identity">
                <div class={format!("peer__status-dot {class}")}></div>
                <span class="peer__id">{&peer.id}</span>
                if peer.current {
                    <span class="peer__current-tag">{"this node"}</span>
                }
            </div>
            <span class={format!("peer__health {class}")}>{health_label(peer.health)}</span>
            <span class="peer__last-seen">{relative_time(peer.last_seen)}</span>
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
    format!(
        "{} ago",
        crate::formatters::compact_duration(chrono::Duration::seconds(seconds))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexts::PeersProvider;

    #[derive(Properties, PartialEq)]
    struct HarnessProps {
        peers: Vec<Peer>,
    }

    /// Wraps the component in the peers context it expects, mirroring the real app tree.
    #[function_component(Harness)]
    fn harness(props: &HarnessProps) -> Html {
        html! {
            <PeersProvider peers={props.peers.clone()}>
                <ClusterStatus />
            </PeersProvider>
        }
    }

    async fn render(peers: Vec<Peer>) -> String {
        yew::ServerRenderer::<Harness>::with_props(move || HarnessProps { peers })
            .render()
            .await
    }

    fn peer(id: &str, health: PeerHealth, current: bool) -> Peer {
        Peer {
            id: id.to_string(),
            last_seen: chrono::Utc::now(),
            health,
            current,
        }
    }

    #[tokio::test]
    async fn test_renders_nothing_without_members() {
        let html = render(vec![]).await;
        assert!(!html.contains("cluster-status"), "expected no chip, got: {html}");
    }

    #[tokio::test]
    async fn test_renders_members_with_current_node_first() {
        let html = render(vec![
            peer("remote-node", PeerHealth::Online, false),
            peer("local-node", PeerHealth::Online, true),
        ])
        .await;

        assert!(html.contains("cluster-status good"), "expected a healthy chip, got: {html}");
        assert!(html.contains("2/2 online"), "expected the online summary, got: {html}");
        assert!(html.contains("this node"), "expected the current-node tag, got: {html}");

        let local = html.find("local-node").unwrap();
        let remote = html.find("remote-node").unwrap();
        assert!(local < remote, "expected the current node to be listed first, got: {html}");
    }

    #[tokio::test]
    async fn test_summarises_cluster_health() {
        let html = render(vec![
            peer("local-node", PeerHealth::Online, true),
            peer("remote-node", PeerHealth::Suspect, false),
        ])
        .await;
        assert!(html.contains("cluster-status warning"), "expected a suspect member to warn, got: {html}");

        let html = render(vec![
            peer("local-node", PeerHealth::Online, true),
            peer("remote-node", PeerHealth::Offline, false),
        ])
        .await;
        assert!(html.contains("cluster-status error"), "expected an offline member to error, got: {html}");
        assert!(html.contains("1/2 online"), "expected the online summary, got: {html}");
    }

    #[test]
    fn test_health_rank_orders_healthiest_first() {
        assert!(health_rank(PeerHealth::Online) < health_rank(PeerHealth::Transitive));
        assert!(health_rank(PeerHealth::Transitive) < health_rank(PeerHealth::Suspect));
        assert!(health_rank(PeerHealth::Suspect) < health_rank(PeerHealth::Offline));
    }

    #[test]
    fn test_health_labels() {
        assert_eq!(health_label(PeerHealth::Online), "Online");
        assert_eq!(health_label(PeerHealth::Transitive), "Transitive");
        assert_eq!(health_label(PeerHealth::Suspect), "Suspect");
        assert_eq!(health_label(PeerHealth::Offline), "Offline");
    }

    #[test]
    fn test_relative_time() {
        let now = chrono::Utc::now();
        assert_eq!(relative_time(now), "just now");
        assert_eq!(relative_time(now - chrono::Duration::seconds(42)), "42s ago");
        assert_eq!(relative_time(now - chrono::Duration::minutes(17)), "17m ago");
        assert_eq!(relative_time(now - chrono::Duration::days(5)), "5d ago");
    }
}
