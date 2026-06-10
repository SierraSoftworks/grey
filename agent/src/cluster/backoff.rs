use std::time::Duration;

/// A strategy deciding how long to wait before retrying an address that has failed to respond.
///
/// Implementations receive the number of consecutive misses observed for the address and return the
/// delay before the next attempt, allowing different schedules (exponential, linear, jittered, ...)
/// to be swapped in without touching the membership logic.
pub trait Backoff {
    /// Returns how long to wait before the next attempt after `misses` consecutive failures.
    /// `misses == 0` means the address has not failed and must return [`Duration::ZERO`].
    fn backoff(&self, misses: u32) -> Duration;
}

/// `min(base * 2^(misses - 1), max)`, saturating: the first miss waits `base`, each further miss
/// doubles the delay, and `max` caps it so an address is never deferred past the point where its
/// member would expire from the registry.
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    base: Duration,
    max: Duration,
}

impl ExponentialBackoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self { base, max }
    }
}

impl Backoff for ExponentialBackoff {
    fn backoff(&self, misses: u32) -> Duration {
        if misses == 0 {
            return Duration::ZERO;
        }
        // Cap the exponent at 31: `1u32 << 32` overflows the shift (panicking in debug builds and
        // wrapping to `1 << 0` in release, which would silently reset the backoff to its base).
        let shift = (misses - 1).min(31);
        let scaled = self.base.saturating_mul(1u32 << shift);
        scaled.min(self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_backoff_grows_and_caps() {
        let strategy = ExponentialBackoff::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(strategy.backoff(0), Duration::ZERO);
        assert_eq!(strategy.backoff(1), Duration::from_secs(1));
        assert_eq!(strategy.backoff(2), Duration::from_secs(2));
        assert_eq!(strategy.backoff(3), Duration::from_secs(4));
        assert_eq!(strategy.backoff(30), Duration::from_secs(60), "must cap at max");
    }

    #[test]
    fn exponential_backoff_holds_at_max_for_very_high_miss_counts() {
        // Miss counts past the shift width must hold at max rather than overflowing the shift
        // (which would panic in debug builds and silently reset the backoff in release builds).
        let strategy = ExponentialBackoff::new(Duration::from_secs(1), Duration::from_secs(60));
        for misses in [32, 33, 34, 100, u32::MAX] {
            assert_eq!(
                strategy.backoff(misses),
                Duration::from_secs(60),
                "backoff({misses}) must hold at max"
            );
        }
    }
}
