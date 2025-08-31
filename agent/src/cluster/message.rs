use std::{collections::HashMap, fmt::{Debug, Display}, hash::Hash};

use serde::{Deserialize, Serialize};
use tracing::Span;
use tracing_batteries::prelude::OpenTelemetrySpanExt;
use crate::cluster::Versioned;

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
        }
    }

    pub fn metadata(&self) -> &MessageMetadata<Peer> {
        match self {
            Message::Syn(meta, _) => meta,
            Message::Ack(meta, _) => meta,
            Message::SynAck(meta, _, _) => meta,
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