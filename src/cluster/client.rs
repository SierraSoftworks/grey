use std::fmt::Display;

use tracing_batteries::prelude::*;

use super::*;

pub struct GossipClient<S, T>
where
    S: GossipStore,
    T: GossipTransport<Peer = S::Peer, Address = S::Address, State = S::State>,
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
    T: GossipTransport<Peer = S::Peer, Address = S::Address, State = S::State>,
    S::Peer: Display + Clone + Send + 'static,
    S::Address: Display + Clone + Eq + Send + 'static,
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

    async fn gossip(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut peer_addresses = self.store.get_peer_addresses().await.unwrap_or_default();
        peer_addresses.extend(self.seed_peers.iter().cloned());
        peer_addresses.dedup();
        if peer_addresses.is_empty() {
            return Ok(());
        }

        let self_id = self.store.get_self_id().await?;
        let digest = self.store.get_digest().await?;

        for addr in peer_addresses {
            self.transport.send(addr, Message::Syn(self_id.clone(), digest.clone())).await?;
        }

        Ok(())
    }

    async fn receive_loop(&self) {
        loop {
            match self.transport.try_receive().await {
                Ok(Some((addr, msg))) => match self.handle_message(&addr, msg).await {
                    Ok(()) => {}
                    Err(err) => {
                        warn!("Failed to handle gossip message from {addr}: {err:?}");
                    }
                },
                Ok(None) => {
                    // No message received, continue
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(err) => {
                    // Handle error
                    warn!("Malformed gossip message received, ignoring (make sure all Grey instances in the cluster are running the same major version): {err:?}");
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
        }
    }

    async fn handle_message(&self, addr: &S::Address, msg: Message<S::Peer, S::State>) -> Result<(), Box<dyn std::error::Error>> {
        match msg {
            Message::Syn(peer_id, digest) => {
                trace!("Received gossip syn from {peer_id}: {digest}");
                let self_id = self.store.get_self_id().await?;
                let delta = self.store.get_diff(digest).await?;
                let digest = self.store.get_digest().await?;
                self.transport
                    .send(addr.clone(), Message::SynAck(self_id.clone(), digest, delta))
                    .await?;
            }
            Message::SynAck(peer_id, digest, diff) => {
                trace!("Received gossip synack from {peer_id}: {digest}");
                let self_id = self.store.get_self_id().await?;
                let delta = self.store.get_diff(digest).await?;
                self.store.apply_diff(diff, addr.clone()).await?;

                self
                    .transport
                    .send(addr.clone(), Message::Ack(self_id.clone(), delta))
                    .await?;
            }
            Message::Ack(peer_id, delta) => {
                trace!("Received gossip ack from {peer_id}");
                self.store.apply_diff(delta, addr.clone()).await?;
            }
        }

        Ok(())
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

        let (transport1, transport2) = InMemoryGossipTransport::new(node1, node2);
        let store1 = InMemoryGossipStore::new(node1, node1);
        let store2 = InMemoryGossipStore::new(node2, node2);
        store2.set("test", VersionedField::new("value2")).await;

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
                    store1.set("test", VersionedField::new("value1")).await;
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
