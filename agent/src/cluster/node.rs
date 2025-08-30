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

// #[derive(Clone, Debug, Serialize, Deserialize)]
// pub struct AddressableNodeID<A> {
//     id: NodeID,
//     address: A,
// }

// impl<A> AddressableNodeID<A> {
//     pub fn new(id: NodeID, address: A) -> Self {
//         Self { id, address }
//     }

//     pub fn id(&self) -> NodeID {
//         self.id
//     }

//     pub fn address(&self) -> &A {
//         &self.address
//     }
// }

// impl<A: Display> Display for AddressableNodeID<A> {
//     fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
//         write!(f, "{}@{}", self.id, self.address)
//     }
// }

// impl<A> AsRef<A> for AddressableNodeID<A> {
//     fn as_ref(&self) -> &A {
//         &self.address
//     }
// }

// impl<A: Hash> Hash for AddressableNodeID<A> {
//     fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
//         self.id.hash(state);
//     }
// }

// impl<A> Eq for AddressableNodeID<A> {}

// impl<A> PartialEq<AddressableNodeID<A>> for AddressableNodeID<A> {
//     fn eq(&self, other: &Self) -> bool {
//         self.id == other.id
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_display() {
        let id = NodeID(12345);
        assert_eq!(format!("{}", id), "9ix");
    }
}
