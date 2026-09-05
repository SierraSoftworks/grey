use crate::contexts::{Store, use_store};
use crate::styles::cluster_class;
use grey_api::{Peer, PeerHealth};
use yew::prelude::*;

/// A "Cluster" entry for the header status area: a coloured indicator summarising the health of
/// the cluster, with a popover (hanging below and to the left) listing every member — including
/// the node serving this page, which is tagged as the current one. Members are named by the
/// hostname they publish (falling back to the node identifier) and carry their other published
/// labels as tags. Renders nothing when no members are known (for example when talking to an older
/// agent which doesn't report itself).
#[function_component(ClusterStatus)]
pub fn cluster_status() -> Html {
    let store = use_store();

    if store.peers().is_empty() {
        return html! {};
    }

    // Current node first, then healthiest (PeerHealth is ordered healthiest-first), then by id for a
    // stable order.
    let mut members = store.peers().to_vec();
    members.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then_with(|| a.health.cmp(&b.health))
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
                    {for members.iter().map(|peer| render_member(&store, peer))}
                </div>
            </div>
        </div>
    }
}

fn render_member(store: &Store, peer: &Peer) -> Html {
    let class = peer.health.as_str();
    let metadata = store.node_metadata(&peer.id);
    let name = metadata
        .map(|m| m.display_name().to_string())
        .unwrap_or_else(|| peer.id.clone());
    // The identifier stays reachable (as a tooltip) for correlating with logs and webhook payloads
    // once the hostname has replaced it on screen.
    let title = if name == peer.id { format!("Node {}", peer.id) } else { format!("Node {} ({name})", peer.id) };
    let tags: Vec<(&str, &str)> = metadata.map(|m| m.tags().collect()).unwrap_or_default();

    html! {
        <div class="peer">
            <div class="peer__identity">
                <div class={format!("peer__status-dot {class}")}></div>
                <span class="peer__name" title={title}>{name}</span>
                if peer.current {
                    <span class="peer__current-tag">{"this node"}</span>
                }
            </div>
            if let Some(node) = &peer.node {
                <span
                    class={format!("peer__node {}", node.status.as_str())}
                    title={format!("{} of {} probes disagree with the cluster (quorum {})", node.disagreeing, node.total, node.quorum)}
                >{node.status.label()}</span>
            }
            <span class={format!("peer__health {class}")}>{peer.health.label()}</span>
            <span class="peer__last-seen">{relative_time(peer.last_seen)}</span>
            if !tags.is_empty() {
                <div class="peer__labels">
                    {for tags.iter().map(|(key, value)| html! {
                        <span class="peer__label" title={format!("{key}={value}")}>
                            <span class="peer__label-key">{*key}</span>
                            <span class="peer__label-value">{*value}</span>
                        </span>
                    })}
                </div>
            }
        </div>
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
    use crate::contexts::StoreProvider;

    #[derive(Properties, PartialEq)]
    struct HarnessProps {
        peers: Vec<Peer>,
        nodes: Vec<grey_api::NodeMetadata>,
    }

    /// Wraps the component in the store it expects, seeded with the peers (and node metadata) under
    /// test.
    #[function_component(Harness)]
    fn harness(props: &HarnessProps) -> Html {
        html! {
            <StoreProvider peers={props.peers.clone()} nodes={props.nodes.clone()}>
                <ClusterStatus />
            </StoreProvider>
        }
    }

    async fn render(peers: Vec<Peer>) -> String {
        render_with_nodes(peers, vec![]).await
    }

    async fn render_with_nodes(peers: Vec<Peer>, nodes: Vec<grey_api::NodeMetadata>) -> String {
        yew::ServerRenderer::<Harness>::with_props(move || HarnessProps { peers, nodes })
            .render()
            .await
    }

    fn metadata(id: &str, labels: &[(&str, &str)]) -> grey_api::NodeMetadata {
        grey_api::NodeMetadata::new(
            id,
            labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            chrono::Utc::now(),
        )
    }

    fn peer(id: &str, health: PeerHealth, current: bool) -> Peer {
        Peer {
            id: id.to_string(),
            last_seen: chrono::Utc::now(),
            health,
            current,
            node: None,
        }
    }

    /// A member with derived observer health of `status`.
    #[tokio::test]
    async fn test_shows_node_health_and_degrades_the_chip() {
        let mut degraded = peer("remote-node", PeerHealth::Online, false);
        degraded.node = Some(grey_api::Node {
            id: "remote-node".into(),
            status: grey_api::NodeStatus::Degraded,
            since: None,
            last_updated: None,
            probes: Default::default(),
            disagreeing: 2,
            total: 3,
            quorum: 2,
        });
        let html = render(vec![peer("local-node", PeerHealth::Online, true), degraded]).await;
        assert!(html.contains("peer__node degraded"), "expected the node status badge, got: {html}");
        assert!(html.contains("Degraded"), "expected the node status label, got: {html}");
        assert!(html.contains("2 of 3 probes disagree"), "expected the disagreement summary, got: {html}");
        assert!(html.contains("cluster-status warning"), "a degraded node must warn on the chip, got: {html}");
    }

    /// A member with published metadata is named by its hostname (the raw identifier moves into the
    /// tooltip) and carries its other labels as tags; one without stays identified by its id.
    #[tokio::test]
    async fn test_names_members_by_hostname_and_shows_labels() {
        let html = render_with_nodes(
            vec![peer("1p3x9k", PeerHealth::Online, true), peer("zz9plural", PeerHealth::Online, false)],
            vec![metadata("1p3x9k", &[("hostname", "grey-syd-1"), ("region", "au-east"), ("cloud", "aws")])],
        )
        .await;

        assert!(html.contains("grey-syd-1"), "expected the hostname, got: {html}");
        assert!(!html.contains(">1p3x9k<"), "the identifier must not be shown as the name, got: {html}");
        assert!(html.contains("Node 1p3x9k (grey-syd-1)"), "expected the id in the tooltip, got: {html}");
        assert!(html.contains("peer__label-key\">region<") && html.contains("peer__label-value\">au-east<"), "expected the region tag, got: {html}");
        assert!(html.contains(">cloud<"), "expected the cloud tag, got: {html}");
        assert!(!html.contains(">hostname<"), "the hostname is the name, not a tag, got: {html}");

        // A member without metadata falls back to its identifier and shows no tags.
        assert!(html.contains(">zz9plural<"), "expected the unresolved id, got: {html}");
        assert_eq!(html.matches("peer__labels").count(), 1, "only the labelled member has a tag row, got: {html}");
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
    fn test_relative_time() {
        let now = chrono::Utc::now();
        assert_eq!(relative_time(now), "just now");
        assert_eq!(relative_time(now - chrono::Duration::seconds(42)), "42s ago");
        assert_eq!(relative_time(now - chrono::Duration::minutes(17)), "17m ago");
        assert_eq!(relative_time(now - chrono::Duration::days(5)), "5d ago");
    }
}
