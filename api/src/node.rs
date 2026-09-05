use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Probe, Quorum};

/// The derived health of a single Grey node (an observer), judged by how its own view of the probes
/// it runs compares with the cluster's quorum view of the same probes.
///
/// This is a pure function of the pooled probe state and the current time (see [`Node::derive`]),
/// so every node in the cluster derives the same answer for every node — including itself — from
/// its own replica, without any coordinator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// The node identifier, as it appears in probe `observations` and on the cluster page.
    pub id: String,

    pub status: NodeStatus,

    /// When the node entered its current status, when known: the onset of the degradation, the
    /// last sample before it fell silent, or the most recent recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,

    /// The most recent sample this node recorded for any of its probes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<DateTime<Utc>>,

    /// The node's view of each probe it observes, keyed by probe name.
    #[serde(default)]
    pub probes: BTreeMap<String, NodeProbe>,

    /// How many of the node's probes it reports failing while the cluster quorum reads them passing.
    pub disagreeing: usize,

    /// How many probes the node observes.
    pub total: usize,

    /// How many disagreeing probes it takes for the node to read as degraded.
    pub quorum: usize,

    /// The labels the node publishes about itself (see [`crate::NodeMetadata`]): its hostname and
    /// any configured `cluster.labels`. Empty when the node has published none (or the record has
    /// not reached this replica yet). Stamped by the agent after derivation, not part of it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// One probe as seen from a single node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeProbe {
    /// Whether this node's own (debounced) streak reads the probe as failing.
    pub failing: bool,

    /// Whether the cluster's quorum reads the probe as failing.
    pub cluster_failing: bool,

    /// When this node's own view of the probe entered its current state, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
}

impl NodeProbe {
    /// The node reports a failure the cluster does not agree with: the signature of a bad vantage
    /// point rather than a genuine outage.
    pub fn disagrees(&self) -> bool {
        self.failing && !self.cluster_failing
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    /// The node's view of its probes agrees with the cluster (or it is the cluster).
    #[default]
    Healthy,
    /// A quorum of the node's probes fail from its vantage point while the cluster reads them
    /// passing — the node, not the services, is most likely at fault.
    Degraded,
    /// The node has not recorded a sample for any of its probes for longer than the configured
    /// silence threshold; its gossip has stopped reaching the cluster.
    Silent,
}

impl NodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeStatus::Healthy => "healthy",
            NodeStatus::Degraded => "degraded",
            NodeStatus::Silent => "silent",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeStatus::Healthy => "Healthy",
            NodeStatus::Degraded => "Degraded",
            NodeStatus::Silent => "Silent",
        }
    }

    /// Collapses the status onto the pass/fail axis used for webhook transitions.
    pub fn healthy(self) -> bool {
        matches!(self, NodeStatus::Healthy)
    }
}

