use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::Mergeable;

/// The current pass/fail state of a probe as reported by a single observer, along with
/// how far back that observer can attest the state has held.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeStatus {
    /// Whether the most recent sample was successful.
    pub passing: bool,

    /// When the probe entered this state, as far back as the reporting observer can
    /// attest. Coverage is continuous from `since` to `updated`: gaps in sampling
    /// (process restarts, paused probes) reset this to the first sample after the gap.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub since: DateTime<Utc>,

    /// Whether `since` marks an observed transition from the opposite state. When false,
    /// `since` is merely the start of this observer's coverage and the state before it
    /// is unknown — aggregation may repair it using other observers' coverage.
    #[serde(default)]
    pub transition: bool,

    /// When this status was last confirmed by a sample. Statuses which haven't been
    /// confirmed recently belong to observers that have stopped reporting and should be
    /// ignored when aggregating.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub updated: DateTime<Utc>,
}

impl ProbeStatus {
    /// Begins tracking state from a first sample; the state before it is unknown.
    pub fn from_sample(passing: bool, time: DateTime<Utc>) -> Self {
        Self {
            passing,
            since: time,
            transition: false,
            updated: time,
        }
    }

    /// Records a subsequent sample. Samples further than `max_gap` apart break coverage:
    /// the time in between is treated as unknown and the streak restarts at this sample,
    /// even if the state matches.
    pub fn record(&mut self, passing: bool, time: DateTime<Utc>, max_gap: chrono::Duration) {
        if time < self.updated {
            return;
        }

        if time - self.updated > max_gap {
            *self = Self::from_sample(passing, time);
        } else if passing != self.passing {
            *self = Self {
                passing,
                since: time,
                transition: true,
                updated: time,
            };
        } else {
            self.updated = time;
        }
    }
}

