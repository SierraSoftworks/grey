use crate::contexts::{Store, use_store};
use crate::tones::{cluster_tone, peer_tone};
use grey_api::{NodeStatus, Peer, PeerHealth};
use yew::prelude::*;
use yewi::prelude::*;

/// The members in display order: the current node first, then healthiest (PeerHealth is ordered
/// healthiest-first), then by id for a stable order.
fn sorted_members(peers: &[Peer]) -> Vec<Peer> {
    let mut members = peers.to_vec();
    members.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then_with(|| a.health.cmp(&b.health))
            .then_with(|| a.id.cmp(&b.id))
    });
    members
}

/// A "Cluster" entry for the app bar: a coloured indicator summarising the health of the cluster,
/// which opens a popover listing every member (see [`ClusterMembers`]). Renders nothing when no
/// members are known (for example when talking to an older agent which doesn't report itself).
#[function_component(ClusterStatus)]
pub fn cluster_status() -> Html {
    let store = use_store();

    if store.peers().is_empty() {
        return html! {};
    }

    let tone = cluster_tone(store.peers());

    html! {
        <Popover
            class={classes!("cluster-status", format!("cluster-status--{}", tone.as_str()))}
            label="Cluster members"
            placement={Placement::BottomEnd}
            content={html! { <ClusterMembers /> }}
        >
            <Button variant={ButtonVariant::Subtle}>
                <StatusDot {tone} active=true />
                <span class="cluster-status__label">{ "Cluster" }</span>
            </Button>
        </Popover>
    }
}

/// Every member of the cluster, including the node serving this page, which is tagged as the
/// current one. Members are named by the hostname they publish (falling back to the node
/// identifier) and carry their other published labels as tags.
#[function_component(ClusterMembers)]
fn cluster_members() -> Html {
    let store = use_store();
    let members = sorted_members(store.peers());
    let online = members.iter().filter(|p| p.health == PeerHealth::Online).count();

    html! {
        <div class="cluster">
            <div class="cluster__title">
                <span>{ "Cluster members" }</span>
                <span class="cluster__summary">{ format!("{online}/{} online", members.len()) }</span>
            </div>
            <div class="cluster__members">
                { for members.iter().map(|peer| render_member(&store, peer)) }
            </div>
        </div>
    }
}

fn render_member(store: &Store, peer: &Peer) -> Html {
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
                <StatusDot tone={peer_tone(peer.health)} />
                <span class="peer__name" title={title}>{ name }</span>
                if peer.current {
                    <Pill dot=false>{ "this node" }</Pill>
                }
            </div>
            <div class="peer__state">
                if let Some(node) = &peer.node {
                    <span title={format!("{} of {} probes disagree with the cluster (quorum {})", node.disagreeing, node.total, node.quorum)}>
                        <Pill
                            tone={if node.status == NodeStatus::Healthy { Tone::Ok } else { Tone::Warn }}
                            dot=false
                        >
                            { node.status.label() }
                        </Pill>
                    </span>
                }
                <Text tone={peer_tone(peer.health)} size={TextSize::Sm}>{ peer.health.label() }</Text>
                <span class="peer__last-seen">{ relative_time(peer.last_seen) }</span>
            </div>
            if !tags.is_empty() {
                <div class="peer__labels">
                    { for tags.iter().map(|(key, value)| html! {
                        <Tag name={key.to_string()}>{ *value }</Tag>
                    }) }
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
                // The popover only mounts its content once opened, which SSR can't reach.
                <ClusterMembers />
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
            labels: Default::default(),
        });
        let html = render(vec![peer("local-node", PeerHealth::Online, true), degraded]).await;
                assert!(html.contains("Degraded"), "expected the node status label, got: {html}");
        assert!(html.contains("2 of 3 probes disagree"), "expected the disagreement summary, got: {html}");
        assert!(html.contains("cluster-status--warn"), "a degraded node must warn on the chip, got: {html}");
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
        assert!(html.contains("region") && html.contains("au-east"), "expected the region tag, got: {html}");
        assert!(html.contains("cloud"), "expected the cloud tag, got: {html}");
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

        assert!(html.contains("cluster-status--ok"), "expected a healthy chip, got: {html}");
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
        assert!(html.contains("cluster-status--warn"), "expected a suspect member to warn, got: {html}");

        let html = render(vec![
            peer("local-node", PeerHealth::Online, true),
            peer("remote-node", PeerHealth::Offline, false),
        ])
        .await;
        assert!(html.contains("cluster-status--danger"), "expected an offline member to error, got: {html}");
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
