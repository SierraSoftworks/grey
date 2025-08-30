use std::{collections::HashMap, fmt::Display, hash::Hash};

use serde::{Deserialize, Serialize};

use crate::cluster::Versioned;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message<P: Eq + Hash, T> {
    Syn(P, ClusterStateDigest<P>),
    Ack(P, ClusterStateDiff<P, T>),
    SynAck(P, ClusterStateDigest<P>, ClusterStateDiff<P, T>),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ClusterStateDiff<Peer: Eq + Hash, Value>(HashMap<Peer, HashMap<String, Value>>);

impl<P: Eq + Hash, T: Versioned> ClusterStateDiff<P, T> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn update(&mut self, node_id: P, field: String, value: T)
    where
        T: Clone,
    {
        self.0
            .entry(node_id)
            .or_default()
            .entry(field)
            .and_modify(|v| {
                v.apply(&value);
            })
            .or_insert(value);
    }

    #[cfg(test)]
    pub fn with_node(self, node_id: P, state: HashMap<String, T>) -> Self {
        let mut map = self.0;
        map.insert(node_id, state);
        Self(map)
    }

    pub fn into_inner(self) -> HashMap<P, HashMap<String, T>> {
        self.0
    }
}

impl<P: Eq + Hash, T: Versioned> From<HashMap<P, HashMap<String, T>>> for ClusterStateDiff<P, T> {
    fn from(value: HashMap<P, HashMap<String, T>>) -> Self {
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
