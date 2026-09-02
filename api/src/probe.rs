use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Mergeable, ObserverState, ProbeHistoryBucket, Quorum, Streak};
use crate::observation::Observation;

/// Raw probe data as returned by the /api/v1/probes endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Probe {
    pub name: String,

    #[serde(default)]
    pub tags: HashMap<String, String>,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_updated: chrono::DateTime<chrono::Utc>,

    #[serde(default)]
    pub history: Vec<ProbeHistoryBucket>,

    /// Observations collected from this probe, keyed by observer ID
    #[serde(default)]
    pub observations: HashMap<String, Observation>,

    /// The cluster-converged record of this probe's pass/fail streaks
    #[serde(default)]
    pub streak: Streak,

    /// The debounce window applied to this probe's streak-derived health (a config echo of the
    /// probe's `alerting.debounce`, stamped by the agent). Applied symmetrically to both the onset of
    /// a fault and the recovery from it. `None` falls back to [`Streak::default_recovery_window`],
    /// preserving the historical 5-minute behaviour for records written before per-probe alerting
    /// existed.
    #[serde(default, with = "humantime_serde::option")]
    pub debounce: Option<std::time::Duration>,

    /// A propagating tombstone: the observing node no longer has this probe in its configuration.
    /// Retired records are excluded from the pooled view (and so from the API and UI) but keep
    /// gossiping until they age out, so removing a probe from the config removes it cluster-wide
    /// rather than leaving its history stranded on every peer.
    #[serde(default)]
    pub retired: bool,

    /// Each observer's own view of the probe (the streak built from its samples alone), keyed by
    /// node identifier. A node's own record carries only its own entry; the pooled view carries one
    /// per observer, and health is decided by quorum over them (see [`Probe::healthy_at`]).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub observers: HashMap<String, ObserverState>,

    /// The quorum of observers that must agree before the probe reads as failing (and, by
    /// symmetry, as recovered). Stamped from the local configuration, like `debounce`; absent means
    /// [`Quorum::Majority`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quorum: Option<Quorum>,
}

impl Probe {
    /// The debounce window governing this probe's streak-derived health, falling back to the default
    /// when the agent has not stamped a configured value.
    pub fn window(&self) -> chrono::Duration {
        self.debounce
            .and_then(|w| chrono::Duration::from_std(w).ok())
            .unwrap_or_else(Streak::default_recovery_window)
    }

