use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, RwLock,
};
use tokio::time::Instant;

use crate::result::{ProbeResult, ValidationResult};

#[derive(Clone)]
pub struct HistoryProvider<const N: usize = 10> {
    probe_histories: Arc<RwLock<HashMap<String, Arc<ProbeHistory<N>>>>>,
}

impl<const N: usize> HistoryProvider<N> {
    /// Create a new history provider
    pub fn new() -> Self {
        Self {
            probe_histories: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get<K: ToString>(&self, probe: K) -> Option<Arc<ProbeHistory<N>>> {
        self.probe_histories
            .read()
            .unwrap()
            .get(&probe.to_string())
            .cloned()
    }

    pub fn init<K: ToString>(&self, probe: K, history: Arc<ProbeHistory<N>>) {
        self.probe_histories
            .write()
            .unwrap()
            .entry(probe.to_string())
            .or_insert(history);
    }
}

/// Represents a state that the probe can be in
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeState {
    pub pass: bool,
    pub message: String,
    pub validations: HashMap<String, ValidationResult>,
}

impl ProbeState {
    /// Create a probe state from a probe result
    pub fn from_result(result: &ProbeResult) -> Self {
        Self {
            pass: result.pass,
            message: result.message.clone(),
            validations: result.validations.clone(),
        }
    }
}

/// Represents an aggregated state bucket with timing and success information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateBucket {
    /// The state this bucket represents
    pub state: ProbeState,
    /// When this state period started
    pub start_time: DateTime<Utc>,
    /// When this state period ended (None if it's the current state)
    pub end_time: Option<DateTime<Utc>>,
    /// Total number of attempts in this state
    pub total_attempts: u64,
    /// Total duration of all samples in this state
    #[serde(with = "crate::serializers::chrono_duration_humantime")]
    pub total_latency: Duration,
    /// Number of successful samples in this state bucket
    pub successful_samples: u64,
    /// Total number of samples in this state bucket
    pub total_samples: u64,
}

impl StateBucket {
    /// Create a new state bucket from the first sample
    pub fn new(result: &ProbeResult) -> Self {
        Self {
            state: ProbeState::from_result(result),
            start_time: result.start_time,
            end_time: None,
            total_attempts: result.attempts as u64,
            total_latency: result.duration,
            successful_samples: if result.pass { 1 } else { 0 },
            total_samples: 1,
        }
    }

    /// Add a sample to this state bucket
    pub fn add_sample(&mut self, result: &ProbeResult) {
        self.total_latency += result.duration;
        self.total_samples += 1;
        self.total_attempts += result.attempts as u64;
        if result.pass {
            self.successful_samples += 1;
        }
    }

    /// Finalize this state bucket when transitioning to a new state
    pub fn finalize(&mut self, end_time: DateTime<Utc>) {
        self.end_time = Some(end_time);
    }

    /// Get the duration of this state period
    pub fn duration(&self) -> Duration {
        match self.end_time {
            Some(end) => end - self.start_time,
            _ => Utc::now() - self.start_time,
        }
    }

