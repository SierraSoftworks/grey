use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;
use tracing_batteries::prelude::*;

use super::*;
use crate::telemetry::{GossipDirection, metrics};

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
            seed_peers: Vec::new(),
            seed_resolve_interval: std::time::Duration::from_secs(60),
            resolved_seed_peers: tokio::sync::RwLock::new(Vec::new()),
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
        info!(
            name: "gossip.start",
            {
                gossip.interval = ?self.gossip_interval,
                gossip.factor = self.gossip_factor,
                gossip.seeds = ?self.seed_peers,
            },
            "Starting cluster gossip with {} seed peer(s).",
            self.seed_peers.len(),
        );
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

        // Seed addresses are exempt from address-set bounding in the membership registry, so keep
        // its view of them in sync with what we resolved.
        self.membership.set_seed_addresses(resolved.iter().cloned());
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

        // The resolved seed addresses are maintained by the background resolver loop so we never
        // block the gossip hot path on DNS resolution here.
        let seed_addresses = self.resolved_seed_peers.read().await.clone();
        let targets = self.build_targets(now, &seed_addresses);
        if targets.is_empty() {
            return Ok(());
        }

        // The full (shuffled) memberlist; the transport fits it to its envelope, truncating the
        // excess to ride a later round.
        let digest = self.store.digest().await?;
        let sample = self.membership.sample_for_gossip(now);

        for (maybe_id, addr) in targets {
            if let Some(id) = &maybe_id {
                self.membership.record_send(id, &addr, now);
            }

            let span = info_span!("gossip.peer", otel.kind = "client", node.id = %self_id, peer.addr=%addr);
            let syn_meta = span.in_scope(|| MessageMetadata::new(self_id.clone()).with_trace_context());

            // Probe-state anti-entropy (the established Syn/SynAck/Ack handshake). Best effort per
            // target: a failure to reach one peer must not prevent the remaining targets (including
            // the seeds) from being gossiped this round.
            let sent = self
                .transport
                .send(addr.clone(), Message::Syn(syn_meta, digest.clone()))
                .instrument(span.clone())
                .await;
            span.in_scope(|| {
                metrics().record_gossip(
                    "syn",
                    GossipDirection::Sent,
                    &self_id,
                    maybe_id.as_ref().map(|id| id as &dyn Display),
                    sent.is_ok(),
                )
            });
            if let Err(err) = sent {
                warn!(name: "gossip.send", { peer.addr = %addr, exception = %err }, "Failed to send gossip syn to {addr}: {err:?}");
                continue;
            }

            // Fire-and-forget membership dissemination. A failure here must not abort the probe
            // gossip round (an old peer, for example, simply drops the unknown message), so errors
            // are logged and swallowed rather than propagated.
            if !sample.is_empty() {
                let member_meta =
                    span.in_scope(|| MessageMetadata::new(self_id.clone()).with_trace_context());
                let sent = self
                    .transport
                    .send(addr.clone(), Message::MemberGossip(member_meta, sample.clone()))
                    .instrument(span.clone())
                    .await;
                span.in_scope(|| {
                    metrics().record_gossip(
                        "members",
                        GossipDirection::Sent,
                        &self_id,
                        maybe_id.as_ref().map(|id| id as &dyn Display),
                        sent.is_ok(),
                    )
                });
                if let Err(err) = sent {
                    trace!("Failed to send membership gossip to {addr}: {err:?}");
                }
            }
        }

        Ok(())
    }

    /// Builds this round's gossip target set: healthy peers up to `gossip_factor`, one slot to
    /// retry an unhealthy peer that is due (so recovery is detected), and always the configured
    /// seeds — a node forgotten after a long partition can only rejoin via a live seed.
    ///
    /// At most one address is selected per peer. [`Membership::gossip_candidates`] chooses the
    /// single address for every discovered peer (preferring a configured seed address unless it is
    /// failing its retry backoff while another address is eligible), and a seed address that is
    /// known to belong to an already-targeted member is skipped rather than producing a second Syn
    /// to the same peer in the same round.
    fn build_targets(
        &self,
        now: Instant,
        seed_addresses: &[T::Address],
    ) -> Vec<(Option<S::Id>, T::Address)> {
        // The single address chosen for each known peer this round: any send to that peer —
        // including one triggered by a seed address it owns — uses this address.
        let mut chosen: HashMap<S::Id, T::Address> = HashMap::new();
        let mut healthy = Vec::new();
        let mut unhealthy = Vec::new();
        for candidate in self.membership.gossip_candidates(now) {
            chosen.insert(candidate.id.clone(), candidate.address.clone());
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
        let mut targeted: HashSet<S::Id> = HashSet::new();
        for (id, addr) in sample_peers(healthy, self.gossip_factor) {
            targeted.insert(id.clone());
            targets.push((Some(id), addr));
        }
        for (id, addr) in sample_peers(unhealthy, 1) {
            targeted.insert(id.clone());
            targets.push((Some(id), addr));
        }

        for addr in seed_addresses {
            match self.membership.owner_of(addr) {
                Some(owner) => {
                    // The seed belongs to a discovered member: contact it once, at the member's
                    // chosen address, and attribute the send so the address's link health (and
                    // retry backoff) is tracked.
                    if !targeted.insert(owner.clone()) {
                        continue;
                    }
                    let addr = chosen.get(&owner).cloned().unwrap_or_else(|| addr.clone());
                    targets.push((Some(owner), addr));
                }
                None => targets.push((None, addr.clone())),
            }
        }

        unique_by_address(targets)
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
                    
                    let _ = span.set_parent(meta.trace_context());

                    trace!(name: "gossip.receive", "Received gossip {} message from {}: {:?}", msg.kind(), addr, msg);

                    let kind = msg.kind();
                    let from = meta.from.clone();
                    let result = self.handle_message(self_id.clone(), &addr, msg).instrument(span.clone()).await;
                    span.in_scope(|| {
                        metrics().record_gossip(
                            kind,
                            GossipDirection::Received,
                            &self_id,
                            Some(&from),
                            result.is_ok(),
                        )
                    });
                    if let Err(err) = result {
                        warn!(name: "gossip.handle", { peer.id = %from, peer.addr = %addr, message.kind = kind, exception = %err }, "Failed to handle gossip {kind} message from {addr}: {err:?}");
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
                    let sent = self.transport
                        .send(
                            addr.clone(),
                            Message::SynAck(MessageMetadata::new(self_id.clone()).with_trace_context(), digest, delta),
                        )
                        .await;
                    metrics().record_gossip("synack", GossipDirection::Sent, &self_id, Some(&meta.from), sent.is_ok());
                    sent.map_err(|e| format!("Failed to send synack gossip message to peer {} at {addr}: {e:?}", meta.from))?;
                    trace!("Sent synack to {} at {}", meta.from, addr);
                }
                Message::SynAck(meta, digest, diff) => {
                    // A SynAck is a reply to a Syn we sent, so it confirms our messages are reaching
                    // this peer (the signal that distinguishes a healthy link from a one-way one).
                    self.membership.record_confirmation(&from, now);
                    let delta = self.store.diff(digest).await
                        .map_err(|e| format!("Failed to compute diff for peer {}: {e:?}", meta.from))?;
                    self.store.apply(diff).await?;

                    let sent = self.transport
                        .send(addr.clone(), Message::Ack(MessageMetadata::new(self_id.clone()).with_trace_context(), delta))
                        .await;
                    metrics().record_gossip("ack", GossipDirection::Sent, &self_id, Some(&meta.from), sent.is_ok());
                    sent.map_err(|e| format!("Failed to send ack gossip message to peer {} at {addr}: {e:?}", meta.from))?;

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
            phi_prior: Duration::from_millis(50),
            phi_threshold: 8.0,
            gossip_factor: 3,
            working_window: Duration::from_secs(60),
            reply_timeout: Duration::from_millis(250),
            peer_expiry: Duration::from_secs(300),
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

    // ---- Per-peer target selection (one address per peer per round) ------------------------------

    /// A peer that is both a configured seed and a discovered member (with a different advertised
    /// address) must receive exactly one Syn per round, at its configured (seed) address.
    #[tokio::test]
    async fn build_targets_selects_a_single_address_per_peer() {
        let a: SocketAddr = "127.0.0.1:23001".parse().unwrap();
        let seed_b: SocketAddr = "127.0.0.1:23002".parse().unwrap();
        let advertised_b: SocketAddr = "10.0.0.2:23002".parse().unwrap();
        let (na, nb) = (NodeID::new(), NodeID::new());

        let net = MockNet::new();
        let ma = socket_membership(na, a);
        ma.set_seed_addresses([seed_b]);
        let client = mock_client(&net, na, a, vec![seed_b], ma.clone());

        let now = Instant::now();
        ma.record_inbound(&nb, seed_b, now);
        ma.record_inbound(&nb, advertised_b, now);

        let targets = client.build_targets(now, &[seed_b]);
        assert_eq!(
            targets,
            vec![(Some(nb), seed_b)],
            "the peer must be targeted exactly once, at its configured address"
        );
    }

    /// When the configured address is failing its retry backoff and another discovered address is
    /// eligible, the peer is contacted (once) at the eligible address instead.
    #[tokio::test]
    async fn build_targets_uses_discovered_address_when_seed_is_backing_off() {
        let a: SocketAddr = "127.0.0.1:23011".parse().unwrap();
        let seed_b: SocketAddr = "127.0.0.1:23012".parse().unwrap();
        let advertised_b: SocketAddr = "10.0.0.2:23012".parse().unwrap();
        let (na, nb) = (NodeID::new(), NodeID::new());

        let net = MockNet::new();
        let ma = socket_membership(na, a);
        ma.set_seed_addresses([seed_b]);
        let client = mock_client(&net, na, a, vec![seed_b], ma.clone());

        let base = Instant::now();
        ma.record_inbound(&nb, seed_b, base);
        ma.record_inbound(&nb, advertised_b, base);

        // An unanswered send to the seed address trips its backoff once the reply timeout passes.
        ma.record_send(&nb, &seed_b, base + Duration::from_millis(10));
        let later = base + Duration::from_millis(500);
        ma.sweep(later);

        let targets = client.build_targets(later, &[seed_b]);
        assert_eq!(
            targets,
            vec![(Some(nb), advertised_b)],
            "the eligible discovered address must be used, and the seed must not add a second Syn"
        );
    }

    /// Multiple configured addresses that all belong to the same discovered peer must still result
    /// in a single Syn per round.
    #[tokio::test]
    async fn build_targets_collapses_multiple_seed_addresses_of_one_peer() {
        let a: SocketAddr = "127.0.0.1:23021".parse().unwrap();
        let seed1_b: SocketAddr = "127.0.0.1:23022".parse().unwrap();
        let seed2_b: SocketAddr = "10.0.0.2:23022".parse().unwrap();
        let (na, nb) = (NodeID::new(), NodeID::new());

        let net = MockNet::new();
        let ma = socket_membership(na, a);
        ma.set_seed_addresses([seed1_b, seed2_b]);
        let client = mock_client(&net, na, a, vec![seed1_b, seed2_b], ma.clone());

        let base = Instant::now();
        ma.record_inbound(&nb, seed1_b, base + Duration::from_millis(10));
        ma.record_inbound(&nb, seed2_b, base);

        let targets = client.build_targets(base + Duration::from_millis(10), &[seed1_b, seed2_b]);
        assert_eq!(
            targets,
            vec![(Some(nb), seed1_b)],
            "both configured addresses belong to the same peer, so only one may be contacted"
        );
    }

    /// A seed address that cannot be attributed to any discovered member is still always contacted
    /// (it is the only way a forgotten node can rejoin the cluster).
    #[tokio::test]
    async fn build_targets_keeps_unattributed_seed_addresses() {
        let a: SocketAddr = "127.0.0.1:23031".parse().unwrap();
        let seed: SocketAddr = "127.0.0.1:23032".parse().unwrap();
        let na = NodeID::new();

        let net = MockNet::new();
        let ma = socket_membership(na, a);
        ma.set_seed_addresses([seed]);
        let client = mock_client(&net, na, a, vec![seed], ma.clone());

        let targets = client.build_targets(Instant::now(), &[seed]);
        assert_eq!(
            targets,
            vec![(None, seed)],
            "an unknown seed address must be contacted anonymously as before"
        );
    }

    /// A seed-owning member that was not picked by the random per-round sampling is still contacted
    /// every round (seeds are never skipped), attributed to the member so its link health is
    /// tracked.
    #[tokio::test]
    async fn build_targets_attributes_unsampled_seed_members() {
        let a: SocketAddr = "127.0.0.1:23041".parse().unwrap();
        let seed_b: SocketAddr = "127.0.0.1:23042".parse().unwrap();
        let (na, nb) = (NodeID::new(), NodeID::new());

        let net = MockNet::new();
        let ma = socket_membership(na, a);
        ma.set_seed_addresses([seed_b]);
        // A gossip factor of zero means the healthy sampling never picks the peer, leaving the
        // seed loop as the only path that can contact it.
        let client = mock_client(&net, na, a, vec![seed_b], ma.clone()).with_gossip_factor(0);

        let now = Instant::now();
        ma.record_inbound(&nb, seed_b, now);

        let targets = client.build_targets(now, &[seed_b]);
        assert_eq!(
            targets,
            vec![(Some(nb), seed_b)],
            "the seed contact must be attributed to its member so sends feed its link health"
        );
    }
}
