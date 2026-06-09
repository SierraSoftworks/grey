use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;
use std::str::FromStr;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::health::{Liveness, PhiAccrualDetector};

/// A single node's membership record as it travels on the wire inside a [`MembershipSample`]. It
/// deliberately carries no `last_seen` — that is a *local* observation ("when did **I** last hear
/// from this node") and gossiping it would corrupt failure detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemberRecord {
    /// Addresses the advertising node believes are working for this member.
    pub addresses: Vec<String>,
    /// The member's boot generation (unix-millis at process start). A restart yields a larger
    /// generation so its fresh record supersedes a stale one even though its heartbeat reset to 0.
    pub generation: u64,
    /// A monotonic counter the member bumps every gossip round; its advance is the liveness signal.
    pub heartbeat: u64,
}

impl MemberRecord {
    /// A single comparable version for last-write-wins reconciliation: generation dominates, with
    /// the heartbeat breaking ties within a generation.
    pub fn version(&self) -> u128 {
        ((self.generation as u128) << 64) | (self.heartbeat as u128)
    }
}

/// A bounded, fire-and-forget sample of the memberlist gossiped to peers. Receivers merge it into
/// their own registry; it is never reconciled with a Syn/Ack handshake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MembershipSample<Peer: Eq + Hash>(HashMap<Peer, MemberRecord>);

impl<Peer: Eq + Hash> Default for MembershipSample<Peer> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<Peer: Eq + Hash> MembershipSample<Peer> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, peer: Peer, record: MemberRecord) {
        self.0.insert(peer, record);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the sample, returning one with at most `max` records (the rest ride a later round).
    pub fn truncate(self, max: usize) -> Self {
        if self.0.len() <= max {
            return self;
        }
        Self(self.0.into_iter().take(max).collect())
    }

    pub fn into_inner(self) -> HashMap<Peer, MemberRecord> {
        self.0
    }
}

/// Per-address, per-vantage link health. Times are monotonic [`Instant`]s, so they are immune to
/// wall-clock skew and never persisted.
#[derive(Debug, Clone)]
struct AddressHealth {
    /// We received a datagram from this source address.
    last_inbound: Option<Instant>,
    /// We sent a gossip message to this address.
    last_send: Option<Instant>,
    /// A reply attributable to the peer followed a send to this address (proves our send arrived).
    last_confirmed: Option<Instant>,
    /// Consecutive sends to this address that were not (yet) confirmed.
    consecutive_misses: u32,
    /// Earliest time we will gossip to this address again (per-address exponential backoff).
    backoff_until: Instant,
}

impl AddressHealth {
    fn new(now: Instant) -> Self {
        Self {
            last_inbound: None,
            last_send: None,
            last_confirmed: None,
            consecutive_misses: 0,
            backoff_until: now,
        }
    }

    /// Whether this address has demonstrated reachability recently (we received from it or had a
    /// send confirmed) and is therefore worth advertising and preferring as a target.
    fn is_working(&self, now: Instant, window: Duration) -> bool {
        [self.last_inbound, self.last_confirmed]
            .into_iter()
            .flatten()
            .any(|t| now.saturating_duration_since(t) <= window)
    }

    /// Most recent positive signal for this address, used to rank candidate addresses.
    fn last_good(&self) -> Option<Instant> {
        [self.last_inbound, self.last_confirmed].into_iter().flatten().max()
    }
}

/// All in-memory state we hold about a peer. Never persisted to redb.
struct Member<Addr: Eq + Hash> {
    generation: u64,
    heartbeat: u64,
    addresses: HashMap<Addr, AddressHealth>,
    detector: PhiAccrualDetector,
    /// When we last had any fresh signal about this member (direct receipt or heartbeat advance).
    last_seen: Instant,
    /// Wall-clock counterpart of `last_seen`, kept only so the API can render a human timestamp.
    last_seen_wall: chrono::DateTime<chrono::Utc>,
    /// When (if ever) this member was first classified dead, for grace-period expiry.
    dead_since: Option<Instant>,
    /// The liveness last reported to the tracing pipeline, so transitions are emitted edge-triggered
    /// rather than on every sweep.
    last_reported: Option<Liveness>,
}

