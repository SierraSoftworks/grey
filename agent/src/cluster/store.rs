use std::{future::Future, hash::Hash};

use super::*;

#[cfg(test)]
pub use in_memory::InMemoryGossipStore;

pub trait GossipStore {
    type Peer: Eq + Hash;
    type Address;
    type State: Versioned;

    fn get_self_id(&self) -> impl Future<Output = Result<Self::Peer, Box<dyn std::error::Error>>>;

    fn get_peer_addresses(
        &self,
    ) -> impl Future<Output = Result<Vec<Self::Address>, Box<dyn std::error::Error>>>;

    fn get_digest(
        &self,
    ) -> impl Future<Output = Result<ClusterStateDigest<Self::Peer>, Box<dyn std::error::Error>>>;

    fn get_diff(
        &self,
        digest: ClusterStateDigest<Self::Peer>,
    ) -> impl Future<
        Output = Result<ClusterStateDiff<Self::Peer, Self::State>, Box<dyn std::error::Error>>,
    >;

    fn apply_diff(
        &self,
        diff: ClusterStateDiff<Self::Peer, Self::State>,
        address: Self::Address,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>>;
}

#[cfg(test)]
mod in_memory {
    use super::*;
    use serde::{de::DeserializeOwned, Serialize};
    use std::{collections::HashMap, fmt::Debug, hash::Hash, sync::Arc};
    use tokio::sync::RwLock;

    #[derive(Clone)]
    pub struct InMemoryGossipStore<P: Eq + Hash, A, T: Versioned> {
        node_id: P,
        address: A,
        state: Arc<RwLock<HashMap<P, NodeState<T, A>>>>,
    }

    impl<P: Eq + Hash + Clone, A: Clone + Eq + Debug + Serialize + DeserializeOwned, T: Clone + Versioned> InMemoryGossipStore<P, A, T> {
        pub fn new(node_id: P, address: A) -> Self {
            Self {
                node_id,
                address,
                state: Arc::new(RwLock::new(HashMap::new())),
            }
        }

        pub async fn get(&self, node_id: &P, field: &str) -> Option<T>
        where
            T: Clone,
        {
            self.state
                .read()
                .await
                .get(node_id)
                .and_then(|node| node.fields.get(field).cloned())
        }

        pub async fn update<S: ToString>(&self, field: S, value: T::Diff) {
            let field = field.to_string();

            let mut state = self.state.write().await;

            let node_state = state
                .entry(self.node_id.clone())
                .or_insert_with(|| NodeState::new(self.address.clone()));

            node_state
                .fields
                .entry(field)
                .and_modify(|f| f.apply(&value))
                .or_insert(value.into());

            node_state.max_version = node_state
                .fields
                .values()
                .map(|v| v.version())
                .max()
                .unwrap_or_default()
                .max(node_state.address.version());
        }
    }