impl Node {
    /// Derives every observer's health from the pooled probes.
    ///
    /// For each observer, every probe it has a (non-retired) record for contributes a [`NodeProbe`]
    /// comparing the observer's own debounced streak with the probe's quorum-derived health. The
    /// observer is `degraded` once `quorum` of its probes disagree with the cluster, or `silent` when
    /// none of its probes has seen a sample within `silent_after` (when set). Nodes are returned in
    /// identifier order.
    pub fn derive<'a>(
        probes: impl IntoIterator<Item = &'a Probe>,
        now: DateTime<Utc>,
        quorum: Quorum,
        silent_after: Option<chrono::Duration>,
    ) -> Vec<Node> {
        let mut nodes: BTreeMap<String, Node> = BTreeMap::new();

        for probe in probes {
            let window = probe.window();
            let cluster_failing = !probe.healthy_at(now, window);

            for (observer, state) in &probe.observers {
                let node = nodes.entry(observer.clone()).or_insert_with(|| Node {
                    id: observer.clone(),
                    status: NodeStatus::Healthy,
                    since: None,
                    last_updated: None,
                    probes: BTreeMap::new(),
                    disagreeing: 0,
                    total: 0,
                    quorum: 0,
                    labels: BTreeMap::new(),
                });

                node.last_updated = node.last_updated.max(Some(state.last_updated));
                node.probes.insert(
                    probe.name.clone(),
                    NodeProbe {
                        failing: state.streak.failing_for(now, window),
                        cluster_failing,
                        since: state.streak.since_at(now, window),
                    },
                );
            }
        }

        for node in nodes.values_mut() {
            node.total = node.probes.len();
            node.disagreeing = node.probes.values().filter(|p| p.disagrees()).count();
            node.quorum = quorum.required(node.total);

            let silent = silent_after
                .zip(node.last_updated)
                .map(|(threshold, last)| now - last > threshold)
                .unwrap_or(false);

            if silent {
                node.status = NodeStatus::Silent;
                node.since = node.last_updated;
            } else if node.disagreeing >= node.quorum {
                node.status = NodeStatus::Degraded;
                // The degradation began when the quorum-th disagreeing probe started failing.
                let mut onsets: Vec<_> = node
                    .probes
                    .values()
                    .filter(|p| p.disagrees())
                    .filter_map(|p| p.since)
                    .collect();
                onsets.sort();
                node.since = onsets.get(node.quorum - 1).or(onsets.last()).copied();
            } else {
                node.status = NodeStatus::Healthy;
                // Healthy since the most recent probe (from this vantage point) stopped failing.
                node.since = node
                    .probes
                    .values()
                    .filter(|p| !p.failing)
                    .filter_map(|p| p.since)
                    .max();
            }
        }

        nodes.into_values().collect()
    }

    pub fn healthy(&self) -> bool {
        self.status.healthy()
    }

    pub fn status_token(&self) -> &'static str {
        self.status.as_str()
    }

    /// The name to show for the node: its published hostname when it has one, else its identifier.
    pub fn display_name(&self) -> &str {
        self.labels
            .get(crate::HOSTNAME_LABEL)
            .map(String::as_str)
            .filter(|h| !h.trim().is_empty())
            .unwrap_or(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ObserverState, Streak};
    use std::collections::HashMap;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    const NOW: i64 = 100_000;
    const WINDOW: i64 = 300;

    fn failing_observer(since: i64) -> ObserverState {
        ObserverState {
            streak: Streak { failing_since: Some(ts(since)), failing_until: Some(ts(NOW)), covered_since: None },
            last_updated: ts(NOW),
        }
    }

    fn passing_observer(last_updated: i64) -> ObserverState {
        ObserverState {
            streak: Streak { failing_since: None, failing_until: None, covered_since: Some(ts(0)) },
            last_updated: ts(last_updated),
        }
    }

    /// A probe observed by `a`, `b` and `c`; each entry says whether that observer reads it failing.
    fn probe(name: &str, views: [bool; 3]) -> Probe {
        let observers = ["a", "b", "c"]
            .into_iter()
            .zip(views)
            .map(|(id, failing)| {
                let state = if failing { failing_observer(NOW - 2 * WINDOW) } else { passing_observer(NOW) };
                (id.to_string(), state)
            })
            .collect();
        Probe {
            name: name.into(),
            tags: HashMap::new(),
            last_updated: ts(NOW),
            history: Vec::new(),
            observations: HashMap::new(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers,
            quorum: None,
        }
    }

    fn status_of(nodes: &[Node], id: &str) -> NodeStatus {
        nodes.iter().find(|n| n.id == id).map(|n| n.status).expect("node must be derived")
    }

    #[test]
    fn a_node_disagreeing_with_the_cluster_on_a_quorum_of_its_probes_is_degraded() {
        // `a` sees every probe failing while `b` and `c` see them passing: `a` is the problem.
        let probes = [probe("web", [true, false, false]), probe("api", [true, false, false]), probe("db", [true, false, false])];
        let nodes = Node::derive(&probes, ts(NOW), Quorum::Majority, None);

        assert_eq!(nodes.len(), 3);
        assert_eq!(status_of(&nodes, "a"), NodeStatus::Degraded);
        assert_eq!(status_of(&nodes, "b"), NodeStatus::Healthy);
        assert_eq!(status_of(&nodes, "c"), NodeStatus::Healthy);

        let a = &nodes[0];
        assert_eq!((a.disagreeing, a.total, a.quorum), (3, 3, 2));
        assert_eq!(a.since, Some(ts(NOW - 2 * WINDOW)));
        assert!(!a.healthy());
        assert_eq!(a.status_token(), "degraded");
    }

    #[test]
    fn a_genuine_outage_does_not_degrade_the_nodes_reporting_it() {
        // Everyone agrees the probes are down: that's the services' problem, not any node's.
        let probes = [probe("web", [true, true, true]), probe("api", [true, true, false])];
        let nodes = Node::derive(&probes, ts(NOW), Quorum::Majority, None);
        for node in &nodes {
            assert_eq!(node.status, NodeStatus::Healthy, "{}", node.id);
            assert_eq!(node.disagreeing, 0, "{}", node.id);
        }
        assert!(nodes[0].probes["web"].cluster_failing);
        assert!(nodes[0].probes["web"].failing);
    }

    #[test]
    fn a_minority_of_disagreeing_probes_is_tolerated() {
        let probes = [probe("web", [true, false, false]), probe("api", [false, false, false]), probe("db", [false, false, false])];
        let nodes = Node::derive(&probes, ts(NOW), Quorum::Majority, None);
        assert_eq!(status_of(&nodes, "a"), NodeStatus::Healthy);
        assert_eq!(nodes[0].disagreeing, 1);

        // A stricter (lower) quorum flips it.
        let nodes = Node::derive(&probes, ts(NOW), Quorum::Count(1), None);
        assert_eq!(status_of(&nodes, "a"), NodeStatus::Degraded);
    }

    #[test]
    fn a_node_with_no_recent_samples_is_silent() {
        let mut probes = [probe("web", [false, false, false])];
        probes[0].observers.insert("a".into(), passing_observer(NOW - 7_200));

        let nodes = Node::derive(&probes, ts(NOW), Quorum::Majority, Some(chrono::Duration::hours(1)));
        assert_eq!(status_of(&nodes, "a"), NodeStatus::Silent);
        assert_eq!(nodes[0].since, Some(ts(NOW - 7_200)));
        assert_eq!(status_of(&nodes, "b"), NodeStatus::Healthy);

        // Without a silence threshold the node is simply healthy.
        let nodes = Node::derive(&probes, ts(NOW), Quorum::Majority, None);
        assert_eq!(status_of(&nodes, "a"), NodeStatus::Healthy);
    }

    #[test]
    fn only_observers_appear_and_ordering_is_stable() {
        let mut probe = probe("web", [false, false, false]);
        probe.observers.remove("b");
        let nodes = Node::derive([&probe], ts(NOW), Quorum::Majority, None);
        assert_eq!(nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
        assert!(Node::derive(std::iter::empty(), ts(NOW), Quorum::Majority, None).is_empty());
    }

    #[test]
    fn serialises_with_a_stable_shape() {
        let probes = [probe("web", [true, false, false])];
        let nodes = Node::derive(&probes, ts(NOW), Quorum::Count(1), None);
        let json = serde_json::to_value(&nodes[0]).unwrap();
        assert_eq!(json["id"], "a");
        assert_eq!(json["status"], "degraded");
        assert_eq!(json["probes"]["web"]["failing"], true);
        assert_eq!(json["probes"]["web"]["cluster_failing"], false);
        assert_eq!(json["disagreeing"], 1);
        assert_eq!(json["quorum"], 1);
        assert!(json.get("labels").is_none(), "no labels until the agent stamps them");
        let decoded: Node = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, nodes[0]);
    }

    #[test]
    fn display_name_follows_the_hostname_label() {
        let probes = [probe("web", [false, false, false])];
        let mut node = Node::derive(&probes, ts(NOW), Quorum::Majority, None).remove(0);
        assert_eq!(node.display_name(), "a");
        node.labels.insert("hostname".into(), "grey-syd-1".into());
        assert_eq!(node.display_name(), "grey-syd-1");
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["labels"]["hostname"], "grey-syd-1");
    }
}