    /// Get the availability percentage for this state bucket
    pub fn availability(&self) -> f64 {
        if self.total_samples == 0 {
            100.0
        } else {
            100.0 * self.successful_samples as f64 / self.total_samples as f64
        }
    }
}

/// A history manager that tracks probe results using state-based aggregation
#[derive(Debug)]
pub struct ProbeHistory<const MAX_STATES: usize> {
    /// Total number of samples recorded
    sample_count_total: AtomicU64,
    /// Number of healthy samples recorded
    sample_count_healthy: AtomicU64,
    /// State buckets using circular buffer for state transitions
    state_buckets: std::sync::RwLock<circular_buffer::CircularBuffer<MAX_STATES, StateBucket>>,
    /// Maximum age for a state bucket before it's forced to finalize
    max_state_age: Duration,
    /// The minimum amount of time between snapshots
    snapshot_interval: Option<Duration>,
    /// Optional path for snapshot file
    snapshot_file: Option<PathBuf>,
    /// Last time a snapshot was written to disk
    last_snapshot_time: Arc<RwLock<Option<Instant>>>,
}

/// Serializable representation of probe history for disk snapshots
#[derive(Debug, Serialize, Deserialize)]
struct ProbeHistorySnapshot {
    sample_count_total: u64,
    sample_count_healthy: u64,
    state_buckets: Vec<StateBucket>,
    #[serde(with = "crate::serializers::chrono_duration_humantime")]
    max_state_age: Duration,
}

impl<const MAX_STATES: usize> Default for ProbeHistory<MAX_STATES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_STATES: usize> ProbeHistory<MAX_STATES> {
    /// Creates a new probe history with default settings (1 hour max state age)
    pub fn new() -> Self {
        Self {
            sample_count_total: AtomicU64::new(0),
            sample_count_healthy: AtomicU64::new(0),
            state_buckets: std::sync::RwLock::new(circular_buffer::CircularBuffer::new()),
            max_state_age: Duration::hours(1),
            snapshot_file: None,
            snapshot_interval: None,
            last_snapshot_time: Arc::new(RwLock::new(None)),
        }
    }

    /// Creates a new probe history with the specified maximum state age
    pub fn with_max_state_age(self, max_state_age: Duration) -> Self {
        Self {
            max_state_age,
            ..self
        }
    }

    pub fn with_snapshot_interval(self, interval: Duration) -> Self {
        Self {
            snapshot_interval: Some(interval),
            ..self
        }
    }

    /// Creates a new probe history with snapshotting enabled
    pub fn with_snapshot_file<P: Into<PathBuf>>(self, path: P) -> std::io::Result<Self> {
        let snapshot_file = path.into();

        // Try to load existing snapshot
        let snapshot = if snapshot_file.exists() {
            let content = std::fs::read_to_string(&snapshot_file)?;
            serde_json::from_str::<ProbeHistorySnapshot>(&content).ok()
        } else {
            None
        };

        let mut history = Self {
            snapshot_file: Some(snapshot_file),
            last_snapshot_time: Arc::new(RwLock::new(None)),
            ..self
        };

        // Restore from snapshot if available
        if let Some(snapshot) = snapshot {
            history
                .sample_count_total
                .store(snapshot.sample_count_total, Ordering::Relaxed);
            history
                .sample_count_healthy
                .store(snapshot.sample_count_healthy, Ordering::Relaxed);
            history.max_state_age = snapshot.max_state_age;

            let mut buckets = history.state_buckets.write().unwrap();
            for bucket in snapshot.state_buckets {
                buckets.push_back(bucket);
            }
        }

        Ok(history)
    }

    /// Calculate the aligned start time for a state based on the max age
    fn align_start_time(&self, timestamp: DateTime<Utc>) -> DateTime<Utc> {
        let age_seconds = self.max_state_age.num_seconds();

        // For very short durations (< 1 second), don't align
        if age_seconds < 1 {
            return timestamp;
        }

        // Calculate Unix timestamp and align to the max_state_age boundary
        let unix_timestamp = timestamp.timestamp();
        let aligned_timestamp = (unix_timestamp / age_seconds) * age_seconds;

        // Convert back to DateTime
        DateTime::from_timestamp(aligned_timestamp, 0).unwrap_or(timestamp)
    }

    /// Check if a state bucket should be finalized due to age
    fn should_finalize_for_age(&self, bucket: &StateBucket, new_timestamp: DateTime<Utc>) -> bool {
        let aligned_start = self.align_start_time(bucket.start_time);
        let max_end_time = aligned_start + self.max_state_age;
        new_timestamp >= max_end_time
    }