    /// Drops history buckets that have aged out of the 48-hour retention window.
    ///
    /// This must run everywhere a record is rewritten, not just on the gossip receive path
    /// ([`Mergeable::merge`]): an observer's *own* records are only ever updated by applying its
    /// probe results, and without pruning there they grow by one bucket per hour indefinitely —
    /// every read of such a record (gossip diffs, the pooled API/UI view, the notifier) then pays
    /// for decoding the full unbounded history.
    pub fn prune_history(&mut self) {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24 * 2);
        self.history.retain(|h| h.start_time > cutoff);
    }

    /// Aggregate all observations into a single total observation
    pub fn total(&self) -> Observation {
        self.observations.values().fold(Observation::default(), |mut acc, obs| {
            acc.merge(obs);
            acc
        })
    }

    /// Calculate availability percentage based on successful vs total samples
    pub fn availability(&self) -> f64 {
        self.total().success_rate()
    }

    /// Whether this probe is currently passing (debounced by its configured window), falling back to
    /// the latest history bucket's result when the streak record carries no observations.
    /// The quorum in force for this probe.
    pub fn quorum(&self) -> Quorum {
        self.quorum.unwrap_or_default()
    }

    /// How many observers must report a failure for the probe to read as failing.
    pub fn quorum_size(&self) -> usize {
        self.quorum().required(self.observers.len())
    }

    /// The observers whose own debounced streak reads the probe as failing at `now`.
    pub fn failing_observers_at(&self, now: chrono::DateTime<chrono::Utc>, window: chrono::Duration) -> usize {
        self.observers
            .values()
            .filter(|o| o.streak.failing_for(now, window))
            .count()
    }

    /// The probe's debounced health at `now`, decided by quorum: it reads as failing only while at
    /// least [`Probe::quorum_size`] observers each report a sustained failure, and as passing (or
    /// recovered) otherwise. Observers that have gone quiet decay to passing on their own, so a
    /// node that stops reporting biases the probe towards recovery rather than blocking it.
    ///
    /// Records that predate per-observer streaks fall back to the pooled [`Streak`].
    pub fn healthy_at(&self, now: chrono::DateTime<chrono::Utc>, window: chrono::Duration) -> bool {
        if self.observers.is_empty() {
            self.streak.healthy_at(now, window)
        } else {
            self.failing_observers_at(now, window) < self.quorum_size()
        }
    }

    /// When the probe entered its current quorum-derived state, when known.
    ///
    /// While failing this is the onset seen by the quorum-th observer to start failing; while
    /// passing it is the last failing observation of the observer whose recovery brought the count
    /// back under the quorum (the quorum-th most recent `failing_until` across all observers), or
    /// the earliest coverage on record when fewer than a quorum have ever recorded a failure.
    pub fn since_at(&self, now: chrono::DateTime<chrono::Utc>, window: chrono::Duration) -> Option<chrono::DateTime<chrono::Utc>> {
        if self.observers.is_empty() {
            return self.streak.since_at(now, window);
        }

        let quorum = self.quorum_size();
        if !self.healthy_at(now, window) {
            let mut onsets: Vec<_> = self
                .observers
                .values()
                .filter(|o| o.streak.failing_for(now, window))
                .filter_map(|o| o.streak.failing_since)
                .collect();
            onsets.sort();
            return onsets.get(quorum - 1).or(onsets.last()).copied();
        }

        // Every observer's last failing observation takes part, including those still failing: the
        // count dropped below the quorum at the quorum-th most recent of them, and an observer that
        // has not recovered simply holds one of the more recent slots.
        let mut recoveries: Vec<_> = self
            .observers
            .values()
            .filter_map(|o| o.streak.failing_until)
            .collect();
        recoveries.sort_by(|a, b| b.cmp(a));
        recoveries.get(quorum - 1).copied().or_else(|| {
            self.observers
                .values()
                .filter_map(|o| o.streak.covered_since)
                .min()
        })
    }

    pub fn passing(&self) -> bool {
        if self.streak.is_empty() && self.observers.is_empty() {
            self.history.last().map(|h| h.pass).unwrap_or(true)
        } else {
            self.healthy_at(chrono::Utc::now(), self.window())
        }
    }

    pub fn since(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.since_at(chrono::Utc::now(), self.window())
    }

    pub fn status_token(&self) -> &'static str {
        if self.passing() { "passing" } else { "failing" }
    }

    /// Calculate recent availability percentage based on successful vs total samples
    pub fn recent(&self, max_hours: usize) -> Observation {
        self
            .history
            .iter()
            .filter(|h| {
                h.start_time > chrono::Utc::now() - chrono::Duration::hours(max_hours as i64)
            })
            .map(|h| h.total())
            .fold(Observation::default(), |mut acc, obs| {
                acc.merge(&obs);
                acc
            })
    }
}

impl Mergeable for Probe {
    fn merge(&mut self, other: &Self) {
        if other.last_updated > self.last_updated {
            self.name = other.name.clone();
            self.tags = other.tags.clone();
            self.retired = other.retired;
        }

        self.last_updated = self.last_updated.max(other.last_updated);
        self.observations.extend(other.observations.clone());
        self.streak.join(&other.streak);
        for (observer, state) in &other.observers {
            self.observers
                .entry(observer.clone())
                .and_modify(|mine| mine.merge(state))
                .or_insert_with(|| state.clone());
        }

        let mut i = 0;
        let mut j = 0;

        while i < self.history.len() && j < other.history.len() {
            if self.history[i].start_time == other.history[j].start_time {
                self.history[i].merge(&other.history[j]);
                i += 1;
                j += 1;
            } else if self.history[i].start_time < other.history[j].start_time {
                i += 1;
            } else {
                self.history.insert(i, other.history[j].clone());
                i += 1;
                j += 1;
            }
        }

        while j < other.history.len() {
            self.history.push(other.history[j].clone());
            j += 1;
        }

        self.prune_history();
    }
}