impl<Addr: Eq + Hash> Member<Addr> {
    fn version(&self) -> u128 {
        ((self.generation as u128) << 64) | (self.heartbeat as u128)
    }
}

/// Tuning parameters for the membership registry, derived from [`crate::config::ClusterConfig`].
#[derive(Debug, Clone)]
pub struct MembershipConfig {
    pub failure_detector_window: usize,
    pub phi_prior: Duration,
    pub phi_threshold: f64,
    pub dead_grace: Duration,
    pub max_addresses: usize,
    pub working_window: Duration,
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    pub member_expiry: Duration,
}

/// A candidate peer (and the specific address) to gossip with this round.
pub struct GossipCandidate<Id, Addr> {
    pub id: Id,
    pub address: Addr,
    pub liveness: Liveness,
    /// False when this address is currently in backoff and should only be retried opportunistically.
    pub due: bool,
}

/// The in-memory cluster membership registry: who we know about, which of their addresses work, and
/// how healthy each peer is. Shared between the gossip client (writer) and the API (reader) behind an
/// [`Arc`]; all interior state is guarded by a single [`RwLock`].
pub struct Membership<Id: Eq + Hash, Addr: Eq + Hash> {
    self_id: Id,
    self_generation: u64,
    config: MembershipConfig,
    inner: RwLock<Inner<Id, Addr>>,
}

struct Inner<Id: Eq + Hash, Addr: Eq + Hash> {
    self_heartbeat: u64,
    self_addresses: Vec<String>,
    members: HashMap<Id, Member<Addr>>,
}