    /// Adds a new probe result to the history
    pub fn add_sample(&self, result: ProbeResult) {
        self.sample_count_total.fetch_add(1, Ordering::Relaxed);
        if result.pass {
            self.sample_count_healthy.fetch_add(1, Ordering::Relaxed);
        }

        let new_state = ProbeState::from_result(&result);
        {
            let mut buckets = self.state_buckets.write().unwrap();

            // Check if we can add to the current state bucket or need to create a new one
            if let Some(current_bucket) = buckets.back_mut() {
                let same_state = current_bucket.state == new_state;
                let age_exceeded = self.should_finalize_for_age(current_bucket, result.start_time);

                if same_state && !age_exceeded {
                    // Same state and within age limit, add to current bucket
                    current_bucket.add_sample(&result);
                    return;
                } else {
                    // State transition or age exceeded, finalize the current bucket
                    current_bucket.finalize(result.start_time);
                }
            }

            let new_bucket = StateBucket::new(&result);
            buckets.push_back(new_bucket);
        }

        self.maybe_trigger_snapshot();
    }

    /// Triggers a snapshot if one hasn't been written in the last 60 seconds
    fn maybe_trigger_snapshot(&self) {
        if let (Some(snapshot_file), Some(interval)) =
            (&self.snapshot_file, &self.snapshot_interval)
        {
            let now = Instant::now();
            let should_snapshot = {
                let last_snapshot = self.last_snapshot_time.read().unwrap();
                match *last_snapshot {
                    Some(last_time) => {
                        now.duration_since(last_time).as_secs() as i64 >= interval.num_seconds()
                    }
                    _ => true,
                }
            };

            if should_snapshot {
                let snapshot_file = snapshot_file.clone();
                let snapshot_data = self.create_snapshot();
                *self.last_snapshot_time.write().unwrap() = Some(now);

                tokio::spawn(async move {
                    if let Err(e) = Self::write_snapshot_async(&snapshot_file, snapshot_data).await
                    {
                        tracing::warn!("Failed to write probe history snapshot: {}", e);
                    }
                });
            }
        }
    }

    /// Creates a snapshot of the current state
    fn create_snapshot(&self) -> ProbeHistorySnapshot {
        let buckets = self.state_buckets.read().unwrap();
        ProbeHistorySnapshot {
            sample_count_total: self.sample_count_total.load(Ordering::Relaxed),
            sample_count_healthy: self.sample_count_healthy.load(Ordering::Relaxed),
            state_buckets: buckets.iter().cloned().collect(),
            max_state_age: self.max_state_age,
        }
    }

    /// Writes a snapshot to disk asynchronously
    async fn write_snapshot_async(
        snapshot_file: &Path,
        snapshot: ProbeHistorySnapshot,
    ) -> std::io::Result<()> {
        let json_data = serde_json::to_string_pretty(&snapshot).map_err(std::io::Error::other)?;

        // Write to a temporary file first, then atomically rename
        let temp_file = snapshot_file.with_extension("tmp");
        tokio::fs::write(&temp_file, json_data).await?;
        tokio::fs::rename(&temp_file, &snapshot_file).await?;

        Ok(())
    }

    /// Manually trigger a snapshot (useful for shutdown)
    #[cfg(test)]
    pub async fn force_snapshot(&self) -> std::io::Result<()> {
        if let Some(snapshot_file) = &self.snapshot_file {
            let snapshot = self.create_snapshot();
            Self::write_snapshot_async(&snapshot_file, snapshot).await?;
            *self.last_snapshot_time.write().unwrap() = Some(Instant::now());
        }
        Ok(())
    }

    /// Calculates the current availability percentage
    pub fn availability(&self) -> f64 {
        let (sample_count_healthy, sample_count_total) = (
            self.sample_count_healthy.load(Ordering::Relaxed),
            self.sample_count_total.load(Ordering::Relaxed),
        );

        match sample_count_total {
            0 => 100.0,
            _ => 100.0 * sample_count_healthy as f64 / sample_count_total as f64,
        }
    }

    /// Returns the total number of samples recorded
    #[cfg(test)]
    pub fn total_samples(&self) -> u64 {
        self.sample_count_total.load(Ordering::Relaxed)
    }

