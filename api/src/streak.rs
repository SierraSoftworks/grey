use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::Mergeable;

/// A cluster-converged record of a probe's pass/fail streaks, expressed as three
/// independently monotone markers. Every mutation moves the register up the same lattice,
/// so gossip merges, storage round-trips, and display pooling all use the one [`Streak::join`]
/// operation — and every node converges on exactly the same value (the join is commutative,
/// associative, and idempotent).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Streak {
    /// When the current (or most recent) failure episode began. Only advanced when a
    /// failure is observed while the register reads as passing, so observers joining an
    /// ongoing failure don't move its onset.
    #[serde(default, with = "chrono::serde::ts_milliseconds_option")]
    pub failing_since: Option<DateTime<Utc>>,

    /// The most recent failing observation made by any node. The probe reads as failing
    /// until this is more than [`Streak::recovery_window`] in the past, so a failure
    /// which stops being observed (transient issues, or its only observer going away)
    /// recovers on its own — there are no recovery declarations to converge on.
    #[serde(default, with = "chrono::serde::ts_milliseconds_option")]
    pub failing_until: Option<DateTime<Utc>>,

    /// The earliest passing observation made by any node. Only meaningful while the
    /// register has never recorded a failure (any failure permanently supersedes it);
    /// being a minimum, a freshly restarted node's samples can never shorten it — which
    /// is what lets rolling restarts inherit the cluster's streak.
    #[serde(default, with = "chrono::serde::ts_milliseconds_option")]
    pub covered_since: Option<DateTime<Utc>>,
}

impl Streak {
    /// How long after the last failing observation the probe is still considered to be
    /// failing. Recovery is implicit: failures which stop being observed for this long
    /// expire, rather than requiring observers to agree on a recovery.
    pub fn recovery_window() -> chrono::Duration {
        chrono::Duration::minutes(5)
    }

    /// Whether this register carries any observations at all (records written by older
    /// agents decode as empty).
    pub fn is_empty(&self) -> bool {
        self.failing_since.is_none() && self.failing_until.is_none() && self.covered_since.is_none()
    }

    /// Whether the probe reads as failing at `now`: a failure has been observed within
    /// the last [`Streak::recovery_window`].
    pub fn failing_at(&self, now: DateTime<Utc>) -> bool {
        self.failing_until
            .map(|until| until > now - Self::recovery_window())
            .unwrap_or(false)
    }

    /// Whether the probe reads as passing at `now`.
    pub fn passing_at(&self, now: DateTime<Utc>) -> bool {
        !self.failing_at(now)
    }

    /// When the state reported at `now` was entered: the failure onset while failing;
    /// after a failure expires, the streak starts at the last failing observation; and a
    /// probe which has never failed has been passing for as long as it has been watched.
    pub fn since_at(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if self.failing_at(now) {
            self.failing_since
        } else if self.failing_until.is_some() {
            self.failing_until
        } else {
            self.covered_since
        }
    }

    /// Whether the probe currently reads as failing.
    pub fn failing(&self) -> bool {
        self.failing_at(Utc::now())
    }

    /// Whether the probe currently reads as passing.
    pub fn passing(&self) -> bool {
        self.passing_at(Utc::now())
    }

    /// When the current state was entered.
    pub fn since(&self) -> Option<DateTime<Utc>> {
        self.since_at(Utc::now())
    }

    /// Folds a sample into the register. Every write is monotone (it can only move the
    /// register up the join lattice), so concurrent observations from different nodes —
    /// or even out-of-order samples — converge without coordination.
    pub fn observe(&mut self, passing: bool, time: DateTime<Utc>) {
        if passing {
            // A no-op unless this is the earliest passing observation the cluster has
            // ever made; in particular a restarted node cannot shorten the streak.
            self.covered_since = match (self.covered_since, Some(time)) {
                (Some(mine), Some(sample)) => Some(mine.min(sample)),
                (mine, sample) => mine.or(sample),
            };
        } else {
            if !self.failing_at(time) {
                // The first failure after a passing period starts a new episode; while
                // the register already reads failing, the onset stays where it was.
                self.failing_since = self.failing_since.max(Some(time));
            }

            self.failing_until = self.failing_until.max(Some(time));
        }
    }

    /// Joins another register into this one: the pointwise join of three monotone
    /// markers (latest failure onset, latest failing observation, earliest coverage).
    pub fn join(&mut self, other: &Self) {
        self.failing_since = self.failing_since.max(other.failing_since);
        self.failing_until = self.failing_until.max(other.failing_until);
        self.covered_since = match (self.covered_since, other.covered_since) {
            (Some(mine), Some(theirs)) => Some(mine.min(theirs)),
            (mine, theirs) => mine.or(theirs),
        };
    }
}