impl Mergeable for ProbeStatus {
    /// Combines two observers' views of the same probe: a current failure always wins
    /// over a passing report, while passing reports extend each other's coverage — the
    /// merged streak reaches back to the earliest attested start, but never past the
    /// most recent observed failure.
    fn merge(&mut self, other: &Self) {
        match (self.passing, other.passing) {
            // Both failing: the failure has been ongoing since the earliest report of it.
            (false, false) => {
                if other.since < self.since {
                    self.since = other.since;
                    self.transition = other.transition;
                }
            }

            // Defer to failures: a passing report cannot mask a concurrent failure.
            (false, true) => {}
            (true, false) => {
                self.passing = false;
                self.since = other.since;
                self.transition = other.transition;
            }

            (true, true) => {
                let (coverage, coverage_transition) = if other.since < self.since {
                    (other.since, other.transition)
                } else {
                    (self.since, self.transition)
                };

                // A `transition` marks a failure observed immediately before `since`, so
                // the merged streak cannot reach past the latest such bound.
                let failure_bound = [(self.transition, self.since), (other.transition, other.since)]
                    .into_iter()
                    .filter(|(transition, _)| *transition)
                    .map(|(_, since)| since)
                    .max();

                match failure_bound {
                    Some(bound) if bound > coverage => {
                        self.since = bound;
                        self.transition = true;
                    }
                    _ => {
                        self.since = coverage;
                        self.transition = coverage_transition;
                    }
                }
            }
        }

        self.updated = self.updated.max(other.updated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn gap() -> chrono::Duration {
        chrono::Duration::seconds(120)
    }

    #[test]
    fn test_record_tracks_transitions() {
        let mut status = ProbeStatus::from_sample(true, ts(100));
        assert!(status.passing);
        assert_eq!(status.since, ts(100));
        assert!(!status.transition);

        // Repeating the same state keeps the streak and refreshes the confirmation time.
        status.record(true, ts(160), gap());
        assert_eq!(status.since, ts(100));
        assert_eq!(status.updated, ts(160));

        // A flip moves the streak forward and marks an observed transition.
        status.record(false, ts(220), gap());
        assert!(!status.passing);
        assert_eq!(status.since, ts(220));
        assert!(status.transition);

        // An out-of-order older sample is ignored.
        status.record(true, ts(200), gap());
        assert!(!status.passing);
        assert_eq!(status.since, ts(220));
    }

    #[test]
    fn test_record_gap_resets_coverage() {
        let mut status = ProbeStatus::from_sample(true, ts(100));

        // A sample after a coverage gap restarts the streak even though the state
        // matches: the time in between is unknown.
        status.record(true, ts(100 + 3600), gap());
        assert!(status.passing);
        assert_eq!(status.since, ts(100 + 3600));
        assert!(!status.transition);
    }

    #[test]
    fn test_merge_passing_repairs_unknown_coverage() {
        // One observer restarted recently; another has watched the probe pass for far
        // longer. The merged streak reaches back to the longest attested coverage.
        let mut restarted = ProbeStatus::from_sample(true, ts(9_000));
        let continuous = ProbeStatus::from_sample(true, ts(1_000));

        restarted.merge(&continuous);
        assert!(restarted.passing);
        assert_eq!(restarted.since, ts(1_000));
        assert!(!restarted.transition);
    }

    #[test]
    fn test_merge_passing_bounded_by_observed_failure() {
        // One observer saw the probe recover from a real failure; another observer's
        // coverage claims passing from before that. The failure bounds the streak.
        let recovered = ProbeStatus { passing: true, since: ts(8_000), transition: true, updated: ts(9_000) };
        let mut covering = ProbeStatus::from_sample(true, ts(1_000));

        covering.merge(&recovered);
        assert!(covering.passing);
        assert_eq!(covering.since, ts(8_000));
        assert!(covering.transition);
    }

    #[test]
    fn test_merge_defers_to_failures() {
        let mut passing = ProbeStatus { passing: true, since: ts(1_000), transition: false, updated: ts(9_000) };
        let failing = ProbeStatus { passing: false, since: ts(8_000), transition: true, updated: ts(8_500) };

        passing.merge(&failing);
        assert!(!passing.passing);
        assert_eq!(passing.since, ts(8_000));

        // ...and in the other merge order too.
        let mut failing = ProbeStatus { passing: false, since: ts(8_000), transition: true, updated: ts(8_500) };
        failing.merge(&ProbeStatus { passing: true, since: ts(1_000), transition: false, updated: ts(9_000) });
        assert!(!failing.passing);
        assert_eq!(failing.since, ts(8_000));
    }

    #[test]
    fn test_merge_failing_uses_earliest_report() {
        let mut failing = ProbeStatus { passing: false, since: ts(8_000), transition: true, updated: ts(9_000) };
        failing.merge(&ProbeStatus { passing: false, since: ts(5_000), transition: false, updated: ts(8_500) });

        assert!(!failing.passing);
        assert_eq!(failing.since, ts(5_000));
        assert!(!failing.transition);
        assert_eq!(failing.updated, ts(9_000));
    }

    #[test]
    fn test_merge_is_order_independent() {
        // Folding the same statuses in any order yields the same passing/since pair.
        let statuses = [
            ProbeStatus { passing: true, since: ts(7_000), transition: true, updated: ts(9_000) },
            ProbeStatus { passing: true, since: ts(1_000), transition: false, updated: ts(9_100) },
            ProbeStatus { passing: true, since: ts(4_000), transition: false, updated: ts(9_200) },
        ];

        let fold = |order: &[usize]| {
            let mut acc = statuses[order[0]].clone();
            for &i in &order[1..] {
                acc.merge(&statuses[i]);
            }
            acc
        };

        for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [2, 0, 1]] {
            let merged = fold(&order);
            assert!(merged.passing);
            assert_eq!(merged.since, ts(7_000), "order {order:?}");
            assert!(merged.transition, "order {order:?}");
            assert_eq!(merged.updated, ts(9_200), "order {order:?}");
        }
    }

    #[test]
    fn test_msgpack_roundtrip() {
        let status = ProbeStatus { passing: true, since: ts(1_000), transition: true, updated: ts(2_000) };

        let packed = rmp_serde::to_vec(&status).unwrap();
        let unpacked: ProbeStatus = rmp_serde::from_slice(&packed).unwrap();
        assert_eq!(status, unpacked);

        let packed = rmp_serde::to_vec_named(&status).unwrap();
        let unpacked: ProbeStatus = rmp_serde::from_slice(&packed).unwrap();
        assert_eq!(status, unpacked);
    }
}
