use std::{collections::HashMap, fmt::{Debug, Display}, hash::Hash};

use serde::{Deserialize, Serialize};
use tracing::Span;
use tracing_batteries::prelude::OpenTelemetrySpanExt;
use crate::cluster::Versioned;
use crate::cluster::MembershipSample;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message<Peer, Value>
where
    Peer: Eq + Hash,
    Value: Versioned,
    Value::Diff: Serialize + Debug + Clone,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    Syn(MessageMetadata<Peer>, ClusterStateDigest<Peer>),
    Ack(MessageMetadata<Peer>, ClusterStateDiff<Peer, Value>),
    SynAck(MessageMetadata<Peer>, ClusterStateDigest<Peer>, ClusterStateDiff<Peer, Value>),
    /// A fire-and-forget sample of the memberlist used for peer discovery and liveness propagation.
    /// Appended after the original three variants so older nodes simply fail to decode (and drop) it
    /// without affecting their probe-state gossip, which keeps the wire format backward-compatible.
    MemberGossip(MessageMetadata<Peer>, MembershipSample<Peer>),
}

impl<Peer, Value> Message<Peer, Value>
where
    Peer: Eq + Hash,
    Value: Versioned,
    Value::Diff: Serialize + Debug + Clone,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    pub fn kind(&self) -> &'static str {
        match self {
            Message::Syn(_, _) => "syn",
            Message::Ack(_, _) => "ack",
            Message::SynAck(_, _, _) => "synack",
            Message::MemberGossip(_, _) => "members",
        }
    }

    pub fn metadata(&self) -> &MessageMetadata<Peer> {
        match self {
            Message::Syn(meta, _) => meta,
            Message::Ack(meta, _) => meta,
            Message::SynAck(meta, _, _) => meta,
            Message::MemberGossip(meta, _) => meta,
        }
    }
}

impl<Peer, Value> Message<Peer, Value>
where
    Peer: Eq + Hash + Clone,
    Value: Versioned,
    Value::Diff: Serialize + Debug + Clone,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    /// Number of state entries carried by this message that [`Message::partition`] can drop in
    /// order to shrink it. A `Syn` carries only a digest, so it has nothing to drop and returns 0.
    pub fn len(&self) -> usize {
        match self {
            Message::Syn(_, _) => 0,
            Message::Ack(_, diff) | Message::SynAck(_, _, diff) => diff.len(),
            Message::MemberGossip(_, sample) => sample.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Message::Syn(_, _) => true,
            Message::Ack(_, diff) | Message::SynAck(_, _, diff) => diff.is_empty(),
            Message::MemberGossip(_, sample) => sample.is_empty(),
        }
    }
}