impl Mergeable for Streak {
    fn merge(&mut self, other: &Self) {
        self.join(other);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn streak(failing_since: Option<i64>, failing_until: Option<i64>, covered_since: Option<i64>) -> Streak {
        Streak {
            failing_since: failing_since.map(ts),
            failing_until: failing_until.map(ts),
            covered_since: covered_since.map(ts),
        }
    }

    /// The join must be idempotent, commutative, and associative — this is what
    /// guarantees every node converges on exactly the same register regardless of the
    /// order (or repetition) in which gossip delivers updates.
    #[test]
    fn test_join_is_a_semilattice() {
        let values = [None, Some(1), Some(5), Some(9)];
        let mut registers = Vec::new();
        for f in values {
            for u in values {
                for c in values {
                    registers.push(streak(f, u, c));
                }
            }
        }

        let join = |a: &Streak, b: &Streak| {
            let mut j = a.clone();
            j.join(b);
            j
        };

        for a in &registers {
            assert_eq!(join(a, a), *a, "idempotent: {a:?}");
            for b in &registers {
                assert_eq!(join(a, b), join(b, a), "commutative: {a:?} {b:?}");
                for c in &registers {
                    assert_eq!(
                        join(&join(a, b), c),
                        join(a, &join(b, c)),
                        "associative: {a:?} {b:?} {c:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_coverage_is_inherited_not_shortened() {
        // The cluster has watched this probe pass for a long time...
        let mut register = streak(None, None, Some(1_000));

        // ...and a freshly restarted node's samples cannot shorten that streak.
        register.observe(true, ts(500_000));
        assert_eq!(register.covered_since, Some(ts(1_000)));
        assert!(register.passing_at(ts(500_000)));
        assert_eq!(register.since_at(ts(500_000)), Some(ts(1_000)));

        // An earlier observation (e.g. replayed out of order) can only extend it.
        register.observe(true, ts(500));
        assert_eq!(register.covered_since, Some(ts(500)));
    }

    #[test]
    fn test_failure_episodes() {
        let window = Streak::recovery_window().num_seconds();
        let mut register = streak(None, None, Some(1_000));

        // A failure starts an episode and reads as failing immediately.
        register.observe(false, ts(10_000));
        assert!(register.failing_at(ts(10_000)));
        assert_eq!(register.since_at(ts(10_000)), Some(ts(10_000)));

        // Further failing observations refresh the high-water mark without moving the
        // onset, even from observers which joined the episode late.
        register.observe(false, ts(10_060));
        register.observe(false, ts(10_120));
        assert_eq!(register.failing_since, Some(ts(10_000)));
        assert_eq!(register.failing_until, Some(ts(10_120)));

        // Once no failure has been observed for the recovery window, the probe reads as
        // passing since the last failing observation...
        let recovered_at = ts(10_120 + window + 1);
        assert!(register.passing_at(recovered_at));
        assert_eq!(register.since_at(recovered_at), Some(ts(10_120)));

        // ...and coverage from before the failure is permanently superseded.
        register.observe(true, ts(10_121));
        assert_eq!(register.since_at(recovered_at), Some(ts(10_120)));

        // A failure after recovery starts a new episode with a fresh onset.
        let second_failure = 10_120 + window + 100;
        register.observe(false, ts(second_failure));
        assert!(register.failing_at(ts(second_failure)));
        assert_eq!(register.failing_since, Some(ts(second_failure)));
    }

    #[test]
    fn test_transient_subset_failure_recovers_on_its_own() {
        // One node sees a single failing sample; nobody declares a recovery.
        let window = Streak::recovery_window().num_seconds();
        let mut register = streak(None, None, Some(1_000));
        register.observe(false, ts(20_000));

        // Other nodes' passing samples don't mask the failure...
        register.observe(true, ts(20_030));
        assert!(register.failing_at(ts(20_030)));

        // ...but once the window passes without further failures, the probe recovers,
        // passing since the failing observation.
        assert!(register.passing_at(ts(20_000 + window + 1)));
        assert_eq!(register.since_at(ts(20_000 + window + 1)), Some(ts(20_000)));
    }

    #[test]
    fn test_join_converges_across_nodes() {
        // Node A carries the long coverage claim; node B witnessed a failure episode.
        let a = streak(None, None, Some(1_000));
        let b = streak(Some(50_000), Some(50_060), Some(2_000));

        let mut ab = a.clone();
        ab.join(&b);
        let mut ba = b.clone();
        ba.join(&a);
        assert_eq!(ab, ba);

        assert_eq!(ab.covered_since, Some(ts(1_000)));
        assert!(ab.failing_at(ts(50_100)));
        assert_eq!(ab.since_at(ts(50_100)), Some(ts(50_000)));

        // Joining with an empty register (a record from an older agent) is the identity.
        let mut with_empty = ab.clone();
        with_empty.join(&Streak::default());
        assert_eq!(with_empty, ab);
    }

    #[test]
    fn test_msgpack_roundtrip() {
        for register in [
            streak(Some(50_000), Some(50_060), Some(1_000)),
            streak(None, None, Some(1_000)),
            streak(Some(50_000), Some(50_060), None),
            streak(None, None, None),
        ] {
            let packed = rmp_serde::to_vec(&register).unwrap();
            let unpacked: Streak = rmp_serde::from_slice(&packed).unwrap();
            assert_eq!(register, unpacked);

            let packed = rmp_serde::to_vec_named(&register).unwrap();
            let unpacked: Streak = rmp_serde::from_slice(&packed).unwrap();
            assert_eq!(register, unpacked);
        }
    }
}
