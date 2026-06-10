use std::collections::{HashSet, VecDeque};
use std::hash::Hash;

/// In-place Fisher–Yates shuffle, providing a cheap, unbiased reorder for sampling.
pub fn shuffle<T>(items: &mut [T]) {
    use rand::RngExt;
    let mut rng = rand::rng();
    let len = items.len();
    for i in (1..len).rev() {
        let j = rng.random_range(0..=i);
        items.swap(i, j);
    }
}

/// Randomly selects up to `count` items from `items` without replacement.
///
/// Uses a partial Fisher–Yates shuffle so it touches only `count` elements regardless of the
/// input size. Returns all items when `count` exceeds their number.
pub fn sample_peers<A>(mut items: Vec<A>, count: usize) -> Vec<A> {
    use rand::RngExt;

    let take = count.min(items.len());
    let mut rng = rand::rng();
    for i in 0..take {
        let j = i + rng.random_range(0..(items.len() - i));
        items.swap(i, j);
    }
    items.truncate(take);
    items
}

/// Removes duplicate targets that share an address, keeping the first occurrence. Used so a seed
/// that is also a discovered peer (or two candidates resolving to the same address) is only gossiped
/// to once per round.
pub fn unique_by_address<I, A: Clone + Eq + Hash>(items: Vec<(I, A)>) -> Vec<(I, A)> {
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(items.len());
    for (id, addr) in items {
        if seen.insert(addr.clone()) {
            result.push((id, addr));
        }
    }
    result
}

/// A bounded window of samples with a running sum, so that `sum()`, `len()`, and `avg()` are all
/// `O(1)` (at `O(N)` space) rather than re-reducing the window on every read.
///
/// The running sum drifts from the true sum by at most a few ULPs per push/pop pair, which is
/// irrelevant at the precision the failure detector needs.
#[derive(Debug, Clone)]
pub struct WindowedAggregation {
    window: usize,
    values: VecDeque<f64>,
    sum: f64,
}

impl WindowedAggregation {
    pub fn new(window: usize) -> Self {
        let window = window.max(1);
        Self {
            window,
            values: VecDeque::with_capacity(window.min(1024)),
            sum: 0.0,
        }
    }

    /// Appends a sample, evicting the oldest one once the window is full.
    pub fn push(&mut self, value: f64) {
        if self.values.len() >= self.window
            && let Some(evicted) = self.values.pop_front()
        {
            self.sum -= evicted;
        }
        self.values.push_back(value);
        self.sum += value;
    }

    pub fn sum(&self) -> f64 {
        self.sum
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The mean of the windowed samples, or 0 when no samples have been recorded.
    #[allow(dead_code)]
    pub fn avg(&self) -> f64 {
        if self.values.is_empty() {
            0.0
        } else {
            self.sum / self.values.len() as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_by_address_keeps_first_occurrence_per_address() {
        // A seed (address "a") that is also a discovered peer, and a duplicate seed ("b"), must each
        // be gossiped to only once per round even though they are not adjacent.
        let input = vec![
            (Some(1), "a"),
            (None, "b"),
            (Some(2), "a"),
            (None, "c"),
            (None, "b"),
        ];
        assert_eq!(
            unique_by_address(input),
            vec![(Some(1), "a"), (None, "b"), (None, "c")]
        );
    }

    #[test]
    fn sample_peers_limits_to_count_and_returns_distinct_subset() {
        let all: Vec<u32> = (0..10).collect();
        for _ in 0..100 {
            let sampled = sample_peers(all.clone(), 3);
            assert_eq!(sampled.len(), 3, "should sample exactly gossip_factor peers");
            assert!(sampled.iter().all(|x| all.contains(x)), "samples must come from the input");
            let distinct: HashSet<_> = sampled.iter().collect();
            assert_eq!(distinct.len(), sampled.len(), "samples must be without replacement");
        }
    }

    #[test]
    fn sample_peers_caps_at_available() {
        assert_eq!(sample_peers(vec![1, 2], 5).len(), 2);
        assert!(sample_peers(Vec::<u32>::new(), 5).is_empty());
    }

    #[test]
    fn sample_peers_eventually_covers_all_candidates() {
        // Sampling must rotate across rounds so anti-entropy reaches every peer over time.
        let all: Vec<u32> = (0..5).collect();
        let mut seen: HashSet<u32> = HashSet::new();
        for _ in 0..1000 {
            seen.extend(sample_peers(all.clone(), 1));
        }
        assert_eq!(seen.len(), all.len(), "every candidate should be reachable across rounds");
    }

    #[test]
    fn shuffle_preserves_elements() {
        let mut items: Vec<u32> = (0..16).collect();
        shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..16).collect::<Vec<_>>());
    }

    #[test]
    fn windowed_aggregation_tracks_sum_len_and_avg() {
        let mut agg = WindowedAggregation::new(3);
        assert!(agg.is_empty());
        assert_eq!(agg.avg(), 0.0, "an empty window has a zero average");

        agg.push(1.0);
        agg.push(2.0);
        agg.push(3.0);
        assert_eq!((agg.sum(), agg.len(), agg.avg()), (6.0, 3, 2.0));

        // Pushing past the window evicts the oldest sample (1.0) from the running sum.
        agg.push(7.0);
        assert_eq!((agg.sum(), agg.len(), agg.avg()), (12.0, 3, 4.0));
    }
}
