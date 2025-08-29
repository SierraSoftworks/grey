use std::{collections::HashMap, fmt::Display, hash::Hash};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message<P: Eq + Hash, T> {
    Syn(P, ClusterStateDigest<P>),
    Ack(P, ClusterStateDiff<P, T>),
    SynAck(P, ClusterStateDigest<P>, ClusterStateDiff<P, T>),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ClusterStateDiff<P: Eq + Hash, T>(HashMap<P, NodeStateDiff<T>>);

impl<P: Eq + Hash, T> ClusterStateDiff<P, T> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn update<K: ToString>(&mut self, node_id: P, field: K, value: VersionedField<T>) {
        self.0
            .entry(node_id)
            .or_insert_with(NodeStateDiff::new)
            .insert(field, value);
    }

    #[cfg(test)]
    pub fn with_node(self, node_id: P, state_diff: NodeStateDiff<T>) -> Self {
        let mut map = self.0;
        map.insert(node_id, state_diff);
        Self(map)
    }

    pub fn into_inner(self) -> HashMap<P, NodeStateDiff<T>> {
        self.0
    }
}

impl<P: Eq + Hash, T> From<HashMap<P, NodeStateDiff<T>>> for ClusterStateDiff<P, T> {
    fn from(value: HashMap<P, NodeStateDiff<T>>) -> Self {
        Self(value)
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct NodeStateDiff<T>(HashMap<String, VersionedField<T>>);

impl<T> NodeStateDiff<T> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert<K: ToString>(&mut self, key: K, field: VersionedField<T>) {
        self.0.insert(key.to_string(), field);
    }

    #[cfg(test)]
    pub fn with_field<K: ToString>(self, key: K, field: VersionedField<T>) -> Self {
        let mut map = self.0;
        map.insert(key.to_string(), field);
        Self(map)
    }

    pub fn into_inner(self) -> HashMap<String, VersionedField<T>> {
        self.0
    }
}

impl<T> From<HashMap<String, VersionedField<T>>> for NodeStateDiff<T> {
    fn from(value: HashMap<String, VersionedField<T>>) -> Self {
        Self(value)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct VersionedField<T> {
    pub version: u64,
    pub value: T,
}

impl<T> VersionedField<T> {
    pub fn new(value: T) -> Self {
        Self { version: 1, value }
    }

    pub fn with_version(self, version: u64) -> Self {
        Self { version, ..self }
    }
}

impl<T> From<(u64, T)> for VersionedField<T> {
    fn from(value: (u64, T)) -> Self {
        Self {
            version: value.0,
            value: value.1,
        }
    }
}