/// Probe policy information
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Policy {
    #[serde(with = "humantime_serde")]
    pub interval: std::time::Duration,

    #[serde(default)]
    pub retries: Option<u8>,

    #[serde(with = "humantime_serde")]
    pub timeout: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;
    use super::*;
    
    #[test]
    fn test_probe_merge() {
        let mut probe1 = Probe {
            name: "probe1".into(),
            tags: vec![("env".into(), "prod".into())].into_iter().collect(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: vec![("observer1".into(), Observation {
                total_samples: 10,
                successful_samples: 9,
                total_retries: 2,
                total_latency: std::time::Duration::from_secs(5),
            })].into_iter().collect(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers: HashMap::new(),
            quorum: None,
        };

        let probe2 = Probe {
            name: "probe2".into(),
            tags: vec![("env".into(), "staging".into())].into_iter().collect(),
            last_updated: chrono::Utc::now() + chrono::Duration::seconds(10),
            history: vec![],
            observations: vec![("observer2".into(), Observation {
                total_samples: 5,
                successful_samples: 4,
                total_retries: 1,
                total_latency: std::time::Duration::from_secs(3),
            })].into_iter().collect(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers: HashMap::new(),
            quorum: None,
        };

        probe1.merge(&probe2);

        assert_eq!(probe1.name, "probe2");
        assert_eq!(probe1.tags.get("env").unwrap(), "staging");
        assert_eq!(probe1.observations.len(), 2);
        assert_eq!(probe1.observations.get("observer1").unwrap().total_samples, 10);
        assert_eq!(probe1.observations.get("observer2").unwrap().total_samples, 5);
    }
    
    #[test]
    fn test_probe_total() {
        let probe = Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: vec![
                ("observer1".into(), Observation {
                    total_samples: 10,
                    successful_samples: 9,
                    total_retries: 2,
                    total_latency: std::time::Duration::from_secs(5),
                }),
                ("observer2".into(), Observation {
                    total_samples: 5,
                    successful_samples: 4,
                    total_retries: 1,
                    total_latency: std::time::Duration::from_secs(3),
                }),
            ].into_iter().collect(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers: HashMap::new(),
            quorum: None,
        };

        let total = probe.total();
        assert_eq!(total.total_samples, 15);
        assert_eq!(total.successful_samples, 13);
        assert_eq!(total.total_retries, 3);
        assert_eq!(total.total_latency, std::time::Duration::from_secs(8));
    }
    
    #[test]
    fn test_probe_availability() {
        let probe = Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: vec![
                ("observer1".into(), Observation {
                    total_samples: 10,
                    successful_samples: 9,
                    total_retries: 2,
                    total_latency: std::time::Duration::from_secs(5),
                }),
                ("observer2".into(), Observation {
                    total_samples: 5,
                    successful_samples: 4,
                    total_retries: 1,
                    total_latency: std::time::Duration::from_secs(3),
                }),
            ].into_iter().collect(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers: HashMap::new(),
            quorum: None,
        };

        let availability = probe.availability();
        assert_eq!(availability, (13.0 / 15.0) * 100.0);
    }
    
    #[test]
    fn test_probe_passing() {
        let now = chrono::Utc::now();
        let mut probe = Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: now,
            history: vec![],
            observations: HashMap::new(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers: HashMap::new(),
            quorum: None,
        };

        // With an empty streak record (e.g. data from older agents), the probe falls
        // back to the latest history bucket's result.
        assert!(probe.streak.is_empty());
        assert!(probe.passing());
        probe.history.push(ProbeHistoryBucket {
            start_time: now,
            pass: false,
            message: "Timeout".into(),
            validations: HashMap::new(),
            observations: HashMap::new(),
        });
        assert!(!probe.passing());

        // A streak record with a long-standing coverage claim reports passing.
        probe.streak.observe(true, now - chrono::Duration::days(3), Streak::default_recovery_window());
        assert!(probe.streak.passing_at(now, Streak::default_recovery_window()));
        assert_eq!(probe.streak.since_at(now, Streak::default_recovery_window()), Some(now - chrono::Duration::days(3)));
        assert!(probe.passing());
    }

    /// The streak-derived health is debounced by the window: a fault reads failing only once it has
    /// persisted for the whole window, and reads passing again a window after the last failure. This
    /// is evaluated at explicit instants (unlike [`Probe::passing`], which reads the wall clock).
    #[test]
    fn test_probe_health_is_debounced() {
        let window = Streak::default_recovery_window();
        let base = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        // A continuous fault: onset at `base`, still failing at `base + window`.
        let streak = Streak {
            failing_since: Some(base),
            failing_until: Some(base + window),
            covered_since: None,
        };

        // The onset is debounced away until the fault is a full window old.
        assert!(streak.healthy_at(base, window), "onset must be debounced");
        assert!(streak.healthy_at(base + window - chrono::Duration::seconds(1), window));
        assert!(!streak.healthy_at(base + window, window), "a sustained fault trips at the window");
        assert_eq!(streak.since_at(base + window, window), Some(base));

        // Recovery is likewise debounced: still failing until a window after the last failure.
        let last_fail = base + window;
        assert!(!streak.healthy_at(last_fail + window - chrono::Duration::seconds(1), window));
        assert!(streak.healthy_at(last_fail + window + chrono::Duration::seconds(1), window));
    }

    fn observer(streak: Streak, last_updated: chrono::DateTime<chrono::Utc>) -> crate::ObserverState {
        crate::ObserverState { streak, last_updated }
    }

    /// A probe with one observer per entry of `views`: `Some(onset)` for an observer that has been
    /// failing continuously since `now - onset`, `None` for one that only ever passed.
    fn quorum_probe(now: chrono::DateTime<chrono::Utc>, quorum: Option<Quorum>, views: &[Option<chrono::Duration>]) -> Probe {
        let observers = views
            .iter()
            .enumerate()
            .map(|(i, view)| {
                let streak = match view {
                    Some(onset) => Streak { failing_since: Some(now - *onset), failing_until: Some(now), covered_since: None },
                    None => Streak { failing_since: None, failing_until: None, covered_since: Some(now - chrono::Duration::days(1)) },
                };
                (format!("node-{i}"), observer(streak, now))
            })
            .collect();
        Probe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: now,
            history: vec![],
            observations: HashMap::new(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers,
            quorum,
        }
    }

    #[test]
    fn health_is_decided_by_a_quorum_of_observers() {
        let now = chrono::Utc::now();
        let window = Streak::default_recovery_window();
        let sustained = Some(window * 2);
        let cases: &[(&str, Option<Quorum>, &[Option<chrono::Duration>], bool)] = &[
            ("single observer failing is its own majority", None, &[sustained], false),
            ("single observer passing", None, &[None], true),
            ("one of three failing is tolerated", None, &[sustained, None, None], true),
            ("two of three failing trips the majority", None, &[sustained, sustained, None], false),
            ("an even split reads passing", None, &[sustained, sustained, None, None], true),
            ("a count quorum of one alerts on any observer", Some(Quorum::Count(1)), &[sustained, None, None], false),
            ("a percentage quorum", Some(Quorum::Percent(100)), &[sustained, sustained, None], true),
            ("an unconfirmed fault does not count", None, &[Some(chrono::Duration::seconds(10)), Some(chrono::Duration::seconds(10))], true),
        ];
        for (name, quorum, views, expect_healthy) in cases {
            let probe = quorum_probe(now, *quorum, views);
            assert_eq!(probe.healthy_at(now, window), *expect_healthy, "{name}");
            assert_eq!(probe.passing(), *expect_healthy, "{name} (passing)");
        }
    }

    #[test]
    fn since_follows_the_quorum() {
        let now = chrono::Utc::now();
        let window = Streak::default_recovery_window();
        let early = chrono::Duration::hours(2);
        let late = chrono::Duration::hours(1);

        // Failing: the onset is the moment the quorum-th observer started failing.
        let probe = quorum_probe(now, None, &[Some(early), Some(late), None]);
        assert!(!probe.healthy_at(now, window));
        assert_eq!(probe.since_at(now, window), Some(now - late));

        // Recovered: "passing since" is the last failing sample of the observer whose recovery took
        // the count back under the quorum (the earlier of the two recoveries).
        let mut probe = quorum_probe(now, None, &[None, None, None]);
        for (id, ended) in [("node-0", 20), ("node-1", 40)] {
            let ended = now - chrono::Duration::minutes(ended);
            probe.observers.get_mut(id).unwrap().streak = Streak {
                failing_since: Some(ended - chrono::Duration::hours(1)),
                failing_until: Some(ended),
                covered_since: None,
            };
        }
        assert!(probe.healthy_at(now, window));
        assert_eq!(probe.since_at(now, window), Some(now - chrono::Duration::minutes(40)));

        // A partial recovery: one observer is still failing, but the other's recovery took the count
        // under the quorum, so "passing since" is that recovery rather than the coverage fallback.
        let mut probe = quorum_probe(now, None, &[Some(chrono::Duration::hours(1)), None, None]);
        let ended = now - chrono::Duration::minutes(40);
        probe.observers.get_mut("node-1").unwrap().streak = Streak {
            failing_since: Some(ended - chrono::Duration::hours(1)),
            failing_until: Some(ended),
            covered_since: None,
        };
        assert!(probe.healthy_at(now, window));
        assert_eq!(probe.since_at(now, window), Some(ended));

        // Never failed as a quorum: covered since the earliest observation.
        let probe = quorum_probe(now, None, &[None, None, None]);
        assert_eq!(probe.since_at(now, window), Some(now - chrono::Duration::days(1)));
        let probe = quorum_probe(now, None, &[Some(chrono::Duration::hours(1)), None, None]);
        assert_eq!(probe.since_at(now, window), Some(now - chrono::Duration::days(1)), "one failing observer of three has never been a quorum");
    }

    #[test]
    fn records_without_observers_fall_back_to_the_pooled_streak() {
        let now = chrono::Utc::now();
        let window = Streak::default_recovery_window();
        let mut probe = quorum_probe(now, None, &[]);
        probe.streak = Streak { failing_since: Some(now - window * 2), failing_until: Some(now), covered_since: None };
        assert!(!probe.healthy_at(now, window));
        assert_eq!(probe.since_at(now, window), Some(now - window * 2));
    }

    #[test]
    fn merge_unions_observers() {
        let now = chrono::Utc::now();
        let window = Streak::default_recovery_window();
        let mut a = quorum_probe(now, None, &[Some(window * 2)]);
        let mut b = quorum_probe(now, None, &[None, None]);
        b.observers.remove("node-0");
        let mut c = quorum_probe(now, None, &[None, None, None]);
        c.observers.retain(|k, _| k == "node-2");

        let mut ab = a.clone();
        ab.merge(&b);
        ab.merge(&c);
        let mut cba = c.clone();
        cba.merge(&b);
        cba.merge(&a);
        assert_eq!(ab.observers, cba.observers, "the observer map converges regardless of merge order");
        assert_eq!(ab.observers.len(), 3);
        assert!(ab.healthy_at(now, window), "one failing observer of three is below the majority");

        a.merge(&a.clone());
        assert_eq!(a.observers.len(), 1, "merge is idempotent");
    }

    #[test]
    fn test_msgpack_roundtrip() {
        let probe = Probe {
            name: "probe".into(),
            tags: vec![("env".into(), "prod".into())].into_iter().collect(),
            last_updated: chrono::Utc::now().with_time(NaiveTime::from_hms_micro_opt(1, 2, 3, 0).unwrap()).unwrap(),
            history: vec![],
            observations: vec![("observer1".into(), Observation {
                total_samples: 10,
                successful_samples: 9,
                total_retries: 2,
                total_latency: std::time::Duration::from_secs(5),
            })].into_iter().collect(),
            streak: Streak {
                failing_since: Some(chrono::DateTime::from_timestamp(1_699_999_000, 0).unwrap()),
                failing_until: Some(chrono::DateTime::from_timestamp(1_699_999_900, 0).unwrap()),
                covered_since: Some(chrono::DateTime::from_timestamp(1_690_000_000, 0).unwrap()),
            },
            debounce: None,
            retired: false,
            observers: vec![("observer1".into(), crate::ObserverState {
                streak: Streak {
                    failing_since: Some(chrono::DateTime::from_timestamp(1_699_999_000, 0).unwrap()),
                    failing_until: Some(chrono::DateTime::from_timestamp(1_699_999_900, 0).unwrap()),
                    covered_since: None,
                },
                last_updated: chrono::DateTime::from_timestamp(1_699_999_900, 0).unwrap(),
            })].into_iter().collect(),
            quorum: Some(Quorum::Percent(60)),
        };

        let packed = rmp_serde::to_vec(&probe).unwrap();
        let unpacked: Probe = rmp_serde::from_slice(&packed).unwrap();
        assert_eq!(probe, unpacked);
    }

    #[test]
    fn test_decodes_legacy_probes() {
        // Probe records stored or gossiped by agents which pre-date streak tracking lack
        // the streak register; they must decode with an empty one in both wire formats.
        #[derive(Serialize)]
        struct LegacyProbe {
            name: String,
            tags: HashMap<String, String>,
            #[serde(with = "chrono::serde::ts_seconds")]
            last_updated: chrono::DateTime<chrono::Utc>,
            history: Vec<ProbeHistoryBucket>,
            observations: HashMap<String, Observation>,
        }

        let legacy = LegacyProbe {
            name: "probe".into(),
            tags: HashMap::new(),
            last_updated: chrono::Utc::now().with_time(NaiveTime::from_hms_opt(1, 2, 3).unwrap()).unwrap(),
            history: vec![],
            observations: vec![("observer1".into(), Observation {
                total_samples: 10,
                successful_samples: 9,
                total_retries: 2,
                total_latency: std::time::Duration::from_secs(5),
            })].into_iter().collect(),
        };

        for packed in [rmp_serde::to_vec(&legacy).unwrap(), rmp_serde::to_vec_named(&legacy).unwrap()] {
            let unpacked: Probe = rmp_serde::from_slice(&packed).unwrap();
            assert_eq!(unpacked.name, "probe");
            assert_eq!(unpacked.observations.len(), 1);
            assert!(unpacked.streak.is_empty());
            assert!(unpacked.streak.is_empty());
        }
    }
}