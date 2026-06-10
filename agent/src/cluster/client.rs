use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;
use tracing_batteries::prelude::*;

use super::*;

pub struct GossipClient<S, T>
where
    S: GossipStore,
    T: GossipTransport<S::Id, S::State>,
    T::Address: Eq + Hash,
{
    store: S,
    transport: T,
    /// The in-memory membership registry: discovered peers, per-address link health, and the
    /// failure detector. Shared with the rest of the process (e.g. the API) behind an [`Arc`].
    membership: Arc<Membership<S::Id, T::Address>>,

    seed_peers: Vec<String>,
    /// How frequently the seed peers are re-resolved by the background resolver loop.
    seed_resolve_interval: std::time::Duration,
    /// The most recently resolved seed peer addresses, maintained by the resolver loop so that the
    /// gossip hot path never has to perform DNS resolution itself.
    resolved_seed_peers: tokio::sync::RwLock<Vec<T::Address>>,

    gossip_factor: usize,
    gossip_interval: std::time::Duration,
    /// Number of member records carried in each fire-and-forget membership gossip datagram.
    membership_sample_size: usize,
}

impl<S, T> GossipClient<S, T>
where
    S: GossipStore,
    T: GossipTransport<S::Id, S::State>,
    S::Id: Display + Debug + Clone + Send + 'static,
    T::Address: Display + Debug + Clone + Eq + Hash + FromStr + Send + 'static,
    S::State: Debug,
{
    pub fn new(store: S, transport: T, membership: Arc<Membership<S::Id, T::Address>>) -> Self {
        Self {
            store,
            transport,
            membership,

            gossip_factor: 1,
            gossip_interval: std::time::Duration::from_secs(10),
            membership_sample_size: 16,
            seed_peers: Vec::new(),
            seed_resolve_interval: std::time::Duration::from_secs(60),
            resolved_seed_peers: tokio::sync::RwLock::new(Vec::new()),
        }
    }

    pub fn with_membership_sample_size(self, size: usize) -> Self {
        Self {
            membership_sample_size: size,
            ..self
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

    pub fn with_seed_peers(self, addresses: Vec<String>) -> Self {
        Self {
            seed_peers: addresses,
            ..self
        }
    }

    pub fn with_seed_resolve_interval(self, interval: std::time::Duration) -> Self {
        Self {
            seed_resolve_interval: interval,
            ..self
        }
    }

    pub async fn run(&self) {
        tokio::join!(self.gossip_loop(), self.receive_loop(), self.resolve_loop());
    }

    /// Periodically re-resolves the configured seed peers in the background so that DNS changes are
    /// picked up without forcing the gossip loop to perform (potentially blocking) DNS lookups on
    /// every round. The resolved addresses are cached and read cheaply by [`Self::gossip`].
    async fn resolve_loop(&self) {
        if self.seed_peers.is_empty() {
            return;
        }

        loop {
            self.refresh_seed_peers().await;
            tokio::time::sleep(self.seed_resolve_interval).await;
        }
    }

    /// Resolves every configured seed peer and updates the cached address list. If resolution yields
    /// no addresses at all (for example during a transient DNS outage), the previously resolved
    /// addresses are retained rather than dropping all of our seeds.
    async fn refresh_seed_peers(&self) {
        let mut resolved = Vec::new();
        for seed in self.seed_peers.iter() {
            match self.transport.resolve(seed).await {
                Ok(addresses) if addresses.is_empty() => {
                    warn!(name: "gossip.seed.resolve", { peer.seed = %seed }, "Seed peer '{seed}' did not resolve to any addresses, skipping it.");
                }
                Ok(addresses) => resolved.extend(addresses),
                Err(err) => {
                    warn!(name: "gossip.seed.resolve", { peer.seed = %seed, exception = %err }, "Failed to resolve seed peer '{seed}', skipping it: {err:?}");
                }
            }
        }

        if resolved.is_empty() && !self.seed_peers.is_empty() {
            warn!(name: "gossip.seed.resolve", "Failed to resolve any seed peers, retaining the previously resolved addresses.");
            return;
        }

        *self.resolved_seed_peers.write().await = resolved;
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
        tracing::Span::current().record("node.id", self_id.to_string().as_str());

        let now = Instant::now();

        // Advance our own heartbeat (so peers observe a regular liveness signal) and run the
        // failure-detector / backoff maintenance once per round.
        self.membership.bump_heartbeat();
        self.membership.sweep(now);

        // Build the gossip target set. Prefer healthy peers up to `gossip_factor`, reserve one slot
        // to retry an unhealthy peer that is due (so recovery is detected), and always include the
        // configured seeds — a node forgotten after a long partition can only rejoin via a live seed.
        let candidates = self.membership.gossip_candidates(now);
        let mut healthy = Vec::new();
        let mut unhealthy = Vec::new();
        for candidate in candidates {
            if !candidate.due {
                continue;
            }
            if candidate.liveness == Liveness::Healthy {
                healthy.push((candidate.id, candidate.address));
            } else {
                unhealthy.push((candidate.id, candidate.address));
            }
        }

        let mut targets: Vec<(Option<S::Id>, T::Address)> = Vec::new();
        for (id, addr) in sample_peers(healthy, self.gossip_factor) {
            targets.push((Some(id), addr));
        }
        for (id, addr) in sample_peers(unhealthy, 1) {
            targets.push((Some(id), addr));
        }
        // The resolved seed addresses are maintained by the background resolver loop so we never
        // block the gossip hot path on DNS resolution here. Seeds have no known NodeID yet.
        for addr in self.resolved_seed_peers.read().await.iter().cloned() {
            targets.push((None, addr));
        }

        let targets = unique_by_address(targets);
        if targets.is_empty() {
            return Ok(());
        }

        let digest = self.store.digest().await?;
        let sample = self.membership.sample_for_gossip(self.membership_sample_size, now);

        for (maybe_id, addr) in targets {
            if let Some(id) = &maybe_id {
                self.membership.record_send(id, &addr, now);
            }

            let span = info_span!("gossip.peer", otel.kind = "client", node.id = %self_id, peer.addr=%addr);
            let syn_meta = span.in_scope(|| MessageMetadata::new(self_id.clone()).with_trace_context());

            // Probe-state anti-entropy (the established Syn/SynAck/Ack handshake).
            self.transport
                .send(addr.clone(), Message::Syn(syn_meta, digest.clone()))
                .instrument(span.clone())
                .await?;

            // Fire-and-forget membership dissemination. A failure here must not abort the probe
            // gossip round (an old peer, for example, simply drops the unknown message), so errors
            // are logged and swallowed rather than propagated.
            if !sample.is_empty() {
                let member_meta =
                    span.in_scope(|| MessageMetadata::new(self_id.clone()).with_trace_context());
                if let Err(err) = self
                    .transport
                    .send(addr.clone(), Message::MemberGossip(member_meta, sample.clone()))
                    .instrument(span)
                    .await
                {
                    trace!("Failed to send membership gossip to {addr}: {err:?}");
                }
            }
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
                    // No message available (e.g. a closed in-memory channel); the UDP transport
                    // now awaits the next datagram, so this no longer busy-polls.
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
        addr: &T::Address,
        msg: Message<S::Id, S::State>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let now = Instant::now();
        // Every inbound datagram proves this source address works for the sender — record it so the
        // working-address set (and therefore discovery and link health) grows from observed traffic.
        let from = msg.metadata().from.clone();
        self.membership.record_inbound(&from, addr.clone(), now);

        let result = {
            match msg {
                Message::Syn(meta, digest) => {
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
                    // A SynAck is a reply to a Syn we sent, so it confirms our messages are reaching
                    // this peer (the signal that distinguishes a healthy link from a one-way one).
                    self.membership.record_confirmation(&from, now);
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
                    // An Ack confirms our SynAck reached the peer — our messages get through.
                    self.membership.record_confirmation(&from, now);
                    self.store.apply(delta).await
                        .map_err(|e| format!("Failed to apply delta from peer {}: {e:?}", meta.from))?;
                }
                Message::MemberGossip(_meta, sample) => {
                    // Fire-and-forget membership dissemination: merge the advertised peers/addresses
                    // and feed observed heartbeat advances to the failure detector.
                    self.membership.merge_sample(sample, now);
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
    use std::collections::HashSet;
    use std::net::SocketAddr;
    use std::time::Duration;

    use super::*;

    fn test_membership_config() -> MembershipConfig {
        // Generous windows so nothing expires or backs off during a short test; the detector never
        // accrues samples here (membership self-advertisement is off), so peers stay Healthy.
        MembershipConfig {
            failure_detector_window: 100,
            phi_prior: Duration::from_millis(50),
            phi_threshold: 8.0,
            dead_grace: Duration::from_secs(60),
            max_addresses: 8,
            working_window: Duration::from_secs(60),
            backoff_base: Duration::from_millis(10),
            backoff_max: Duration::from_secs(1),
            member_expiry: Duration::from_secs(300),
        }
    }

    fn test_membership(id: NodeID) -> Arc<Membership<NodeID, NodeID>> {
        Arc::new(Membership::new(id, 1, Vec::new(), test_membership_config()))
    }

    #[tokio::test]
    async fn test_client_gossip() {
        let node1 = NodeID::new();
        let node2 = NodeID::new();

        let (transport1, transport2) = InMemoryGossipTransport::<_, LastWriteWinsValue<String>>::new(node1, node2);
        let store1 = InMemoryGossipStore::<_, _, LastWriteWinsValue<String>>::new(node1, node1);
        let store2 = InMemoryGossipStore::<_, _, LastWriteWinsValue<String>>::new(node2, node2);
        store2.update("test", LastWriteWinsValue::new("value2".to_string())).await;

        let client1 = GossipClient::new(store1.clone(), transport1, test_membership(node1))
            .with_gossip_interval(Duration::from_millis(10));
        let client2 = GossipClient::new(store2.clone(), transport2, test_membership(node2))
            .with_gossip_interval(Duration::from_millis(10))
            .with_seed_peers(vec![node1.to_string()]);

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

    // ---- Multi-node mock network (for discovery and unidirectional-link tests) -------------------

    type MockMsg = Message<NodeID, LastWriteWinsValue<String>>;

    /// A shared in-memory network of nodes addressed by [`SocketAddr`], supporting directional link
    /// failures so we can simulate partitions and one-way links.
    struct MockNet {
        inboxes: std::sync::Mutex<
            std::collections::HashMap<SocketAddr, tokio::sync::mpsc::UnboundedSender<(SocketAddr, MockMsg)>>,
        >,
        blocked: std::sync::Mutex<HashSet<(SocketAddr, SocketAddr)>>,
    }

    impl MockNet {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                inboxes: std::sync::Mutex::new(std::collections::HashMap::new()),
                blocked: std::sync::Mutex::new(HashSet::new()),
            })
        }

        fn node(self: &Arc<Self>, addr: SocketAddr) -> MockTransport {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            self.inboxes.lock().unwrap().insert(addr, tx);
            MockTransport {
                addr,
                net: self.clone(),
                rx: tokio::sync::Mutex::new(rx),
            }
        }

        /// Drops all datagrams sent from `from` to `to` (one direction only).
        fn block(&self, from: SocketAddr, to: SocketAddr) {
            self.blocked.lock().unwrap().insert((from, to));
        }
    }

    struct MockTransport {
        addr: SocketAddr,
        net: Arc<MockNet>,
        rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<(SocketAddr, MockMsg)>>,
    }

    impl GossipTransport<NodeID, LastWriteWinsValue<String>> for MockTransport {
        type Address = SocketAddr;

        async fn resolve(&self, address: &str) -> Result<Vec<SocketAddr>, Box<dyn std::error::Error>> {
            Ok(vec![address.parse()?])
        }

        async fn send(&self, address: SocketAddr, msg: MockMsg) -> Result<(), Box<dyn std::error::Error>> {
            if self.net.blocked.lock().unwrap().contains(&(self.addr, address)) {
                return Ok(()); // the datagram is silently dropped by the simulated link failure
            }
            let tx = self.net.inboxes.lock().unwrap().get(&address).cloned();
            if let Some(tx) = tx {
                let _ = tx.send((self.addr, msg));
            }
            Ok(())
        }

        async fn try_receive(&self) -> Result<Option<(SocketAddr, MockMsg)>, Box<dyn std::error::Error>> {
            Ok(self.rx.lock().await.recv().await)
        }
    }

    fn socket_membership(id: NodeID, addr: SocketAddr) -> Arc<Membership<NodeID, SocketAddr>> {
        Arc::new(Membership::new(
            id,
            1,
            vec![addr.to_string()],
            test_membership_config(),
        ))
    }

    fn mock_client(
        net: &Arc<MockNet>,
        id: NodeID,
        addr: SocketAddr,
        seeds: Vec<SocketAddr>,
        membership: Arc<Membership<NodeID, SocketAddr>>,
    ) -> GossipClient<InMemoryGossipStore<NodeID, SocketAddr, LastWriteWinsValue<String>>, MockTransport> {
        let store = InMemoryGossipStore::<_, _, LastWriteWinsValue<String>>::new(id, addr);
        GossipClient::new(store, net.node(addr), membership)
            .with_gossip_interval(Duration::from_millis(10))
            .with_gossip_factor(3)
            .with_seed_peers(seeds.into_iter().map(|a| a.to_string()).collect())
    }

    /// A and C are each seeded only to B (they have no direct knowledge of one another). Membership
    /// gossip relayed through B must let them discover each other's address and gossip directly —
    /// i.e. discovery works without full-mesh seeding (#627).
    #[tokio::test]
    async fn discovers_non_seed_peers_via_membership_gossip() {
        let a: SocketAddr = "127.0.0.1:21001".parse().unwrap();
        let b: SocketAddr = "127.0.0.1:21002".parse().unwrap();
        let c: SocketAddr = "127.0.0.1:21003".parse().unwrap();
        let (na, nb, nc) = (NodeID::new(), NodeID::new(), NodeID::new());

        let net = MockNet::new();
        let (ma, mb, mc) = (
            socket_membership(na, a),
            socket_membership(nb, b),
            socket_membership(nc, c),
        );

        let ca = mock_client(&net, na, a, vec![b], ma.clone());
        let cb = mock_client(&net, nb, b, vec![], mb.clone());
        let cc = mock_client(&net, nc, c, vec![b], mc.clone());

        {
            let local = tokio::task::LocalSet::new();
            local.spawn_local(async move { ca.run().await });
            local.spawn_local(async move { cb.run().await });
            local.spawn_local(async move { cc.run().await });
            local
                .run_until(tokio::time::sleep(Duration::from_millis(400)))
                .await;
        }

        assert!(
            ma.known_addresses(&nc).contains(&c),
            "A should have discovered C's address transitively via B"
        );
        assert!(
            mc.known_addresses(&na).contains(&a),
            "C should have discovered A's address transitively via B"
        );
    }

    /// B can reach A but A cannot reach B (a one-way link). A keeps receiving B's gossip (so B is
    /// observably online) yet none of A's messages to B are answered, which A must classify as
    /// unreachable (#615) rather than a healthy peer.
    #[tokio::test]
    async fn detects_unreachable_peer() {
        let a: SocketAddr = "127.0.0.1:22001".parse().unwrap();
        let b: SocketAddr = "127.0.0.1:22002".parse().unwrap();
        let (na, nb) = (NodeID::new(), NodeID::new());

        let net = MockNet::new();
        net.block(a, b); // A's datagrams to B are dropped; B's to A still arrive.

        let (ma, mb) = (socket_membership(na, a), socket_membership(nb, b));
        let ca = mock_client(&net, na, a, vec![b], ma.clone());
        let cb = mock_client(&net, nb, b, vec![a], mb.clone());

        {
            let local = tokio::task::LocalSet::new();
            local.spawn_local(async move { ca.run().await });
            local.spawn_local(async move { cb.run().await });
            local
                .run_until(tokio::time::sleep(Duration::from_millis(400)))
                .await;
        }

        assert_eq!(
            ma.liveness_of(&nb, Instant::now()),
            Some(Liveness::Unreachable),
            "A should detect that B is online but not responding to its messages"
        );
    }
}
