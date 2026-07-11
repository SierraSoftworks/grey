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
    /// until this is more than the recovery window (a probe/cron's `alerting.debounce`,
    /// defaulting to [`Streak::default_recovery_window`]) in the past, so a failure which
    /// stops being observed (transient issues, or its only observer going away) recovers
    /// on its own — there are no recovery declarations to converge on.
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
    /// The recovery window applied when a caller does not supply an explicit one: how long after the
    /// last failing observation an entity is still considered failing before recovery is implied.
    /// The window is configurable per-entity (a probe/cron's `alerting.debounce`); this is the
    /// fallback preserving the historical behaviour.
    pub fn default_recovery_window() -> chrono::Duration {
        chrono::Duration::minutes(5)
    }

    /// Whether this register carries any observations at all (records written by older
    /// agents decode as empty).
    pub fn is_empty(&self) -> bool {
        self.failing_since.is_none() && self.failing_until.is_none() && self.covered_since.is_none()
    }

    /// Whether a failure has been observed within the last `window` at `now` — the raw (un-debounced)
    /// failing signal, used both for display of an in-progress fault and as the building block for the
    /// debounced [`Streak::failing_for`].
    pub fn failing_at(&self, now: DateTime<Utc>, window: chrono::Duration) -> bool {
        self.failing_until
            .map(|until| until > now - window)
            .unwrap_or(false)
    }

    /// Whether no failure has been observed within the last `window` at `now`.
    pub fn passing_at(&self, now: DateTime<Utc>, window: chrono::Duration) -> bool {
        !self.failing_at(now, window)
    }

    /// The debounced failing signal: whether the entity has been *continuously* failing for at least
    /// `window` at `now`. This is true only once a failure has been observed within the last `window`
    /// (still failing) **and** the current episode began at least `window` ago — so a fault shorter
    /// than `window` never trips it, and a genuine one trips exactly `window` after it began. Because
    /// `failing_since` only advances at the start of a fresh episode, "began ≥ window ago" implies no
    /// recovery occurred in between.
    pub fn failing_for(&self, now: DateTime<Utc>, window: chrono::Duration) -> bool {
        self.failing_at(now, window)
            && self
                .failing_since
                .map(|since| now - since >= window)
                .unwrap_or(false)
    }

    /// The debounced health at `now`: the entity reads healthy unless it has been continuously
    /// failing for at least `window` (see [`Streak::failing_for`]). Symmetric — a failure shorter
    /// than `window`, or a recovery younger than `window`, reads as the prior (healthy) state.
    pub fn healthy_at(&self, now: DateTime<Utc>, window: chrono::Duration) -> bool {
        !self.failing_for(now, window)
    }

    /// When the debounced state reported at `now` was entered: the failure onset while (debounced)
    /// failing; otherwise the last failing observation, or — for an entity which has never failed —
    /// the earliest passing observation.
    pub fn since_at(&self, now: DateTime<Utc>, window: chrono::Duration) -> Option<DateTime<Utc>> {
        if self.failing_for(now, window) {
            self.failing_since
        } else {
            self.failing_until.or(self.covered_since)
        }
    }

    /// Folds a sample into the register. Every write is monotone (it can only move the
    /// register up the join lattice), so concurrent observations from different nodes —
    /// or even out-of-order samples — converge without coordination. `window` decides whether a
    /// failing sample continues the current episode or starts a fresh one.
    pub fn observe(&mut self, passing: bool, time: DateTime<Utc>, window: chrono::Duration) {
        if passing {
            // A no-op unless this is the earliest passing observation the cluster has
            // ever made; in particular a restarted node cannot shorten the streak.
            self.covered_since = match (self.covered_since, Some(time)) {
                (Some(mine), Some(sample)) => Some(mine.min(sample)),
                (mine, sample) => mine.or(sample),
            };
        } else {
            if !self.failing_at(time, window) {
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

    /// The window used by tests that don't care about the exact debounce value.
    fn win() -> chrono::Duration {
        Streak::default_recovery_window()
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
        register.observe(true, ts(500_000), win());
        assert_eq!(register.covered_since, Some(ts(1_000)));
        assert!(register.passing_at(ts(500_000), win()));
        assert_eq!(register.since_at(ts(500_000), win()), Some(ts(1_000)));

        // An earlier observation (e.g. replayed out of order) can only extend it.
        register.observe(true, ts(500), win());
        assert_eq!(register.covered_since, Some(ts(500)));
    }

    #[test]
    fn test_failure_episodes() {
        let window = win().num_seconds();
        let mut register = streak(None, None, Some(1_000));

        // A failure starts an episode and reads as failing immediately.
        register.observe(false, ts(10_000), win());
        assert!(register.failing_at(ts(10_000), win()));
        assert_eq!(register.since_at(ts(10_000), win()), Some(ts(10_000)));

        // Further failing observations refresh the high-water mark without moving the
        // onset, even from observers which joined the episode late.
        register.observe(false, ts(10_060), win());
        register.observe(false, ts(10_120), win());
        assert_eq!(register.failing_since, Some(ts(10_000)));
        assert_eq!(register.failing_until, Some(ts(10_120)));

        // Once no failure has been observed for the recovery window, the probe reads as
        // passing since the last failing observation...
        let recovered_at = ts(10_120 + window + 1);
        assert!(register.passing_at(recovered_at, win()));
        assert_eq!(register.since_at(recovered_at, win()), Some(ts(10_120)));

        // ...and coverage from before the failure is permanently superseded.
        register.observe(true, ts(10_121), win());
        assert_eq!(register.since_at(recovered_at, win()), Some(ts(10_120)));

        // A failure after recovery starts a new episode with a fresh onset.
        let second_failure = 10_120 + window + 100;
        register.observe(false, ts(second_failure), win());
        assert!(register.failing_at(ts(second_failure), win()));
        assert_eq!(register.failing_since, Some(ts(second_failure)));
    }

    /// The debounced [`Streak::failing_for`] only trips once a fault has persisted for the whole
    /// window, and clears once no failure has been observed for the window — the symmetric hysteresis
    /// that gates alerting.
    #[test]
    fn test_failing_for_debounces_both_directions() {
        let window = chrono::Duration::minutes(5);
        let w = window.num_seconds();
        let mut register = streak(None, None, Some(0));

        // A sustained fault: an onset at t=1000, kept alive by failures every half-window so it stays
        // one continuous episode (failing_since pinned at 1000, failing_until advancing).
        register.observe(false, ts(1_000), window);
        for k in 1..=4 {
            register.observe(false, ts(1_000 + k * (w / 2)), window);
        }
        assert_eq!(register.failing_since, Some(ts(1_000)), "the episode stays continuous");
        let last_fail = 1_000 + 2 * w;
        assert_eq!(register.failing_until, Some(ts(last_fail)));

        // Not debounced-failing until the episode is a full window old; then it trips.
        assert!(!register.failing_for(ts(1_000 + w - 1), window), "onset must not trip before the window");
        assert!(register.healthy_at(ts(1_000 + w - 1), window));
        assert!(register.failing_for(ts(1_000 + w), window), "a sustained fault trips at exactly the window");
        assert_eq!(register.since_at(ts(1_000 + w), window), Some(ts(1_000)));

        // Recovery is likewise debounced: it stays failing until a full window after the last failure.
        assert!(register.failing_for(ts(last_fail + w - 1), window), "recovery must not clear before the window");
        assert!(register.healthy_at(ts(last_fail + w + 1), window));
    }

    /// A blip shorter than the window never trips the debounced signal in either direction.
    #[test]
    fn test_failing_for_ignores_short_blips() {
        let window = chrono::Duration::minutes(5);
        let mut register = streak(None, None, Some(0));

        // A single failing sample that is never repeated.
        register.observe(false, ts(1_000), window);

        // It reads raw-failing during the window, but `failing_for` is never simultaneously true with
        // a still-failing state for a single-sample blip, so it never fires an alert.
        for offset in [0, 60, 120, 240, 299, 300, 301, 600] {
            let now = ts(1_000) + chrono::Duration::seconds(offset);
            assert!(!register.failing_for(now, window), "blip must not trip at +{offset}s");
        }
    }

    #[test]
    fn test_transient_subset_failure_recovers_on_its_own() {
        // One node sees a single failing sample; nobody declares a recovery.
        let window = win().num_seconds();
        let mut register = streak(None, None, Some(1_000));
        register.observe(false, ts(20_000), win());

        // Other nodes' passing samples don't mask the failure...
        register.observe(true, ts(20_030), win());
        assert!(register.failing_at(ts(20_030), win()));

        // ...but once the window passes without further failures, the probe recovers,
        // passing since the failing observation.
        assert!(register.passing_at(ts(20_000 + window + 1), win()));
        assert_eq!(register.since_at(ts(20_000 + window + 1), win()), Some(ts(20_000)));
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
        assert!(ab.failing_at(ts(50_100), win()));
        assert_eq!(ab.failing_since, Some(ts(50_000)), "the failure onset converges");

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