    /// Returns the number of healthy samples recorded
    #[cfg(test)]
    pub fn healthy_samples(&self) -> u64 {
        self.sample_count_healthy.load(Ordering::Relaxed)
    }

    /// Returns all state buckets
    pub fn get_state_buckets(&self) -> Vec<StateBucket> {
        self.state_buckets.read().unwrap().iter().cloned().collect()
    }

    /// Returns the number of state buckets currently stored
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.state_buckets.read().unwrap().len()
    }

    /// Returns the maximum number of state transitions that can be stored
    #[cfg(test)]
    pub const fn max_states(&self) -> usize {
        MAX_STATES
    }

    /// Returns the maximum state age configuration
    #[cfg(test)]
    pub fn max_state_age(&self) -> Duration {
        self.max_state_age
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::ProbeResult;
    use chrono::Duration;

    #[test]
    fn test_new_history_has_100_percent_availability() {
        let history: ProbeHistory<10> = ProbeHistory::new();
        assert_eq!(history.availability(), 100.0);
        assert_eq!(history.total_samples(), 0);
        assert_eq!(history.healthy_samples(), 0);
        assert_eq!(history.max_states(), 10);
    }

    #[test]
    fn test_add_passing_sample() {
        let history: ProbeHistory<10> = ProbeHistory::new();
        let mut result = ProbeResult::new();
        result.pass = true;

        history.add_sample(result);

        assert_eq!(history.availability(), 100.0);
        assert_eq!(history.total_samples(), 1);
        assert_eq!(history.healthy_samples(), 1);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_add_failing_sample() {
        let history: ProbeHistory<10> = ProbeHistory::new();
        let mut result = ProbeResult::new();
        result.pass = false;

        history.add_sample(result);

        assert_eq!(history.availability(), 0.0);
        assert_eq!(history.total_samples(), 1);
        assert_eq!(history.healthy_samples(), 0);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_same_state_aggregation() {
        let history: ProbeHistory<10> = ProbeHistory::new();

        // Add multiple samples with the same state (all passing)
        for i in 0..3 {
            let mut result = ProbeResult::new();
            result.pass = true;
            result.duration = Duration::milliseconds(100 + i * 10);
            history.add_sample(result);
        }

        // Should have only one state bucket with 3 samples
        assert_eq!(history.len(), 1);
        assert_eq!(history.total_samples(), 3);
        assert_eq!(history.healthy_samples(), 3);
        assert_eq!(history.availability(), 100.0);

        let buckets = history.get_state_buckets();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].total_samples, 3);
        assert_eq!(buckets[0].successful_samples, 3);
        assert_eq!(buckets[0].availability(), 100.0);
    }

    #[test]
    fn test_state_transition() {
        let history: ProbeHistory<10> = ProbeHistory::new();

        // Add passing sample
        let mut result = ProbeResult::new();
        result.pass = true;
        result.message = "Passing".to_string();
        history.add_sample(result.clone());

        // Add another passing sample (same state)
        history.add_sample(result.clone());

        // Add failing sample (state transition)
        let mut failing_result = ProbeResult::new();
        failing_result.pass = false;
        failing_result.message = "Failing".to_string();
        failing_result.start_time = result.start_time + Duration::seconds(10);
        history.add_sample(failing_result);

        // Should have 2 state buckets
        assert_eq!(history.len(), 2);
        assert_eq!(history.total_samples(), 3);
        assert_eq!(history.healthy_samples(), 2);

        let buckets = history.get_state_buckets();
        assert_eq!(buckets.len(), 2);

        // First bucket: 2 passing samples
        assert_eq!(buckets[0].total_samples, 2);
        assert_eq!(buckets[0].successful_samples, 2);
        assert_eq!(buckets[0].state.pass, true);
        assert!(buckets[0].end_time.is_some());

        // Second bucket: 1 failing sample
        assert_eq!(buckets[1].total_samples, 1);
        assert_eq!(buckets[1].successful_samples, 0);
        assert_eq!(buckets[1].state.pass, false);
        assert!(buckets[1].end_time.is_none()); // Current state
    }

    #[test]
    fn test_circular_buffer_overflow() {
        let history: ProbeHistory<3> = ProbeHistory::new();

        // Add 5 different states (more than buffer capacity)
        for i in 0..5 {
            let mut result = ProbeResult::new();
            result.pass = i % 2 == 0;
            result.message = format!("State {}", i);
            result.start_time = result.start_time + Duration::seconds(i as i64);
            history.add_sample(result);
        }

        // Should only keep the last 3 states
        assert_eq!(history.len(), 3);
        assert_eq!(history.total_samples(), 5);

        let buckets = history.get_state_buckets();
        assert_eq!(buckets.len(), 3);
    }

    #[test]
    fn test_state_bucket_functionality() {
        let mut result = ProbeResult::new();
        result.pass = true;
        result.duration = Duration::milliseconds(100);

        let mut bucket = StateBucket::new(&result);
        assert_eq!(bucket.total_samples, 1);
        assert_eq!(bucket.successful_samples, 1);
        assert_eq!(bucket.availability(), 100.0);

        // Add a failing sample to the same bucket
        let mut failing_result = ProbeResult::new();
        failing_result.pass = false;
        failing_result.duration = Duration::milliseconds(200);
        failing_result.validations = result.validations.clone(); // Same validations

        bucket.add_sample(&failing_result);
        assert_eq!(bucket.total_samples, 2);
        assert_eq!(bucket.successful_samples, 1); // Still only 1 successful
        assert_eq!(bucket.availability(), 50.0);
    }

    #[test]
    fn test_probe_state_equality() {
        let mut result1 = ProbeResult::new();
        result1.pass = true;

        let mut result2 = ProbeResult::new();
        result2.pass = true;

        let state1 = ProbeState::from_result(&result1);
        let state2 = ProbeState::from_result(&result2);

        assert_eq!(state1, state2); // Same passing state with no validations

        // Different passing state
        result2.pass = false;
        let state3 = ProbeState::from_result(&result2);
        assert_ne!(state1, state3);
    }

    #[test]
    fn test_max_state_age_configuration() {
        let history = ProbeHistory::<10>::new().with_max_state_age(Duration::minutes(15));
        assert_eq!(history.max_state_age(), Duration::minutes(15));

        let default_history: ProbeHistory<10> = ProbeHistory::new();
        assert_eq!(default_history.max_state_age(), Duration::hours(1));
    }

    #[test]
    fn test_time_alignment() {
        use chrono::TimeZone;

        // Test timestamp: Thursday, June 15, 2023, 2:37:23 PM UTC (Unix: 1686838643)
        let test_timestamp = Utc.with_ymd_and_hms(2023, 6, 15, 14, 37, 23).unwrap();

        // Test 1 hour (3600s) alignment - should align to hour boundaries
        let history_1h = ProbeHistory::<10>::new().with_max_state_age(Duration::hours(1));
        let aligned_1h = history_1h.align_start_time(test_timestamp);
        let expected_1h = Utc.with_ymd_and_hms(2023, 6, 15, 14, 0, 0).unwrap();
        assert_eq!(aligned_1h, expected_1h);

        // Test 20 minutes (1200s) alignment - should align to 20-minute boundaries
        let history_20m = ProbeHistory::<10>::new().with_max_state_age(Duration::minutes(20));
        let aligned_20m = history_20m.align_start_time(test_timestamp);
        // 2:37:23 should align to 2:20:00 (the 20-minute boundary before it)
        let expected_20m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 20, 0).unwrap();
        assert_eq!(aligned_20m, expected_20m);

        // Test 15 minutes (900s) alignment - should align to 15-minute boundaries
        let history_15m = ProbeHistory::<10>::new().with_max_state_age(Duration::minutes(15));
        let aligned_15m = history_15m.align_start_time(test_timestamp);
        // 2:37:23 should align to 2:30:00 (the 15-minute boundary before it)
        let expected_15m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap();
        assert_eq!(aligned_15m, expected_15m);

        // Test 6 hours (21600s) alignment
        let history_6h = ProbeHistory::<10>::new().with_max_state_age(Duration::hours(6));
        let aligned_6h = history_6h.align_start_time(test_timestamp);
        // 2:37:23 PM should align to 12:00:00 PM (the 6-hour boundary before it)
        let expected_6h = Utc.with_ymd_and_hms(2023, 6, 15, 12, 0, 0).unwrap();
        assert_eq!(aligned_6h, expected_6h);

        // Test 1 day (86400s) alignment
        let history_1d = ProbeHistory::<10>::new().with_max_state_age(Duration::days(1));
        let aligned_1d = history_1d.align_start_time(test_timestamp);
        // Should align to start of day (midnight)
        let expected_1d = Utc.with_ymd_and_hms(2023, 6, 15, 0, 0, 0).unwrap();
        assert_eq!(aligned_1d, expected_1d);

        // Test sub-second duration (no alignment)
        let history_500ms =
            ProbeHistory::<10>::new().with_max_state_age(Duration::milliseconds(500));
        let aligned_500ms = history_500ms.align_start_time(test_timestamp);
        assert_eq!(aligned_500ms, test_timestamp); // Should be unchanged
    }

