use std::{fmt::Display, hash::Hash};

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeID(u128);

impl NodeID {
    pub fn new() -> Self {
        Self(rand::random::<u128>())
    }
}

impl Display for NodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", radix_fmt::radix_36(self.0))
    }
}

impl From<u128> for NodeID {
    fn from(value: u128) -> Self {
        Self(value)
    }
}

impl Into<u128> for NodeID {
    fn into(self) -> u128 {
        self.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AliasedNodeID(String);

impl AliasedNodeID {
    pub fn new(alias: String) -> Self {
        Self(alias)
    }

    pub fn new_with_random_suffix(alias: String) -> Self {
        Self(format!("{}/{}", alias, rand::random::<u32>()))
    }
}

impl Display for AliasedNodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for AliasedNodeID {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for AliasedNodeID {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_display() {
        let id = NodeID(12345);
        assert_eq!(format!("{}", id), "9ix");
    }
}
