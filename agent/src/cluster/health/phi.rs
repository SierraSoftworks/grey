use std::time::{Duration, Instant};

use crate::cluster::helpers::WindowedAggregation;

/// A phi-accrual failure detector (Hayashibara et al.), in the simplified form popularised by
/// quickwit/chitchat: phi is the ratio of the time elapsed since the last observed heartbeat to the
/// mean inter-arrival interval of recent heartbeats. A higher phi means greater confidence that the
/// peer has failed; a fixed `phi_threshold` (default 8) separates healthy from suspect.
///
/// The detector is fed by *observed heartbeat advances* — every time anti-entropy reveals that a
/// peer's gossiped heartbeat counter has increased — rather than by direct contact, so it works even
/// when a peer's liveness is learned indirectly through other members.
#[derive(Debug, Clone)]
pub struct PhiAccrualDetector {
    /// Recent inter-arrival intervals (in milliseconds) between observed heartbeat advances.
    intervals: WindowedAggregation,
    /// Prior mean interval (milliseconds) used to seed the estimate before enough samples have
    /// accrued, preventing a cold-start false positive.
    prior_mean_ms: f64,
    /// When we last observed this peer's heartbeat advance.
    last_arrival: Option<Instant>,
}

impl PhiAccrualDetector {
    pub fn new(window: usize, prior_mean: Duration) -> Self {
        Self {
            intervals: WindowedAggregation::new(window),
            prior_mean_ms: (prior_mean.as_secs_f64() * 1000.0).max(1.0),
            last_arrival: None,
        }
    }

    /// Records that we observed the peer's heartbeat advance at `now`.
    pub fn report(&mut self, now: Instant) {
        if let Some(last) = self.last_arrival {
            let interval = now.saturating_duration_since(last).as_secs_f64() * 1000.0;
            if interval > 0.0 {
                self.intervals.push(interval);
            }
        }
        self.last_arrival = Some(now);
    }

    /// The mean inter-arrival interval (milliseconds), smoothed with the prior so that a small
    /// number of samples cannot produce a wildly optimistic or pessimistic estimate.
    fn mean_ms(&self) -> f64 {
        (self.intervals.sum() + self.prior_mean_ms) / (self.intervals.len() as f64 + 1.0)
    }

    /// The current phi value at `now`. Returns 0 when we have never observed a heartbeat (so a peer
    /// we have only just learned about is never immediately declared dead).
    pub fn phi(&self, now: Instant) -> f64 {
        match self.last_arrival {
            Some(last) => {
                let elapsed = now.saturating_duration_since(last).as_secs_f64() * 1000.0;
                elapsed / self.mean_ms().max(1.0)
            }
            None => 0.0,
        }
    }

    /// When we last observed a heartbeat advance, if ever.
    pub fn last_arrival(&self) -> Option<Instant> {
        self.last_arrival
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn phi_is_zero_before_any_heartbeat() {
        let det = PhiAccrualDetector::new(100, ms(1000));
        let now = Instant::now();
        assert_eq!(det.phi(now), 0.0);
    }

    #[test]
    fn phi_grows_with_elapsed_time_relative_to_mean() {
        let base = Instant::now();
        let mut det = PhiAccrualDetector::new(100, ms(1000));
        // Regular 1s heartbeats establish a ~1s mean interval.
        det.report(base);
        det.report(base + ms(1000));
        det.report(base + ms(2000));

        // One mean-interval of silence ⇒ phi ≈ 1; eight ⇒ phi ≈ 8 (the default suspicion threshold).
        let phi_1s = det.phi(base + ms(3000));
        let phi_8s = det.phi(base + ms(10_000));
        assert!((phi_1s - 1.0).abs() < 0.2, "phi after ~1 mean interval should be ~1, got {phi_1s}");
        assert!(phi_8s >= 8.0, "phi after ~8 mean intervals should reach the threshold, got {phi_8s}");
    }

    #[test]
    fn faster_heartbeats_make_the_detector_more_sensitive() {
        let base = Instant::now();
        let mut fast = PhiAccrualDetector::new(100, ms(100));
        for i in 0..5 {
            fast.report(base + ms(i * 100));
        }
        // With a ~100ms mean, a full second of silence is ~10 mean intervals ⇒ well past threshold.
        assert!(fast.phi(base + ms(400 + 1000)) > 8.0);
    }
}
