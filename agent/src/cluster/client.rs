use std::fmt::{Debug, Display};
use tracing::instrument;
use tracing_batteries::prelude::*;

use super::*;

pub struct GossipClient<S, T>
where
    S: GossipStore,
    T: GossipTransport<S::Id, S::State, Address = S::Address>,
{
    store: S,
    transport: T,

    seed_peers: Vec<S::Address>,

    gossip_factor: usize,
    gossip_interval: std::time::Duration,
}

impl<S, T> GossipClient<S, T>
where
    S: GossipStore,
    T: GossipTransport<S::Id, S::State, Address = S::Address>,
    S::Id: Display + Debug + Clone + Send + 'static,
    S::Address: Display + Debug + Clone + Eq + Send + 'static,
    S::State: Debug,
{
    pub fn new(store: S, transport: T) -> Self {
        Self {
            store,
            transport,

            gossip_factor: 1,
            gossip_interval: std::time::Duration::from_secs(10),
            seed_peers: Vec::new(),
        }
    }

    pub fn with_gossip_interval(self, interval: std::time::Duration) -> Self {
        Self {
            gossip_interval: interval,
            ..self
        }
    }

    pub fn with_gossip_factor(self, factor: usize) -> Self {
        Self {
            gossip_factor: factor,
            ..self
        }
    }

    pub fn with_seed_peers(self, addresses: Vec<S::Address>) -> Self {
        Self {
            store: self.store,
            transport: self.transport,
            gossip_factor: self.gossip_factor,
            gossip_interval: self.gossip_interval,
            seed_peers: addresses,
        }
    }

    pub async fn run(&self) {
        tokio::join!(self.gossip_loop(), self.receive_loop());
    }

    async fn gossip_loop(&self) {
        let start_delay = rand::random::<u128>() % self.gossip_interval.as_millis();
        tokio::time::sleep(std::time::Duration::from_millis(start_delay as u64)).await;

        loop {
            if let Err(err) = self.gossip().await {
                warn!("Failed to send gossip messages: {err:?}");
            }

            tokio::time::sleep(self.gossip_interval).await;
        }
    }

    #[instrument(skip(self), fields(otel.kind = "producer", node.id = EmptyField))]
    async fn gossip(&self) -> Result<(), Box<dyn std::error::Error>> {
        let self_id = self.store.id().await?;
        tracing::Span::current().record("node.id", &self_id.to_string().as_str());

        let mut peer_addresses = self.store.get_peer_addresses().await.unwrap_or_default();
        peer_addresses.extend(self.seed_peers.iter().cloned());
        peer_addresses.dedup();
        if peer_addresses.is_empty() {
            return Ok(());
        }

        let digest = self.store.digest().await?;

        for addr in peer_addresses {
            let span = info_span!("gossip.peer", otel.kind = "client", node.id = %self_id, peer.addr=%addr);
            let meta = span.in_scope(|| {
                MessageMetadata::new(self_id.clone()).with_trace_context()
            });

            self.transport
                .send(addr, Message::Syn(meta, digest.clone()))
                .instrument(span)
                .await?;
        }

        Ok(())
    }

    async fn receive_loop(&self) {
        let self_id = match self.store.id().await {
            Ok(id) => id,
            Err(err) => {
                error!("Failed to get own node ID from store, clustering is disabled: {err:?}");
                return;
            }
        };

        loop {
            match self.transport.try_receive().await {
                Ok(Some((addr, msg))) => {
                    let meta = msg.metadata();
                    let span = info_span!(
                        "gossip.receive",
                        otel.kind = "server",
                        otel.name=format!("gossip.{}", msg.kind()),
                        node.id=%self_id,
                        peer.id=%meta.from,
                        peer.addr=%addr
                    );
                    span.set_parent(meta.trace_context());

                    trace!(name: "gossip.receive", "Received gossip {} message from {}: {:?}", msg.kind(), addr, msg);

                    match self.handle_message(self_id.clone(), &addr, msg).instrument(span).await {
                        Ok(()) => {}
                        Err(err) => {
                            warn!(name: "gossip.handle", "Failed to handle gossip message from {addr}: {err:?}");
                        }
                    }
                },
                Ok(_) => {
                    // No message received, continue
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(err) => {
                    // Handle error
                    warn!(
                        "Malformed gossip message received, ignoring (make sure all Grey instances in the cluster are running the same major version): {err:?}"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
        }
    }

    async fn handle_message(
        &self,
        self_id: S::Id,
        addr: &S::Address,
        msg: Message<S::Id, S::State>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let result = {
            match msg {
                Message::Syn(meta, digest) => {
                    self.store.heartbeat(meta.from.clone(), addr.clone()).await
                        .map_err(|e| format!("Failed to store peer heartbeat: {e:?}"))?;
                    let delta = self.store.diff(digest).await
                        .map_err(|e| format!("Failed to compute diff for peer {}: {e:?}", meta.from))?;
                    let digest = self.store.digest().await
                        .map_err(|e| format!("Failed to compute digest for node: {e:?}"))?;
                    self.transport
                        .send(
                            addr.clone(),
                            Message::SynAck(MessageMetadata::new(self_id.clone()).with_trace_context(), digest, delta),
                        )
                        .await
                        .map_err(|e| format!("Failed to send synack gossip message to peer {} at {addr}: {e:?}", meta.from))?;
                    trace!("Sent synack to {} at {}", meta.from, addr);
                }
                Message::SynAck(meta, digest, diff) => {
                    self.store.heartbeat(meta.from.clone(), addr.clone()).await
                        .map_err(|e| format!("Failed to store peer heartbeat: {e:?}"))?;
                    let delta = self.store.diff(digest).await
                        .map_err(|e| format!("Failed to compute diff for peer {}: {e:?}", meta.from))?;
                    self.store.apply(diff).await?;

                    self.transport
                        .send(addr.clone(), Message::Ack(MessageMetadata::new(self_id.clone()).with_trace_context(), delta))
                        .await
                        .map_err(|e| format!("Failed to send ack gossip message to peer {} at {addr}: {e:?}", meta.from))?;

                    trace!("Sent ack to {} at {}", meta.from, addr);
                }
                Message::Ack(meta, delta) => {
                    self.store.heartbeat(meta.from.clone(), addr.clone()).await
                        .map_err(|e| format!("Failed to store peer heartbeat: {e:?}"))?;
                    self.store.apply(delta).await
                        .map_err(|e| format!("Failed to apply delta from peer {}: {e:?}", meta.from))?;
                }
            }

            Ok(())
        };

        match result {
            Ok(_) => {
                trace!("Successfully handled gossip message from {addr}");
                Ok(())
            }
            Err(err) => {
                trace!("Failed to handle gossip message from {addr}: {err:?}");
                Span::current().record("error", &err);
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn test_client_gossip() {
        let node1 = NodeID::new();
        let node2 = NodeID::new();

        let (transport1, transport2) = InMemoryGossipTransport::<_, LastWriteWinsValue<String>>::new(node1, node2);
        let store1 = InMemoryGossipStore::<_, _, LastWriteWinsValue<String>>::new(node1, node1);
        let store2 = InMemoryGossipStore::<_, _, LastWriteWinsValue<String>>::new(node2, node2);
        store2.update("test", LastWriteWinsValue::new("value2".to_string())).await;

        let client1 = GossipClient::new(store1.clone(), transport1)
            .with_gossip_interval(Duration::from_millis(10));
        let client2 = GossipClient::new(store2.clone(), transport2)
            .with_gossip_interval(Duration::from_millis(10))
            .with_seed_peers(vec![node1]);

        {
            let local_set = tokio::task::LocalSet::new();
            local_set.spawn_local(async move { client1.run().await });
            local_set.spawn_local(async move { client2.run().await });

            local_set
                .run_until(async {
                    store1.update("test", LastWriteWinsValue::new("value1".to_string())).await;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                })
                .await;
        }

        println!("Store 1");
        store1.print_debug().await;

        println!("Store 2");
        store2.print_debug().await;

        assert_eq!(store1.get(&node2, "test").await.unwrap().value, "value2");
        assert_eq!(store2.get(&node1, "test").await.unwrap().value, "value1");
    }
}
