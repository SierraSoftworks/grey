use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::Mergeable;

/// A pair of streak markers describing a probe's state over time: when the current (or
/// most recent) passing streak began, and when the current (or most recent) failure
/// began. Whichever marker is more recent is the current state; the other is retained as
/// history. A streak which carries a `failing_since` witnessed the recovery its
/// `passing_since` describes; one without has merely been watching the probe pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Streak {
    #[serde(default, with = "chrono::serde::ts_milliseconds_option")]
    pub passing_since: Option<DateTime<Utc>>,

    #[serde(default, with = "chrono::serde::ts_milliseconds_option")]
    pub failing_since: Option<DateTime<Utc>>,
}

impl Streak {
    fn from_sample(passing: bool, time: DateTime<Utc>) -> Self {
        Self {
            passing_since: passing.then_some(time),
            failing_since: (!passing).then_some(time),
        }
    }

    /// Whether the probe is currently failing according to this streak. A tie between
    /// the markers is treated pessimistically.
    pub fn failing(&self) -> bool {
        self.failing_since.is_some() && self.failing_since >= self.passing_since
    }

    /// Whether the probe is currently passing according to this streak.
    pub fn passing(&self) -> bool {
        !self.failing()
    }

    /// When the current state was entered, as far back as this streak can attest.
    pub fn since(&self) -> Option<DateTime<Utc>> {
        if self.failing() {
            self.failing_since
        } else {
            self.passing_since
        }
    }

    /// Applies a sample: the marker for the observed state only moves when the state
    /// flips, so it marks the start of the current streak.
    fn observe(&mut self, passing: bool, time: DateTime<Utc>) {
        if passing {
            if self.passing_since.is_none() || self.failing_since > self.passing_since {
                self.passing_since = Some(time);
            }
        } else if self.failing_since.is_none() || self.passing_since > self.failing_since {
            self.failing_since = Some(time);
        }
    }

    /// Adopts history from a peer's streak without ever changing the state this streak
    /// currently reports — only the markers are refined.
    fn repair(&mut self, other: &Self) {
        match (self.failing(), other.failing()) {
            // Both failing: push our failure start back to the peer's earlier claim,
            // unless we observed the probe passing after that claim began.
            (true, true) => {
                if self.passing_since < other.failing_since {
                    self.failing_since = self.failing_since.min(other.failing_since);
                }
            }

            // The peer's passing attestation refines our pre-failure history, but its
            // state never overrides the failure we're observing ourselves.
            (true, false) => {
                if other.passing_since < self.failing_since {
                    self.passing_since = self.passing_since.max(other.passing_since);
                }
            }

            // The peer reports an active failure we aren't seeing: our own samples
            // define our state, and read-time aggregation already defers to the peer.
            // Adopting it here would echo the failure back to the peer's own repairs,
            // and the cluster could never converge back to passing.
            (false, true) => {}

            (false, false) => self.combine_passing(other),
        }
    }

    /// Combines two passing streaks. Streaks which witnessed a failure "vote": the merged
    /// streak only reaches back to the point everyone agrees it recovered (latest
    /// `passing_since` among them). Streaks which never witnessed a failure don't vote —
    /// they adopt a voter's streak wholesale, or pool pure coverage (earliest start)
    /// with each other.
    fn combine_passing(&mut self, other: &Self) {
        match (self.failing_since, other.failing_since) {
            (Some(_), Some(_)) => {
                self.passing_since = self.passing_since.max(other.passing_since);
                self.failing_since = self.failing_since.max(other.failing_since);
            }
            (Some(_), None) => {}
            (None, Some(_)) => {
                self.passing_since = other.passing_since;
                self.failing_since = other.failing_since;
            }
            (None, None) => {
                self.passing_since = match (self.passing_since, other.passing_since) {
                    (Some(mine), Some(theirs)) => Some(mine.min(theirs)),
                    (mine, theirs) => mine.or(theirs),
                };
            }
        }
    }

    /// Read-time aggregation. Unlike [`Streak::repair`] this defers to an active failure
    /// reported by either side, and it is order-independent: any active failure wins,
    /// pushed back to the earliest claim that no observer has seen the probe pass since.
    fn merge(&mut self, other: &Self) {
        match (self.failing(), other.failing()) {
            (true, true) => {
                let last_passing = self.passing_since.max(other.passing_since);
                let pushed_back = [self.failing_since, other.failing_since]
                    .into_iter()
                    .flatten()
                    .filter(|failed| Some(*failed) > last_passing)
                    .min()
                    .map(Some);

                // The side holding the latest passing attestation is itself failing, so
                // its claim always survives the filter; the fallback is defensive only.
                self.failing_since = pushed_back.unwrap_or(self.failing_since.min(other.failing_since));
                self.passing_since = last_passing;
            }
            (true, false) => {
                let last_passing = self.passing_since.max(other.passing_since);
                if last_passing < self.failing_since {
                    self.passing_since = last_passing;
                }
            }
            (false, true) => {
                let last_passing = self.passing_since.max(other.passing_since);
                self.failing_since = other.failing_since;
                self.passing_since = if last_passing < other.failing_since {
                    last_passing
                } else {
                    other.passing_since
                };
            }
            (false, false) => self.combine_passing(other),
        }
    }
}

