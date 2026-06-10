/// Formats a duration compactly using its largest whole unit, e.g. "42s", "17m", "3h", "5d".
pub fn compact_duration(duration: chrono::Duration) -> String {
    let seconds = duration.num_seconds().max(0);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    format!("{}d", hours / 24)
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
        assert_eq!(compact_duration(chrono::Duration::hours(36)), "1d");
    }
}