    #[test]
    fn test_generic_alignment_boundaries() {
        use chrono::TimeZone;

        let test_timestamp = Utc.with_ymd_and_hms(2023, 6, 15, 14, 37, 23).unwrap();

        // Test custom durations to verify the generic algorithm

        // 2-hour duration (7200s) should align to 2-hour boundaries from Unix epoch
        let history_2h = ProbeHistory::<10>::new().with_max_state_age(Duration::hours(2));
        let aligned_2h = history_2h.align_start_time(test_timestamp);
        let expected_2h = Utc.with_ymd_and_hms(2023, 6, 15, 14, 0, 0).unwrap(); // 14:00 (2-hour boundary from epoch)
        assert_eq!(aligned_2h, expected_2h);

        // 3-hour duration (10800s) should align to 3-hour boundaries from Unix epoch
        let history_3h = ProbeHistory::<10>::new().with_max_state_age(Duration::hours(3));
        let aligned_3h = history_3h.align_start_time(test_timestamp);
        let expected_3h = Utc.with_ymd_and_hms(2023, 6, 15, 12, 0, 0).unwrap(); // 12:00 (3-hour boundary from epoch)
        assert_eq!(aligned_3h, expected_3h);

        // 10-minute duration (600s) should align to 10-minute boundaries from Unix epoch
        let history_10m = ProbeHistory::<10>::new().with_max_state_age(Duration::minutes(10));
        let aligned_10m = history_10m.align_start_time(test_timestamp);
        let expected_10m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap(); // 30 minutes (10-min boundary from epoch)
        assert_eq!(aligned_10m, expected_10m);

        // 45-minute duration (2700s) should align to 45-minute boundaries from Unix epoch
        let history_45m = ProbeHistory::<10>::new().with_max_state_age(Duration::minutes(45));
        let aligned_45m = history_45m.align_start_time(test_timestamp);
        let expected_45m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 15, 0).unwrap(); // 15 minutes (45-min boundary from epoch)
        assert_eq!(aligned_45m, expected_45m);
    }

    #[test]
    fn test_state_age_finalization() {
        use chrono::TimeZone;

        let history = ProbeHistory::<10>::new().with_max_state_age(Duration::minutes(15));

        let base_time = Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 0).unwrap();

        // Add initial sample
        let mut result1 = ProbeResult::new();
        result1.pass = true;
        result1.start_time = base_time;
        history.add_sample(result1);

        // Add sample within the same state and age limit (5 minutes later)
        let mut result2 = ProbeResult::new();
        result2.pass = true;
        result2.start_time = base_time + Duration::minutes(5);
        history.add_sample(result2);

        // Should still have only one state bucket
        assert_eq!(history.len(), 1);

        // Add sample with same state but past age limit (20 minutes later)
        let mut result3 = ProbeResult::new();
        result3.pass = true;
        result3.start_time = base_time + Duration::minutes(20);
        history.add_sample(result3);

        // Should now have two state buckets due to age limit
        assert_eq!(history.len(), 2);

        let buckets = history.get_state_buckets();
        assert!(buckets[0].end_time.is_some()); // First bucket should be finalized
        assert!(buckets[1].end_time.is_none()); // Second bucket should be current
    }

    #[tokio::test]
    async fn test_snapshot_creation_and_restore() {
        use tempfile::TempDir;

        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().join("snapshot.json");

        // Create history with some data
        let history = ProbeHistory::<10>::new()
            .with_snapshot_file(&temp_path)
            .unwrap();

        // Add some sample data
        let mut result1 = ProbeResult::new();
        result1.pass = true;
        result1.message = "Test message 1".to_string();
        history.add_sample(result1);

        let mut result2 = ProbeResult::new();
        result2.pass = false;
        result2.message = "Test message 2".to_string();
        history.add_sample(result2);

        // Force a snapshot
        history.force_snapshot().await.unwrap();

        // Verify snapshot file exists and contains data
        assert!(temp_path.exists());
        let snapshot_content = tokio::fs::read_to_string(&temp_path).await.unwrap();
        assert!(snapshot_content.contains("Test message 1"));
        assert!(snapshot_content.contains("Test message 2"));

        // Create a new history from the snapshot
        let restored_history = ProbeHistory::<10>::new()
            .with_snapshot_file(&temp_path)
            .unwrap();

        // Verify data was restored correctly
        assert_eq!(restored_history.total_samples(), history.total_samples());
        assert_eq!(
            restored_history.healthy_samples(),
            history.healthy_samples()
        );
        assert_eq!(restored_history.availability(), history.availability());
        assert_eq!(restored_history.len(), history.len());

        let original_buckets = history.get_state_buckets();
        let restored_buckets = restored_history.get_state_buckets();
        assert_eq!(original_buckets.len(), restored_buckets.len());

        for (orig, rest) in original_buckets.iter().zip(restored_buckets.iter()) {
            assert_eq!(orig.state.pass, rest.state.pass);
            assert_eq!(orig.state.message, rest.state.message);
            assert_eq!(orig.total_samples, rest.total_samples);
            assert_eq!(orig.successful_samples, rest.successful_samples);
        }

        // Clean up
        let _ = tokio::fs::remove_file(&temp_path).await;
    }

    #[test]
    fn test_snapshot_file_does_not_exist() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let non_existent_path = temp_dir.path().join("non_existent.json");

        // Should create new history without error when file doesn't exist
        let history = ProbeHistory::<10>::new()
            .with_snapshot_file(&non_existent_path)
            .unwrap();

        assert_eq!(history.total_samples(), 0);
        assert_eq!(history.healthy_samples(), 0);
        assert_eq!(history.availability(), 100.0);
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_default_constructor_no_snapshots() {
        let history: ProbeHistory<10> = ProbeHistory::default();

        // Add some data
        let mut result = ProbeResult::new();
        result.pass = true;
        history.add_sample(result);

        // Should work normally but without snapshot functionality
        assert_eq!(history.total_samples(), 1);
        assert_eq!(history.healthy_samples(), 1);
        assert_eq!(history.availability(), 100.0);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_serde_duration_module() {
        use serde_json;

        let duration = Duration::milliseconds(5000);
        let serialized = serde_json::to_string(&duration.num_milliseconds()).unwrap();
        assert_eq!(serialized, "5000");

        let deserialized: i64 = serde_json::from_str(&serialized).unwrap();
        let restored_duration = Duration::milliseconds(deserialized);
        assert_eq!(restored_duration, duration);
    }
}
