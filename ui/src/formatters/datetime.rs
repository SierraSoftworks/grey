use chrono::{DateTime, Utc};

/// Formats a timestamp as a date only (`YYYY-MM-DD`).
pub fn date_format(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d").to_string()
}

/// Formats a timestamp to the minute, in UTC (`YYYY-MM-DD HH:MM UTC`).
pub fn time_format(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M UTC").to_string()
}