/// The state of a probe as reported by a single observer: the streak it has observed
/// with its own samples, and the cluster-converged streak it derives from peers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeStatus {
    /// The streak this observer has seen with its own samples. Only ever updated by
    /// [`ProbeStatus::record`] — peers never influence it — and only meaningful to the
    /// observer which authored the record.
    #[serde(default)]
    pub observed: Streak,

    /// The cluster-converged streak: re-seeded from `observed` at every sample and then
    /// refined from peers via [`ProbeStatus::repair`], so adopted history is always
    /// backed by a currently-live record. This is the view exposed to the API/UI.
    #[serde(default)]
    pub converged: Streak,

    /// When this status was last confirmed by a sample. Statuses which haven't been
    /// confirmed recently belong to observers that have stopped reporting and should be
    /// ignored when aggregating or repairing.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub updated: DateTime<Utc>,
}

impl ProbeStatus {
    /// How long a reported status remains valid without being confirmed by a new sample.
    fn validity() -> chrono::Duration {
        chrono::Duration::hours(1)
    }

    /// Begins tracking state from a first sample; the state before it is unknown, so the
    /// record starts with no history and doesn't constrain the rest of the cluster.
    pub fn from_sample(passing: bool, time: DateTime<Utc>) -> Self {
        let observed = Streak::from_sample(passing, time);
        Self {
            converged: observed.clone(),
            observed,
            updated: time,
        }
    }

    /// Whether this observer currently considers the probe to be failing.
    pub fn failing(&self) -> bool {
        self.converged.failing()
    }

    /// Whether this observer currently considers the probe to be passing.
    pub fn passing(&self) -> bool {
        self.converged.passing()
    }

    /// When the current state was entered, as far back as the cluster can attest.
    pub fn since(&self) -> Option<DateTime<Utc>> {
        self.converged.since()
    }

    /// Whether this status has been confirmed by a sample recently enough to count when
    /// aggregating or repairing.
    pub fn is_current(&self, now: DateTime<Utc>) -> bool {
        now - self.updated < Self::validity()
    }

    /// Records a sample against the observed streak. Samples further than `max_gap`
    /// apart break coverage: the time in between is unknown, so the observed streak
    /// resets and the record rejoins the cluster as a fresh, non-voting observer (its
    /// history is repaired from its peers, not silently bridged). The converged streak
    /// is re-seeded from the observed one, washing out any previously adopted history
    /// which the peers no longer attest.
    pub fn record(&mut self, passing: bool, time: DateTime<Utc>, max_gap: chrono::Duration) {
        if time < self.updated {
            return;
        }

        if time - self.updated > max_gap {
            self.observed = Streak::from_sample(passing, time);
        } else {
            self.observed.observe(passing, time);
        }

        self.converged = self.observed.clone();
        self.updated = time;
    }

    /// Adopts streak history from a peer's converged view into our own, without ever
    /// changing the state this observer is seeing with its own samples. This is what
    /// lets a freshly restarted node re-learn the cluster's streak so rolling restarts
    /// don't lose it.
    pub fn repair(&mut self, other: &Self) {
        self.converged.repair(&other.converged);
    }
}

