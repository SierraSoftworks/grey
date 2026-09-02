use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Mergeable, Streak};

/// One observer's (one Grey node's) own view of a probe: the streak built from *its* samples alone
/// and when it last reported. Kept per observer so a pooled probe can decide its health by quorum
/// rather than by whichever node most recently saw a failure.
///
/// Each observer only ever writes its own entry, so a map of these merges as a plain union with the
/// per-key [`Streak::join`] — a join-semilattice like the streak itself, converging identically on
/// every node.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ObserverState {
    #[serde(default)]
    pub streak: Streak,

    /// When this observer last recorded a sample for the probe. Lets consumers grey out observers
    /// that have gone quiet (and drives a node's `silent` status).
    #[serde(default, with = "chrono::serde::ts_milliseconds")]
    pub last_updated: DateTime<Utc>,
}

impl Mergeable for ObserverState {
    fn merge(&mut self, other: &Self) {
        self.streak.join(&other.streak);
        self.last_updated = self.last_updated.max(other.last_updated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn merge_joins_the_streak_and_keeps_the_latest_update() {
        let mut a = ObserverState {
            streak: Streak { failing_since: Some(ts(10)), failing_until: Some(ts(20)), covered_since: Some(ts(5)) },
            last_updated: ts(20),
        };
        let b = ObserverState {
            streak: Streak { failing_since: Some(ts(10)), failing_until: Some(ts(30)), covered_since: Some(ts(1)) },
            last_updated: ts(30),
        };
        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab, ba, "merge is commutative");
        assert_eq!(ab.last_updated, ts(30));
        assert_eq!(ab.streak.failing_until, Some(ts(30)));
        assert_eq!(ab.streak.covered_since, Some(ts(1)));
        a.merge(&a.clone());
        assert_eq!(a.last_updated, ts(20), "merge is idempotent");
    }

    #[test]
    fn msgpack_roundtrip() {
        let state = ObserverState {
            streak: Streak { failing_since: Some(ts(10)), failing_until: Some(ts(20)), covered_since: None },
            last_updated: ts(20),
        };
        let packed = rmp_serde::to_vec_named(&state).unwrap();
        assert_eq!(rmp_serde::from_slice::<ObserverState>(&packed).unwrap(), state);
    }
}
