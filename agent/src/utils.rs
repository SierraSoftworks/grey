use chrono::DateTime;

pub trait Elide {
    type Output;
    fn elide(&self, len: usize) -> Self::Output;
}

impl Elide for String {
    type Output = String;
    fn elide(&self, len: usize) -> Self::Output {
        if self.len() > len {
            format!("{}...", &self[..len - 3])
        } else {
            self.clone()
        }
    }
}

impl Elide for &str {
    type Output = String;
    fn elide(&self, len: usize) -> Self::Output {
        if self.len() > len {
            format!("{}...", &self[..len - 3])
        } else {
            self.to_string()
        }
    }
}

pub trait TimeAlignmentExt {
    fn align(&self, interval: std::time::Duration) -> Self;
}

impl<Tz: chrono::TimeZone> TimeAlignmentExt for chrono::DateTime<Tz> {
    fn align(&self, interval: std::time::Duration) -> Self {
        let interval_secs = interval.as_secs() as i64;

        if interval_secs < 1 {
            return self.clone(); // No alignment for zero-intervals
        }

        let unix_timestamp = self.timestamp();
        let aligned_timestamp = (unix_timestamp / interval_secs) * interval_secs;

        DateTime::<chrono::Utc>::from_timestamp(aligned_timestamp, 0)
            .unwrap_or(self.to_utc())
            .with_timezone(&self.timezone())
    }
}

pub fn random_start_offset(interval: std::time::Duration) -> std::time::Duration {
    let start_delay = rand::random::<u128>() % interval.as_millis();
    std::time::Duration::from_millis(start_delay as u64)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn test_time_alignment() {
        // Test timestamp: Thursday, June 15, 2023, 2:37:23 PM UTC (Unix: 1686838643)
        let test_timestamp = Utc.with_ymd_and_hms(2023, 6, 15, 14, 37, 23).unwrap();

        // Test 1 hour (3600s) alignment - should align to hour boundaries
        let aligned_1h = test_timestamp.align(std::time::Duration::from_secs(3600));
        let expected_1h = Utc.with_ymd_and_hms(2023, 6, 15, 14, 0, 0).unwrap();
        assert_eq!(aligned_1h, expected_1h);

        // Test 20 minutes (1200s) alignment - should align to 20-minute boundaries
        let aligned_20m = test_timestamp.align(std::time::Duration::from_secs(1200));
        let expected_20m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 20, 0).unwrap();
        assert_eq!(aligned_20m, expected_20m);

        // Test 15 minutes (900s) alignment - should align to 15-minute boundaries
        let aligned_15m = test_timestamp.align(std::time::Duration::from_secs(900));
        // 2:37:23 should align to 2:30:00 (the 15-minute boundary before it)
        let expected_15m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap();
        assert_eq!(aligned_15m, expected_15m);

        // Test 6 hours (21600s) alignment
        let aligned_6h = test_timestamp.align(std::time::Duration::from_secs(21600));
        // 2:37:23 PM should align to 12:00:00 PM (the 6-hour boundary before it)
        let expected_6h = Utc.with_ymd_and_hms(2023, 6, 15, 12, 0, 0).unwrap();
        assert_eq!(aligned_6h, expected_6h);

        // Test 1 day (86400s) alignment
        let aligned_1d = test_timestamp.align(std::time::Duration::from_secs(86400));
        // Should align to start of day (midnight)
        let expected_1d = Utc.with_ymd_and_hms(2023, 6, 15, 0, 0, 0).unwrap();
        assert_eq!(aligned_1d, expected_1d);

        // Test sub-second duration (no alignment)
        let aligned_500ms = test_timestamp.align(std::time::Duration::from_millis(500));
        assert_eq!(aligned_500ms, test_timestamp); // Should be unchanged
    }

    #[test]
    fn test_generic_alignment_boundaries() {
        let test_timestamp = Utc.with_ymd_and_hms(2023, 6, 15, 14, 37, 23).unwrap();

        // 2-hour duration (7200s) should align to 2-hour boundaries from Unix epoch
        let aligned_2h = test_timestamp.align(std::time::Duration::from_secs(7200));
        let expected_2h = Utc.with_ymd_and_hms(2023, 6, 15, 14, 0, 0).unwrap(); // 14:00 (2-hour boundary from epoch)
        assert_eq!(aligned_2h, expected_2h);

        // 3-hour duration (10800s) should align to 3-hour boundaries from Unix epoch
        let aligned_3h = test_timestamp.align(std::time::Duration::from_secs(10800));
        let expected_3h = Utc.with_ymd_and_hms(2023, 6, 15, 12, 0, 0).unwrap(); // 12:00 (3-hour boundary from epoch)
        assert_eq!(aligned_3h, expected_3h);

        // 10-minute duration (600s) should align to 10-minute boundaries from Unix epoch
        let aligned_10m = test_timestamp.align(std::time::Duration::from_secs(600));
        let expected_10m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap(); // 30 minutes (10-min boundary from epoch)
        assert_eq!(aligned_10m, expected_10m);

        // 45-minute duration (2700s) should align to 45-minute boundaries from Unix epoch
        let aligned_45m = test_timestamp.align(std::time::Duration::from_secs(2700));
        let expected_45m = Utc.with_ymd_and_hms(2023, 6, 15, 14, 15, 0).unwrap(); // 15 minutes (45-min boundary from epoch)
        assert_eq!(aligned_45m, expected_45m);
    }
}