impl Mergeable for ProbeStatus {
    /// Read-time aggregation of two observers' converged views, deferring to an active
    /// failure reported by either side.
    fn merge(&mut self, other: &Self) {
        self.converged.merge(&other.converged);
        self.updated = self.updated.max(other.updated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn streak(passing_since: Option<i64>, failing_since: Option<i64>) -> Streak {
        Streak {
            passing_since: passing_since.map(ts),
            failing_since: failing_since.map(ts),
        }
    }

    /// A status as it would look right after a sample: observed and converged agree.
    fn status(passing_since: Option<i64>, failing_since: Option<i64>, updated: i64) -> ProbeStatus {
        ProbeStatus {
            observed: streak(passing_since, failing_since),
            converged: streak(passing_since, failing_since),
            updated: ts(updated),
        }
    }

    fn gap() -> chrono::Duration {
        chrono::Duration::seconds(120)
    }

    #[test]
    fn test_record_tracks_streaks() {
        let mut s = ProbeStatus::from_sample(true, ts(100));
        assert!(s.passing());
        assert_eq!(s.since(), Some(ts(100)));
        assert_eq!(s.converged.failing_since, None);

        // Repeating the same state keeps the streak and refreshes the confirmation time.
        s.record(true, ts(160), gap());
        assert_eq!(s.since(), Some(ts(100)));
        assert_eq!(s.updated, ts(160));

        // A flip starts the failing streak, keeping the passing marker as history.
        s.record(false, ts(220), gap());
        assert!(s.failing());
        assert_eq!(s.since(), Some(ts(220)));
        assert_eq!(s.converged.passing_since, Some(ts(100)));

        // Recovery starts a new passing streak, keeping the failure as history.
        s.record(true, ts(280), gap());
        assert!(s.passing());
        assert_eq!(s.since(), Some(ts(280)));
        assert_eq!(s.converged.failing_since, Some(ts(220)));

        // Out-of-order older samples are ignored.
        s.record(false, ts(250), gap());
        assert!(s.passing());
        assert_eq!(s.since(), Some(ts(280)));
    }

    #[test]
    fn test_record_gap_resets_to_fresh_observer() {
        let mut s = status(Some(280), Some(220), 280);

        // A coverage gap resets the observed streak: the record rejoins as a non-voter
        // whose history will be repaired from its peers rather than silently bridged.
        s.record(true, ts(280 + 3600), gap());
        assert!(s.passing());
        assert_eq!(s.observed.passing_since, Some(ts(280 + 3600)));
        assert_eq!(s.observed.failing_since, None);
        assert_eq!(s.converged, s.observed);
    }

    #[test]
    fn test_record_reseeds_converged_from_observed() {
        // A non-voter adopts a peer's streak into its converged view...
        let mut s = status(Some(8_000), None, 9_000);
        s.repair(&status(Some(1_000), Some(500), 9_000));
        assert_eq!(s.converged.passing_since, Some(ts(1_000)));

        // ...without its own observations being touched...
        assert_eq!(s.observed.passing_since, Some(ts(8_000)));

        // ...and the adoption washes out at the next sample, only to be re-adopted if
        // the peers still attest it.
        s.record(true, ts(9_060), gap());
        assert_eq!(s.converged.passing_since, Some(ts(8_000)));
        assert_eq!(s.converged.failing_since, None);
    }

    #[test]
    fn test_repair_restores_streak_after_restart() {
        // A freshly restarted node (no failure history — a non-voter) adopts a peer's
        // streak wholesale, whether that extends or shortens its own claim.
        let mut restarted = ProbeStatus::from_sample(true, ts(9_000));
        restarted.repair(&status(Some(1_000), Some(500), 9_000));
        assert!(restarted.passing());
        assert_eq!(restarted.converged.passing_since, Some(ts(1_000)));
        assert_eq!(restarted.converged.failing_since, Some(ts(500)));

        // ...including when the peer's recovery is more recent than our own coverage,
        // since the peer witnessed a failure we never saw.
        let mut covering = status(Some(1_000), None, 9_000);
        covering.repair(&status(Some(8_000), Some(7_000), 9_000));
        assert_eq!(covering.converged.passing_since, Some(ts(8_000)));
        assert_eq!(covering.converged.failing_since, Some(ts(7_000)));
    }

    #[test]
    fn test_repair_passing_votes_and_coverage() {
        // Two failure witnesses must agree: the streak starts at the latest recovery.
        let mut voter = status(Some(5_000), Some(4_000), 9_000);
        voter.repair(&status(Some(6_000), Some(5_500), 9_000));
        assert_eq!(voter.converged.passing_since, Some(ts(6_000)));
        assert_eq!(voter.converged.failing_since, Some(ts(5_500)));

        // A non-voter cannot drag a witness's recovery backwards.
        let mut witness = status(Some(5_000), Some(4_000), 9_000);
        witness.repair(&status(Some(1_000), None, 9_000));
        assert_eq!(witness.converged.passing_since, Some(ts(5_000)));

        // Two non-voters pool pure coverage: the earliest attested start wins.
        let mut fresh = status(Some(8_000), None, 9_000);
        fresh.repair(&status(Some(2_000), None, 9_000));
        assert_eq!(fresh.converged.passing_since, Some(ts(2_000)));
        assert_eq!(fresh.converged.failing_since, None);
    }

    #[test]
    fn test_repair_failing_pushes_back() {
        // Both failing: adopt the earliest failure claim...
        let mut failing = status(Some(1_000), Some(8_000), 9_000);
        failing.repair(&status(None, Some(6_000), 9_000));
        assert!(failing.failing());
        assert_eq!(failing.converged.failing_since, Some(ts(6_000)));

        // ...unless we saw the probe pass after that claim began.
        let mut seen_passing = status(Some(7_000), Some(8_000), 9_000);
        seen_passing.repair(&status(Some(1_000), Some(6_000), 9_000));
        assert_eq!(seen_passing.converged.failing_since, Some(ts(8_000)));
    }

    #[test]
    fn test_repair_never_flips_observed_state() {
        // A passing node doesn't adopt a peer's active failure into its own record...
        let mut passing = status(Some(5_000), None, 9_000);
        passing.repair(&status(Some(1_000), Some(6_000), 9_000));
        assert!(passing.passing());
        assert_eq!(passing.converged.passing_since, Some(ts(5_000)));

        // ...and a failing node doesn't adopt a passing marker that would mask its
        // failure, though earlier attestations refine its history.
        let mut failing = status(Some(1_000), Some(6_000), 9_000);
        failing.repair(&status(Some(7_000), None, 9_000));
        assert!(failing.failing());
        assert_eq!(failing.converged.passing_since, Some(ts(1_000)));

        failing.repair(&status(Some(2_000), None, 9_000));
        assert!(failing.failing());
        assert_eq!(failing.converged.passing_since, Some(ts(2_000)));
    }

    #[test]
    fn test_merge_defers_to_active_failures() {
        // Read-time aggregation reports a failure seen by either side, in both orders.
        let passing = status(Some(1_000), None, 9_000);
        let failing = status(Some(1_000), Some(8_000), 8_500);

        let mut a = passing.clone();
        a.merge(&failing);
        assert!(a.failing());
        assert_eq!(a.since(), Some(ts(8_000)));
        assert_eq!(a.updated, ts(9_000));

        let mut b = failing.clone();
        b.merge(&passing);
        assert!(b.failing());
        assert_eq!(b.since(), Some(ts(8_000)));
        assert_eq!(b.updated, ts(9_000));
    }

    #[test]
    fn test_merge_failing_pushback_is_order_independent() {
        // One observer has been failing since 6_000 but last saw the probe pass at
        // 7_000... the other failed at 8_000 having passed at 7_000. The push-back is
        // capped by the latest passing attestation in both merge orders.
        let early = status(Some(2_000), Some(6_000), 9_000);
        let late = status(Some(7_000), Some(8_000), 9_000);

        let mut a = early.clone();
        a.merge(&late);
        let mut b = late.clone();
        b.merge(&early);

        assert!(a.failing() && b.failing());
        assert_eq!(a.since(), Some(ts(8_000)));
        assert_eq!(b.since(), Some(ts(8_000)));

        // With no passing attestation after the earlier claim, the failure pushes back.
        let quiet = status(Some(1_000), Some(6_000), 9_000);
        let mut c = late.clone();
        c.merge(&quiet);
        let mut d = quiet.clone();
        d.merge(&late);
        // late passed at 7_000, after quiet's claim began, so the cap still applies...
        assert_eq!(c.since(), Some(ts(8_000)));
        assert_eq!(d.since(), Some(ts(8_000)));

        // ...but two quiet failures converge on the earliest claim.
        let mut e = quiet.clone();
        e.merge(&status(Some(1_500), Some(3_000), 9_000));
        assert_eq!(e.since(), Some(ts(3_000)));
        let mut f = status(Some(1_500), Some(3_000), 9_000);
        f.merge(&quiet);
        assert_eq!(f.since(), Some(ts(3_000)));
    }

    #[test]
    fn test_merge_passing_is_order_independent() {
        let statuses = [
            status(Some(5_000), Some(4_000), 9_000),  // witnessed recovery at 5_000
            status(Some(1_000), None, 9_100),         // pure coverage from 1_000
            status(Some(3_000), Some(2_000), 9_200),  // witnessed recovery at 3_000
        ];

        for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [2, 0, 1], [1, 2, 0], [0, 2, 1]] {
            let mut acc = statuses[order[0]].clone();
            acc.merge(&statuses[order[1]]);
            acc.merge(&statuses[order[2]]);

            assert!(acc.passing(), "order {order:?}");
            assert_eq!(acc.since(), Some(ts(5_000)), "order {order:?}");
            assert_eq!(acc.converged.failing_since, Some(ts(4_000)), "order {order:?}");
            assert_eq!(acc.updated, ts(9_200), "order {order:?}");
        }
    }

    #[test]
    fn test_msgpack_roundtrip() {
        for status in [
            status(Some(1_000), Some(500), 2_000),
            status(Some(1_000), None, 2_000),
            status(None, Some(500), 2_000),
        ] {
            let packed = rmp_serde::to_vec(&status).unwrap();
            let unpacked: ProbeStatus = rmp_serde::from_slice(&packed).unwrap();
            assert_eq!(status, unpacked);

            let packed = rmp_serde::to_vec_named(&status).unwrap();
            let unpacked: ProbeStatus = rmp_serde::from_slice(&packed).unwrap();
            assert_eq!(status, unpacked);
        }
    }
}
