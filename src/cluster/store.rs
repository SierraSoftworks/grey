use std::{future::Future, hash::Hash};

use super::*;

#[cfg(test)]
pub use in_memory::InMemoryGossipStore;

pub trait GossipStore {
    type Peer: Eq + Hash;
    type Address;
    type State;

    fn get_self_id(&self) -> impl Future<Output = Result<Self::Peer, Box<dyn std::error::Error>>>;

    fn get_peer_addresses(
        &self,
    ) -> impl Future<Output = Result<Vec<Self::Address>, Box<dyn std::error::Error>>>;

    fn get_digest(
        &self,
    ) -> impl Future<Output = Result<ClusterStateDigest<Self::Peer>, Box<dyn std::error::Error>>>;

    fn get_delta(
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
    use std::{collections::HashMap, fmt::Debug, hash::Hash, sync::Arc};

    use tokio::sync::RwLock;

    #[derive(Clone)]
    pub struct InMemoryGossipStore<P: Eq + Hash, A, T> {
        pub node_id: P,
        state: Arc<RwLock<HashMap<P, NodeState<T, A>>>>,
    }

    impl<P: Eq + Hash, A: Clone, T: Default> InMemoryGossipStore<P, A, T> {
        pub fn new(node_id: P) -> Self {
            Self {
                node_id,
                state: Arc::new(RwLock::new(HashMap::new())),
            }
        }

        pub async fn get_field(&self, node_id: &P, field: &str) -> Option<T>
        where
            T: Clone,
        {
            self.state
                .read()
                .await
                .get(node_id)
                .and_then(|state| state.values.get(field))
                .map(|field| field.value.clone())
        }
    }

    impl<P: Eq + Hash + Clone, A: Clone, T: Default + Clone> InMemoryGossipStore<P, A, T> {
        pub async fn set_field(&self, address: A, field: &str, value: T) {
            self.state
                .write()
                .await
                .entry(self.node_id.clone())
                .or_insert_with(|| NodeState::new(address))
                .set(field, value)
        }
    }

    impl<P: Eq + Hash + Clone, A: Clone, T: Clone> GossipStore for InMemoryGossipStore<P, A, T> {
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
                        Some(state.address.clone())
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

        async fn get_delta(
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
                        if state.max_version > max_version {
                            return Some((
                                node_id.clone(),
                                state.diff(max_version).unwrap_or_else(NodeStateDiff::new),
                            ));
                        } else {
                            return None;
                        }
                    } else {
                        return Some((
                            node_id.clone(),
                            state.diff(0).unwrap_or_else(NodeStateDiff::new),
                        ));
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
                state
                    .entry(node_id)
                    .and_modify(|e| {
                        e.address = address.clone();
                        e.apply(node_diff.clone());
                    })
                    .or_insert_with(|| NodeState::new_from_diff(node_diff, address.clone()));
            }

            Ok(())
        }
    }

    impl<P: Eq + Hash + Debug, A: Debug, T: Debug> InMemoryGossipStore<P, A, T> {
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

    #[derive(Debug, Clone, PartialEq, Default)]
    struct NodeState<T, A> {
        pub max_version: u64,
        pub address: A,
        pub values: HashMap<String, VersionedField<T>>,
    }

    impl<T, A> NodeState<T, A> {
        pub fn new(address: A) -> Self {
            Self {
                max_version: 0,
                address,
                values: HashMap::new(),
            }
        }

        pub fn new_from_diff(diff: NodeStateDiff<T>, address: A) -> Self {
            let values = diff.into_inner();
            let max_version = values.values().map(|f| f.version).max().unwrap_or(0);
            Self {
                max_version,
                values,
                address,
            }
        }
    }

    impl<T: Clone, A> NodeState<T, A> {
        pub fn set(&mut self, key: &str, value: T) {
            self.max_version += 1;
            let version = self.max_version;
            self.values
                .insert(key.to_string(), VersionedField { version, value });
        }

        pub fn apply(&mut self, diff: NodeStateDiff<T>) {
            for (key, field) in diff.into_inner() {
                let field_clone = field.clone();

                self.max_version = self.max_version.max(field.version);
                self.values
                    .entry(key)
                    .and_modify(move |e| {
                        if field.version > e.version {
                            *e = field;
                        }
                    })
                    .or_insert(field_clone);
            }
        }

        pub fn diff(&self, version: u64) -> Option<NodeStateDiff<T>> {
            if self.max_version <= version {
                return None;
            }

            let mut changes = HashMap::new();
            for (key, field) in &self.values {
                if field.version > version {
                    changes.insert(key.clone(), field.clone());
                }
            }

            Some(changes.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_gossip_store() {
        let store = InMemoryGossipStore::new(NodeID::new());

        store.set_field((), "test", 1).await;
        assert_eq!(store.get_field(&store.node_id, "test").await, Some(1));

        assert_eq!(
            store.get_digest().await.unwrap(),
            ClusterStateDigest::new().with_max_version(store.node_id, 1)
        );

        let diff = store
            .get_delta(ClusterStateDigest::new().with_max_version(store.node_id, 0))
            .await
            .unwrap();
        assert_eq!(
            diff,
            ClusterStateDiff::new().with_node(
                store.node_id,
                NodeStateDiff::new().with_field("test", VersionedField::new(1).with_version(1))
            )
        );

        let new_node = NodeID::new();
        let diff = ClusterStateDiff::new().with_node(
            new_node,
            NodeStateDiff::new().with_field("foo", VersionedField::new(42).with_version(1)),
        );
        store
            .apply_diff(diff, ())
            .await
            .expect("diff to be applied");
        assert_eq!(store.get_field(&new_node, "foo").await, Some(42));
    }
}