impl<Id, Addr> Membership<Id, Addr>
where
    Id: Eq + Hash + Clone + Display,
    Addr: Eq + Hash + Clone + Display + FromStr,
{
    pub fn new(self_id: Id, config: MembershipConfig) -> Self {
        Self {
            self_id,
            // Generation = boot wall-clock millis. A restart produces a larger value, so this node's
            // fresh record supersedes the stale one peers still hold (whose heartbeat may be higher).
            self_generation: chrono::Utc::now().timestamp_millis().max(0) as u64,
            config,
            inner: RwLock::new(Inner {
                self_heartbeat: 0,
                self_addresses: Vec::new(),
                members: HashMap::new(),
            }),
        }
    }

    pub fn self_generation(&self) -> u64 {
        self.self_generation
    }

    /// Sets the addresses this node advertises about itself (typically the configured advertised or
    /// listen address). May be empty (e.g. a wildcard listener with no advertised address).
    pub fn set_self_addresses(&self, addresses: Vec<String>) {
        self.inner.write().unwrap().self_addresses = addresses;
    }

    /// Bumps this node's own heartbeat counter; called once per gossip round so peers observe a
    /// regular liveness signal.
    pub fn bump_heartbeat(&self) -> u64 {
        let mut inner = self.inner.write().unwrap();
        inner.self_heartbeat = inner.self_heartbeat.saturating_add(1);
        inner.self_heartbeat
    }

    fn ensure_member<'a>(
        inner: &'a mut Inner<Id, Addr>,
        config: &MembershipConfig,
        id: &Id,
        now: Instant,
    ) -> &'a mut Member<Addr> {
        inner.members.entry(id.clone()).or_insert_with(|| Member {
            generation: 0,
            heartbeat: 0,
            addresses: HashMap::new(),
            detector: PhiAccrualDetector::new(config.failure_detector_window, config.phi_prior),
            last_seen: now,
            last_seen_wall: chrono::Utc::now(),
            dead_since: None,
            last_reported: None,
        })
    }

    /// Records that we received a datagram from `peer` at source address `addr`. The source is, by
    /// definition, a working address for that peer — this is the primary way the working-address set
    /// grows and the basis for only ever gossiping addresses we know to work.
    pub fn record_inbound(&self, peer: &Id, addr: Addr, now: Instant) {
        if peer == &self.self_id {
            return;
        }
        let mut inner = self.inner.write().unwrap();
        let max_addresses = self.config.max_addresses;
        let member = Self::ensure_member(&mut inner, &self.config, peer, now);
        member.last_seen = now;
        member.last_seen_wall = chrono::Utc::now();
        member.dead_since = None;
        let health = member.addresses.entry(addr).or_insert_with(|| AddressHealth::new(now));
        health.last_inbound = Some(now);
        health.consecutive_misses = 0;
        health.backoff_until = now;
        Self::bound_addresses(member, max_addresses, now);
    }

    /// Records that we sent a gossip message to `addr` for `peer`.
    pub fn record_send(&self, peer: &Id, addr: &Addr, now: Instant) {
        if peer == &self.self_id {
            return;
        }
        let mut inner = self.inner.write().unwrap();
        if let Some(member) = inner.members.get_mut(peer)
            && let Some(health) = member.addresses.get_mut(addr)
        {
            health.last_send = Some(now);
        }
    }

    /// Records that a reply from `peer` arrived after we sent it a `Syn` — proof that at least one of
    /// the addresses we recently sent to is reachable. We confirm the most-recently-sent addresses.
    pub fn record_confirmation(&self, peer: &Id, now: Instant) {
        if peer == &self.self_id {
            return;
        }
        let mut inner = self.inner.write().unwrap();
        if let Some(member) = inner.members.get_mut(peer) {
            for health in member.addresses.values_mut() {
                if health.last_send.is_some() {
                    health.last_confirmed = Some(now);
                    health.consecutive_misses = 0;
                    health.backoff_until = now;
                }
            }
        }
    }

    /// Merges a received membership sample into the registry: reconcile each member by version
    /// (last-write-wins on generation then heartbeat), union in any new advertised addresses, and
    /// feed observed heartbeat advances to the failure detector.
    pub fn merge_sample(&self, sample: MembershipSample<Id>, now: Instant) {
        let mut inner = self.inner.write().unwrap();
        let max_addresses = self.config.max_addresses;
        for (peer, record) in sample.into_inner() {
            if peer == self.self_id {
                continue;
            }
            let member = Self::ensure_member(&mut inner, &self.config, &peer, now);
            let incoming = record.version();
            if incoming > member.version() {
                // The peer is demonstrably alive and producing: count this as a heartbeat advance.
                member.generation = record.generation;
                member.heartbeat = record.heartbeat;
                member.detector.report(now);
                member.last_seen = now;
                member.last_seen_wall = chrono::Utc::now();
                member.dead_since = None;
            }
            for addr_str in record.addresses {
                if let Ok(addr) = Addr::from_str(&addr_str) {
                    member
                        .addresses
                        .entry(addr)
                        .or_insert_with(|| AddressHealth::new(now));
                }
            }
            Self::bound_addresses(member, max_addresses, now);
        }
    }

    /// Keeps a member's address set within the configured cap, evicting the least-recently-useful
    /// addresses first (never-confirmed before stale-confirmed).
    fn bound_addresses(member: &mut Member<Addr>, max: usize, _now: Instant) {
        if member.addresses.len() <= max {
            return;
        }
        // Rank by most recent good signal. `Option<Instant>` orders `None` (never good) before any
        // `Some`, so never-confirmed addresses are evicted first, then the stalest confirmed ones.
        let mut ranked: Vec<(Addr, Option<Instant>)> = member
            .addresses
            .iter()
            .map(|(a, h)| (a.clone(), h.last_good()))
            .collect();
        ranked.sort_by_key(|(_, score)| *score);
        let drop_count = member.addresses.len() - max;
        for (addr, _) in ranked.into_iter().take(drop_count) {
            member.addresses.remove(&addr);
        }
    }

    fn classify(&self, member: &Member<Addr>, now: Instant) -> Liveness {
        let window = self.config.working_window;
        let recent = |t: Option<Instant>| {
            t.map(|t| now.saturating_duration_since(t) <= window).unwrap_or(false)
        };

        let phi = member.detector.phi(now);
        // A node with no heartbeat samples yet is treated as alive so a just-learned peer is never
        // immediately declared dead.
        let alive = member.detector.last_arrival().is_none() || phi < self.config.phi_threshold;
        // Did we receive datagrams directly from this peer, did our sends get answered, are we even
        // trying to send to it?
        let received = member.addresses.values().any(|h| recent(h.last_inbound));
        let confirmed = member.addresses.values().any(|h| recent(h.last_confirmed));
        let sending = member.addresses.values().any(|h| recent(h.last_send));

        if !alive {
            let long_silence = member
                .detector
                .last_arrival()
                .map(|t| now.saturating_duration_since(t) > self.config.dead_grace)
                .unwrap_or(false);
            if long_silence {
                Liveness::Dead
            } else {
                Liveness::Suspect
            }
        } else if sending && received && !confirmed {
            // The peer is alive and reaching us, and we are actively sending to it, yet none of our
            // sends are being answered: a one-way (asymmetric) link from us to the peer.
            Liveness::Unidirectional
        } else {
            Liveness::Healthy
        }
    }

    /// Builds a bounded random sample of the memberlist to gossip, including our own record and, for
    /// each peer, only the addresses we currently believe are working.
    pub fn sample_for_gossip(&self, max_records: usize, now: Instant) -> MembershipSample<Id> {
        let inner = self.inner.read().unwrap();
        let mut sample = MembershipSample::new();

        // Always advertise ourselves (if we have an address to advertise).
        if !inner.self_addresses.is_empty() {
            sample.insert(
                self.self_id.clone(),
                MemberRecord {
                    addresses: inner.self_addresses.clone(),
                    generation: self.self_generation,
                    heartbeat: inner.self_heartbeat,
                },
            );
        }

        // Collect peers that have at least one working address, then take a bounded random subset.
        let mut candidates: Vec<&Id> = inner
            .members
            .iter()
            .filter(|(_, m)| {
                m.addresses
                    .values()
                    .any(|h| h.is_working(now, self.config.working_window))
            })
            .map(|(id, _)| id)
            .collect();
        shuffle(&mut candidates);

        for id in candidates {
            if sample.len() >= max_records {
                break;
            }
            let member = &inner.members[id];
            let addresses: Vec<String> = member
                .addresses
                .iter()
                .filter(|(_, h)| h.is_working(now, self.config.working_window))
                .map(|(a, _)| a.to_string())
                .collect();
            if addresses.is_empty() {
                continue;
            }
            sample.insert(
                id.clone(),
                MemberRecord {
                    addresses,
                    generation: member.generation,
                    heartbeat: member.heartbeat,
                },
            );
        }

        sample
    }

    /// The peers (and the single best address for each) we should consider gossiping with this round,
    /// annotated with liveness and whether the address is out of backoff.
    pub fn gossip_candidates(&self, now: Instant) -> Vec<GossipCandidate<Id, Addr>> {
        let inner = self.inner.read().unwrap();
        let mut out = Vec::new();
        for (id, member) in inner.members.iter() {
            // Pick the best address: prefer working ones, ranked by the most recent good signal.
            let best = member
                .addresses
                .iter()
                .max_by_key(|(_, h)| h.last_good());
            if let Some((addr, health)) = best {
                out.push(GossipCandidate {
                    id: id.clone(),
                    address: addr.clone(),
                    liveness: self.classify(member, now),
                    due: now >= health.backoff_until,
                });
            }
        }
        out
    }

    /// Per-round maintenance: account for unconfirmed sends (advancing per-address backoff), emit
    /// liveness transitions to the tracing pipeline, and expire members that have been dead beyond
    /// the grace period or unseen beyond the member-expiry window.
    pub fn sweep(&self, now: Instant) {
        let mut inner = self.inner.write().unwrap();
        let config = self.config.clone();
        let self_id = self.self_id.clone();

        let mut to_remove: Vec<Id> = Vec::new();
        for (id, member) in inner.members.iter_mut() {
            // Per-address backoff: a send that was never confirmed (and is older than one working
            // window) counts as a miss and pushes the address into exponential backoff.
            for health in member.addresses.values_mut() {
                if let Some(sent) = health.last_send {
                    let confirmed_after_send = health
                        .last_confirmed
                        .map(|c| c >= sent)
                        .unwrap_or(false);
                    let inbound_after_send = health
                        .last_inbound
                        .map(|i| i >= sent)
                        .unwrap_or(false);
                    if !confirmed_after_send
                        && !inbound_after_send
                        && now.saturating_duration_since(sent) > config.working_window
                        && now >= health.backoff_until
                    {
                        health.consecutive_misses = health.consecutive_misses.saturating_add(1);
                        let backoff = exponential_backoff(
                            config.backoff_base,
                            config.backoff_max,
                            health.consecutive_misses,
                        );
                        health.backoff_until = now + backoff;
                    }
                }
            }

            let liveness = self.classify(member, now);

            // Track dead_since for grace-period expiry independently of reporting.
            if liveness == Liveness::Dead {
                member.dead_since.get_or_insert(now);
            } else {
                member.dead_since = None;
            }

            // Emit transitions edge-triggered so a persistent condition is reported once, not every
            // round. A degraded state warns; a return to healthy from a degraded state informs.
            if member.last_reported != Some(liveness) {
                let phi = member.detector.phi(now);
                match liveness {
                    Liveness::Unidirectional => tracing::warn!(
                        name: "cluster.health.transition",
                        { peer.id = %id, state = liveness.as_str(), kind = liveness.as_str(), phi = phi },
                        "Peer {id} can reach us but is not receiving our messages (unidirectional link)."
                    ),
                    Liveness::Suspect | Liveness::Dead => tracing::warn!(
                        name: "cluster.health.transition",
                        { peer.id = %id, state = liveness.as_str(), kind = liveness.as_str(), phi = phi },
                        "Peer {id} is {} (no gossip heartbeats observed).", liveness.as_str()
                    ),
                    Liveness::Healthy => {
                        if member.last_reported.map(|l| l.is_degraded()).unwrap_or(false) {
                            tracing::info!(
                                name: "cluster.health.transition",
                                { peer.id = %id, state = liveness.as_str(), kind = liveness.as_str(), phi = phi },
                                "Peer {id} link recovered."
                            );
                        }
                    }
                }
                member.last_reported = Some(liveness);
            }

            let expired_unseen =
                now.saturating_duration_since(member.last_seen) > config.member_expiry;
            let expired_dead = member
                .dead_since
                .map(|d| now.saturating_duration_since(d) > config.dead_grace)
                .unwrap_or(false);
            if expired_unseen || expired_dead {
                to_remove.push(id.clone());
            }
        }

        for id in to_remove {
            if id != self_id {
                inner.members.remove(&id);
            }
        }
    }

    /// A redacted view of known peers for the API/UI: identifier and last-seen only. **Addresses are
    /// intentionally never exposed** (the API has no access control and may be public).
    pub fn redacted_peers(&self) -> Vec<(String, chrono::DateTime<chrono::Utc>)> {
        let inner = self.inner.read().unwrap();
        inner
            .members
            .iter()
            .map(|(id, m)| (id.to_string(), m.last_seen_wall))
            .collect()
    }

    #[cfg(test)]
    pub fn liveness_of(&self, peer: &Id, now: Instant) -> Option<Liveness> {
        let inner = self.inner.read().unwrap();
        inner.members.get(peer).map(|m| self.classify(m, now))
    }

    #[cfg(test)]
    pub fn known_addresses(&self, peer: &Id) -> Vec<Addr> {
        let inner = self.inner.read().unwrap();
        inner
            .members
            .get(peer)
            .map(|m| m.addresses.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn member_count(&self) -> usize {
        self.inner.read().unwrap().members.len()
    }
}

/// `min(base * 2^(misses-1), max)`, saturating, used for per-address retry backoff.
fn exponential_backoff(base: Duration, max: Duration, misses: u32) -> Duration {
    if misses == 0 {
        return Duration::ZERO;
    }
    let shift = (misses - 1).min(32);
    let scaled = base.saturating_mul(1u32 << shift);
    scaled.min(max)
}

/// In-place Fisher–Yates shuffle (we only need a cheap, unbiased reorder for sampling).
fn shuffle<T>(items: &mut [T]) {
    use rand::RngExt;
    let mut rng = rand::rng();
    let len = items.len();
    for i in (1..len).rev() {
        let j = rng.random_range(0..=i);
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MembershipConfig {
        MembershipConfig {
            failure_detector_window: 100,
            phi_prior: Duration::from_secs(1),
            phi_threshold: 8.0,
            dead_grace: Duration::from_secs(10),
            max_addresses: 4,
            working_window: Duration::from_secs(3),
            backoff_base: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
            member_expiry: Duration::from_secs(120),
        }
    }

    fn nid(n: u128) -> crate::cluster::NodeID {
        crate::cluster::NodeID::from(n)
    }

    fn addr(s: &str) -> std::net::SocketAddr {
        s.parse().unwrap()
    }

    fn record(addrs: &[&str], generation: u64, heartbeat: u64) -> MemberRecord {
        MemberRecord {
            addresses: addrs.iter().map(|s| s.to_string()).collect(),
            generation,
            heartbeat,
        }
    }

    #[test]
    fn record_version_orders_by_generation_then_heartbeat() {
        // A restart (higher generation, heartbeat reset to 0) supersedes a high pre-restart heartbeat.
        let restarted = record(&[], 5, 0);
        let pre_restart = record(&[], 4, u64::MAX);
        assert!(restarted.version() > pre_restart.version());
    }

    #[test]
    fn merge_unions_addresses_and_supersedes_by_version() {
        let m = Membership::<_, std::net::SocketAddr>::new(nid(1), test_config());
        let base = Instant::now();

        let mut s1 = MembershipSample::new();
        s1.insert(nid(2), record(&["10.0.0.2:8888"], 1, 1));
        m.merge_sample(s1, base);
        assert_eq!(m.known_addresses(&nid(2)).len(), 1);

        // A newer record adds another address.
        let mut s2 = MembershipSample::new();
        s2.insert(nid(2), record(&["10.0.0.2:8888", "10.1.0.2:8888"], 1, 2));
        m.merge_sample(s2, base + Duration::from_secs(1));
        let known = m.known_addresses(&nid(2));
        assert_eq!(known.len(), 2, "the new address should be unioned in");
    }

    #[test]
    fn merge_drops_self_records() {
        let m = Membership::<_, std::net::SocketAddr>::new(nid(1), test_config());
        let mut s = MembershipSample::new();
        s.insert(nid(1), record(&["127.0.0.1:1"], 9, 9));
        m.merge_sample(s, Instant::now());
        assert_eq!(m.member_count(), 0, "we must never store a member record for ourselves");
    }

    #[test]
    fn address_set_is_bounded() {
        let m = Membership::<_, std::net::SocketAddr>::new(nid(1), test_config());
        let base = Instant::now();
        let mut s = MembershipSample::new();
        s.insert(
            nid(2),
            record(
                &["10.0.0.1:1", "10.0.0.2:1", "10.0.0.3:1", "10.0.0.4:1", "10.0.0.5:1", "10.0.0.6:1"],
                1,
                1,
            ),
        );
        m.merge_sample(s, base);
        assert!(m.known_addresses(&nid(2)).len() <= 4, "address set must respect max_addresses");
    }

    #[test]
    fn inbound_marks_address_working_and_keeps_node_healthy() {
        let m = Membership::<_, std::net::SocketAddr>::new(nid(1), test_config());
        let base = Instant::now();
        m.record_inbound(&nid(2), addr("10.0.0.2:8888"), base);
        // Healthy: we have a working address and the detector has no reason to suspect.
        assert_eq!(m.liveness_of(&nid(2), base), Some(Liveness::Healthy));
    }

    #[test]
    fn unidirectional_when_alive_but_our_sends_are_unanswered() {
        let m = Membership::<_, std::net::SocketAddr>::new(nid(1), test_config());
        let base = Instant::now();
        let a = addr("10.0.0.2:8888");

        // The peer's gossip reaches us (so its address is a working inbound source and its heartbeat
        // keeps advancing — it is alive), mirroring how `handle_message` records an inbound for every
        // datagram before merging the sample.
        for hb in 1..6 {
            m.record_inbound(&nid(2), a, base + Duration::from_secs(hb));
            let mut s = MembershipSample::new();
            s.insert(nid(2), record(&["10.0.0.2:8888"], 1, hb));
            m.merge_sample(s, base + Duration::from_secs(hb));
        }
        // We send to it, but our messages are never answered (no confirmation).
        m.record_send(&nid(2), &a, base + Duration::from_secs(6));

        let now = base + Duration::from_secs(7);
        assert_eq!(m.liveness_of(&nid(2), now), Some(Liveness::Unidirectional));
    }

    #[test]
    fn dead_when_heartbeats_stop_for_long_enough() {
        let m = Membership::<_, std::net::SocketAddr>::new(nid(1), test_config());
        let base = Instant::now();
        // Establish a ~1s heartbeat cadence.
        for hb in 1..5 {
            let mut s = MembershipSample::new();
            s.insert(nid(2), record(&["10.0.0.2:8888"], 1, hb));
            m.merge_sample(s, base + Duration::from_secs(hb));
        }
        // Long after the last heartbeat (well past dead_grace), with no inbound, it is Dead.
        let now = base + Duration::from_secs(4 + 30);
        assert_eq!(m.liveness_of(&nid(2), now), Some(Liveness::Dead));
    }

    #[test]
    fn sample_includes_self_and_only_working_peer_addresses() {
        let m = Membership::<_, std::net::SocketAddr>::new(nid(1), test_config());
        m.set_self_addresses(vec!["10.0.0.1:8888".to_string()]);
        let base = Instant::now();

        // A working peer (we received from it) and a peer we only heard about (no working address).
        m.record_inbound(&nid(2), addr("10.0.0.2:8888"), base);
        let mut s = MembershipSample::new();
        s.insert(nid(3), record(&["10.0.0.3:8888"], 1, 1));
        m.merge_sample(s, base);

        m.bump_heartbeat();
        let sample = m.sample_for_gossip(16, base).into_inner();
        assert!(sample.contains_key(&nid(1)), "our own record must be advertised");
        assert!(sample.contains_key(&nid(2)), "a working peer must be advertised");
        assert!(
            !sample.contains_key(&nid(3)),
            "a peer with no confirmed-working address must not be advertised"
        );
    }

    #[test]
    fn exponential_backoff_grows_and_caps() {
        let base = Duration::from_secs(1);
        let max = Duration::from_secs(60);
        assert_eq!(exponential_backoff(base, max, 0), Duration::ZERO);
        assert_eq!(exponential_backoff(base, max, 1), Duration::from_secs(1));
        assert_eq!(exponential_backoff(base, max, 2), Duration::from_secs(2));
        assert_eq!(exponential_backoff(base, max, 3), Duration::from_secs(4));
        assert_eq!(exponential_backoff(base, max, 30), max, "must cap at max");
    }
}
