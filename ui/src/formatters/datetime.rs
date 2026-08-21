use chrono::{DateTime, NaiveDateTime, Utc};
use grey_api::{ApiError, is_valid_update_timestamp};

/// Formats a timestamp as a date only (`YYYY-MM-DD`).
pub fn date_format(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d").to_string()
}

/// Formats a timestamp to the minute, in UTC (`YYYY-MM-DD HH:MM UTC`).
pub fn time_format(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// The `value` for an `<input type="datetime-local">`, in UTC (`YYYY-MM-DDTHH:MM`). The admin
/// incident forms treat these inputs as UTC — as the rest of the UI displays times — rather than in
/// the browser's local zone, so this is the exact inverse of [`parse_datetime_local`].
pub fn datetime_local_value(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%dT%H:%M").to_string()
}

/// Parses an `<input type="datetime-local">` value as a UTC timestamp, accepting the minute- and
/// second-precision forms browsers emit. `None` for a blank or unparseable value (the caller then
/// leaves the timestamp to the server).
pub fn parse_datetime_local(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .map(|naive| naive.and_utc())
}

/// Resolves an `<input type="datetime-local">` value into the optional `timestamp` an incident
/// update carries: `None` when the field is blank (the server stamps the update as it is posted), and
/// an [`ApiError`] describing the problem when the value is unusable — unparseable, or dated in the
/// future, which the API would refuse since the newest update sets an incident's current impact.
pub fn resolve_update_timestamp(value: &str) -> Result<Option<DateTime<Utc>>, ApiError> {
    match parse_datetime_local(value) {
        None if value.trim().is_empty() => Ok(None),
        None => Err(ApiError::new(
            "Enter the time as a date and time, or leave it blank to use the current time.",
        )),
        Some(timestamp) if !is_valid_update_timestamp(timestamp, Utc::now()) => Err(ApiError::new(
            "That time is in the future — an incident can only record what has already happened.",
        )),
        Some(timestamp) => Ok(Some(timestamp)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn datetime_local_round_trips_through_utc() {
        let time = ts(1_700_000_100); // 2023-11-14 22:15:00 UTC
        assert_eq!(datetime_local_value(time), "2023-11-14T22:15");
        assert_eq!(parse_datetime_local(&datetime_local_value(time)), Some(time));
    }

    #[test]
    fn resolving_a_timestamp_allows_blank_and_past_but_not_future() {
        assert_eq!(resolve_update_timestamp("   "), Ok(None), "blank leaves the stamping to the server");
        assert_eq!(
            resolve_update_timestamp("2023-11-14T22:15"),
            Ok(Some(ts(1_700_000_100))),
            "a past time backdates the update"
        );
        assert!(resolve_update_timestamp("last tuesday").is_err(), "an unparseable value is refused");

        let future = datetime_local_value(Utc::now() + chrono::Duration::days(1));
        assert!(resolve_update_timestamp(&future).is_err(), "a future-dated update is refused");
    }

    #[test]
    fn parse_accepts_seconds_and_rejects_junk() {
        assert_eq!(parse_datetime_local("2023-11-14T22:15:30"), Some(ts(1_700_000_130)));
        assert_eq!(parse_datetime_local("  2023-11-14T22:15  "), Some(ts(1_700_000_100)));
        assert_eq!(parse_datetime_local(""), None, "a blank input leaves the time to the server");
        assert_eq!(parse_datetime_local("yesterday"), None);
    }
}
