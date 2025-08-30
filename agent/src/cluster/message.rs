use std::{collections::HashMap, fmt::{Debug, Display}, hash::Hash};

use serde::{Deserialize, Serialize};

use crate::cluster::Versioned;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message<Peer, Value>
where
    Peer: Eq + Hash,
    Value: Versioned,
    Value::Diff: Serialize + Debug + Clone,
    for <'dde> Value::Diff: Deserialize<'dde>,
{
    Syn(Peer, ClusterStateDigest<Peer>),
    Ack(Peer, ClusterStateDiff<Peer, Value>),
    SynAck(Peer, ClusterStateDigest<Peer>, ClusterStateDiff<Peer, Value>),
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