    impl<P: Eq + Hash + Clone, A: Clone + PartialEq + Debug + Serialize + DeserializeOwned, T: Clone + Versioned> GossipStore
        for InMemoryGossipStore<P, A, T>
    {
        type Peer = P;
        type Address = A;
        type State = T;

        async fn get_self_id(&self) -> Result<Self::Peer, Box<dyn std::error::Error>> {
            Ok(self.node_id.clone())
        }

        async fn get_peer_addresses(
            &self,
        ) -> Result<Vec<Self::Address>, Box<dyn std::error::Error>> {
            Ok(self
                .state
                .read()
                .await
                .iter()
                .filter_map(|(id, state)| {
                    if *id != self.node_id {
                        Some(state.address.value.clone())
                    } else {
                        None
                    }
                })
                .collect())
        }

        async fn get_digest(
            &self,
        ) -> Result<ClusterStateDigest<Self::Peer>, Box<dyn std::error::Error>> {
            Ok(self
                .state
                .read()
                .await
                .iter()
                .map(|(node_id, state)| (node_id.clone(), state.max_version))
                .collect::<HashMap<_, _>>()
                .into())
        }

        async fn get_diff(
            &self,
            digest: ClusterStateDigest<Self::Peer>,
        ) -> Result<ClusterStateDiff<Self::Peer, Self::State>, Box<dyn std::error::Error>> {
            Ok(self
                .state
                .read()
                .await
                .iter()
                .filter_map(|(node_id, state)| {
                    if let Some(max_version) = digest.get_max_version(node_id) {
                        let diff = state
                            .fields
                            .iter()
                            .filter_map(|(key, value)| value.diff(max_version).map(|diff| (key.clone(), diff)))
                            .collect();

                        Some((node_id.clone(), diff))
                    } else {
                        Some((node_id.clone(), state.fields.iter()
                            .filter_map(|(k, v)| v.diff(0).map(|d| (k.clone(), d)))
                            .collect()))
                    }
                })
                .collect::<HashMap<_, _>>()
                .into())
        }

        async fn apply_diff(
            &self,
            diff: ClusterStateDiff<Self::Peer, Self::State>,
            address: Self::Address,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let mut state = self.state.write().await;
            for (node_id, node_diff) in diff.into_inner() {
                let node_state = state
                    .entry(node_id)
                    .or_insert_with(|| NodeState::new(address.clone()));

                if node_state.address.value != address {
                    node_state.address = VersionedField::new(address.clone())
                        .with_version(node_state.max_version + 1);
                }

                for (field, value) in node_diff {
                    node_state
                        .fields
                        .entry(field)
                        .and_modify(|f| {
                            f.apply(&value);
                        })
                        .or_insert(value.into());
                }

                node_state.max_version = node_state
                    .fields
                    .values()
                    .map(|v| v.version())
                    .max()
                    .unwrap_or_default()
                    .max(node_state.address.version());
            }

            Ok(())
        }
    }

    impl<P: Eq + Hash + Debug, A: Debug, T: Debug + Versioned> InMemoryGossipStore<P, A, T> {
        pub async fn print_debug(&self) {
            let state = self.state.read().await;
            println!("InMemoryGossipStore {{");
            println!("  node_id: {:?}", self.node_id);
            println!("  state: {{");
            for (peer_id, node_state) in &*state {
                println!("    {:?}: {:?}", peer_id, node_state);
            }
            println!("  }}");
            println!("}}");
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct NodeState<T, A> {
        pub address: VersionedField<A>,
        pub fields: HashMap<String, T>,
        pub max_version: u64,
    }

    impl<T, A> NodeState<T, A> {
        pub fn new(address: A) -> Self {
            Self {
                address: VersionedField::new(address),
                fields: HashMap::new(),
                max_version: 0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_gossip_store() {
        let node_id = NodeID::new();
        let store = InMemoryGossipStore::<_, _, VersionedField<i32>>::new(node_id, node_id);

        store
            .update("test", VersionedField::new(1).with_version(1))
            .await;
        assert_eq!(store.get(&node_id, "test").await.unwrap().value, 1);

        assert_eq!(
            store.get_digest().await.unwrap(),
            ClusterStateDigest::new().with_max_version(node_id, 1)
        );

        let diff = store
            .get_diff(ClusterStateDigest::new().with_max_version(node_id, 0))
            .await
            .unwrap();
        assert_eq!(
            diff,
            ClusterStateDiff::new().with_node(
                node_id,
                vec![("test".into(), VersionedField::new(1).with_version(1))]
                    .into_iter()
                    .collect()
            )
        );

        let new_node = NodeID::new();
        let diff = ClusterStateDiff::new().with_node(
            new_node,
            vec![("test".into(), VersionedField::new(42).with_version(1))]
                .into_iter()
                .collect(),
        );
        store
            .apply_diff(diff, new_node)
            .await
            .expect("diff to be applied");
        assert_eq!(store.get(&new_node, "test").await.unwrap().value, 42);
    }
}