impl<Peer, Value> Message<Peer, Value>
where
    Peer: Eq + Hash + Clone,
    Value: Versioned,
    Value::Diff: Versioned + Serialize + Debug + Clone,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    /// Consumes the message, returning one carrying at most `max_items` of its state entries,
    /// keeping the oldest. Any accompanying digest is preserved unchanged. Size-limited transports
    /// use this to fit a message into their frame; the dropped entries are re-sent on a later round.
    pub fn partition(self, max_items: usize) -> Self {
        match self {
            Message::Syn(meta, digest) => Message::Syn(meta, digest),
            Message::Ack(meta, diff) => Message::Ack(meta, diff.partition(max_items)),
            Message::SynAck(meta, digest, diff) => {
                Message::SynAck(meta, digest, diff.partition(max_items))
            }
            Message::MemberGossip(meta, sample) => {
                Message::MemberGossip(meta, sample.truncate(max_items))
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageMetadata<Peer> {
    pub from: Peer,
    pub traceparent: Option<String>,
    pub baggage: Option<String>,
}

impl<Peer> MessageMetadata<Peer> {
    /// Create new message metadata with the given peer as the sender.
    pub fn new(from: Peer) -> Self {
        Self {
            from,
            traceparent: None,
            baggage: None,
        }
    }

    /// Inject the current trace context into the message metadata.
    pub fn with_trace_context(mut self) -> Self {
        tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
            p.inject_context(&Span::current().context(), &mut self)
        });

        self
    }

    /// Extract the trace context from the message metadata.
    pub fn trace_context(&self) -> tracing_batteries::prelude::opentelemetry::Context {
        tracing_batteries::prelude::opentelemetry::global::get_text_map_propagator(|p| {
            p.extract(self)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClusterStateDiff<Peer, Value>
where
    Peer: Eq + Hash,
    Value: Versioned,
    Value::Diff: Serialize,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    inner: HashMap<Peer, HashMap<String, Value::Diff>>
}

impl<Peer, Value> ClusterStateDiff<Peer, Value>
where
    Peer: Eq + Hash,
    Value: Versioned,
    Value::Diff: Serialize,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    pub fn new() -> Self {
        Self {
            inner: HashMap::new()
        }
    }

    pub fn update(&mut self, node_id: Peer, field: String, value: Value::Diff) {
        self.inner
            .entry(node_id)
            .or_default()
            .insert(field, value);
    }

    #[cfg(test)]
    pub fn with_node(self, node_id: Peer, state: HashMap<String, Value::Diff>) -> Self {
        let mut map = self.inner;
        map.insert(node_id, state);
        Self { inner: map }
    }

    pub fn into_inner(self) -> HashMap<Peer, HashMap<String, Value::Diff>> {
        self.inner
    }

    /// Total number of `(node, field)` entries carried by this diff.
    pub fn len(&self) -> usize {
        self.inner.values().map(|fields| fields.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.values().all(|fields| fields.is_empty())
    }
}

impl<Peer, Value> ClusterStateDiff<Peer, Value>
where
    Peer: Eq + Hash + Clone,
    Value: Versioned,
    Value::Diff: Versioned + Serialize,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    /// Consumes the diff, returning at most `max_items` of the oldest (lowest-version) entries;
    /// newer entries are dropped and re-sent on a later gossip round. Size-limited transports use
    /// this to shrink an over-large message while still making forward progress on the records that
    /// have waited longest to propagate. The retained entries are moved, not cloned.
    pub fn partition(self, max_items: usize) -> Self {
        let mut entries: Vec<(u64, Peer, String, Value::Diff)> = self
            .inner
            .into_iter()
            .flat_map(|(peer, fields)| {
                fields.into_iter().map(move |(field, diff)| {
                    let version = diff.version();
                    (version, peer.clone(), field, diff)
                })
            })
            .collect();
        entries.sort_by_key(|(version, _, _, _)| *version);
        entries.truncate(max_items);

        let mut inner: HashMap<Peer, HashMap<String, Value::Diff>> = HashMap::new();
        for (_, peer, field, diff) in entries {
            inner.entry(peer).or_default().insert(field, diff);
        }
        Self { inner }
    }
}

impl<Peer, Value> From<HashMap<Peer, HashMap<String, Value::Diff>>> for ClusterStateDiff<Peer, Value>
where
    Peer: Eq + Hash,
    Value: Versioned,
    Value::Diff: Serialize,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    fn from(value: HashMap<Peer, HashMap<String, Value::Diff>>) -> Self {
        Self { inner: value }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ClusterStateDigest<P: Eq + Hash>(HashMap<P, u64>);

impl<P: Eq + Hash> ClusterStateDigest<P> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn get_max_version(&self, node_id: &P) -> Option<u64> {
        self.0.get(node_id).copied()
    }

    #[cfg(test)]
    pub fn with_max_version(self, node_id: P, version: u64) -> Self {
        let mut map = self.0;
        map.insert(node_id, version);
        Self(map)
    }

    pub fn update(&mut self, node_id: P, version: u64) {
        self.0
            .entry(node_id)
            .and_modify(|v| {
                if *v < version {
                    *v = version;
                }
            })
            .or_insert(version);
    }
}

impl<P: Eq + Hash> From<HashMap<P, u64>> for ClusterStateDigest<P> {
    fn from(value: HashMap<P, u64>) -> Self {
        Self(value)
    }
}

impl<P: Eq + Hash + Display> Display for ClusterStateDigest<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let entries: Vec<String> = self.0.iter().map(|(k, v)| format!("{}@{}", k, v)).collect();
        write!(f, "[{}]", entries.join(", "))
    }
}

impl<Peer> tracing_batteries::prelude::opentelemetry::propagation::Extractor for MessageMetadata<Peer> {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "traceparent" => self.traceparent.as_deref(),
            "baggage" => self.baggage.as_deref(),
            _ => None,
        }
    }

    fn keys(&self) -> Vec<&str> {
        let mut keys = Vec::new();
        if self.traceparent.is_some() {
            keys.push("traceparent");
        }
        if self.baggage.is_some() {
            keys.push("baggage");
        }
        keys
    }
}


impl<Peer> tracing_batteries::prelude::opentelemetry::propagation::Injector for MessageMetadata<Peer> {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "traceparent" => self.traceparent = Some(value),
            "baggage" => self.baggage = Some(value),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::versioned::LastWriteWinsValue;

    fn diff_with(versions: &[u64]) -> ClusterStateDiff<u32, LastWriteWinsValue<i32>> {
        let mut diff = ClusterStateDiff::new();
        for &v in versions {
            diff.update(1u32, format!("f{v}"), LastWriteWinsValue::new(v as i32).with_version(v));
        }
        diff
    }

    #[test]
    fn diff_len_counts_entries() {
        assert_eq!(diff_with(&[1, 2, 3]).len(), 3);
        assert!(ClusterStateDiff::<u32, LastWriteWinsValue<i32>>::new().is_empty());
    }

    #[test]
    fn partition_retains_lowest_versions() {
        let kept = diff_with(&[5, 1, 4, 2, 3]).partition(2);
        let inner = kept.into_inner();
        let fields = &inner[&1u32];
        assert_eq!(fields.len(), 2);
        assert!(
            fields.contains_key("f1") && fields.contains_key("f2"),
            "the two oldest (lowest-version) entries must be kept"
        );
    }

    #[test]
    fn partition_capped_at_available() {
        assert_eq!(diff_with(&[1, 2]).partition(10).len(), 2);
        assert_eq!(diff_with(&[1, 2]).partition(0).len(), 0);
    }

    #[test]
    fn message_partition_keeps_oldest_and_preserves_digest() {
        let digest = ClusterStateDigest::new().with_max_version(1u32, 9);
        let msg = Message::SynAck(MessageMetadata::new(1u32), digest.clone(), diff_with(&[3, 1, 2]));
        assert_eq!(msg.len(), 3);

        let partitioned = msg.partition(1);
        assert_eq!(partitioned.len(), 1);
        match partitioned {
            Message::SynAck(_, d, diff) => {
                assert_eq!(d, digest, "digest must be preserved when partitioning");
                assert!(diff.into_inner()[&1u32].contains_key("f1"), "oldest entry kept");
            }
            _ => panic!("expected SynAck"),
        }
    }

    #[test]
    fn syn_has_no_partitionable_entries() {
        let msg: Message<u32, LastWriteWinsValue<i32>> =
            Message::Syn(MessageMetadata::new(1u32), ClusterStateDigest::new());
        assert_eq!(msg.len(), 0);
        assert!(msg.is_empty());
        assert_eq!(msg.partition(0).len(), 0);
    }
}