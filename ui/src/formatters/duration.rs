/// Formats a duration compactly using its largest whole unit, e.g. "42s", "17m", "3h", "5d".
/// 
/// It offsets by 0.5*boundary to round to the nearest unit, e.g. "89s" -> "1m" and "90s" -> "2m".
pub fn compact_duration(duration: chrono::Duration) -> String {
    let seconds = duration.num_seconds().max(0);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = (seconds + 30) / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = (minutes + 30) / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    format!("{}d", (hours + 12) / 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_duration() {
        assert_eq!(compact_duration(chrono::Duration::seconds(-5)), "0s");
        assert_eq!(compact_duration(chrono::Duration::seconds(42)), "42s");
        assert_eq!(compact_duration(chrono::Duration::minutes(17)), "17m");
        assert_eq!(compact_duration(chrono::Duration::hours(3)), "3h");
        assert_eq!(compact_duration(chrono::Duration::days(5)), "5d");
        assert_eq!(compact_duration(chrono::Duration::hours(36)), "2d");
        assert_eq!(compact_duration(chrono::Duration::seconds(89)), "1m");
        assert_eq!(compact_duration(chrono::Duration::seconds(90)), "2m");
        assert_eq!(compact_duration(chrono::Duration::minutes(89)), "1h");
        assert_eq!(compact_duration(chrono::Duration::minutes(90)), "2h");
    }
}
