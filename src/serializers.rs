/// Serializer for `chrono::Duration` as humantime string format
pub mod chrono_duration_humantime {
    use chrono::Duration;
    use serde::de::Visitor;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let std_duration = std::time::Duration::from_millis(duration.num_milliseconds() as u64);
        let humantime_string = humantime::format_duration(std_duration).to_string();
        serializer.serialize_str(&humantime_string)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(ChronoDurationVisitor)
    }

    struct ChronoDurationVisitor;

    impl<'de> Visitor<'de> for ChronoDurationVisitor {
        type Value = Duration;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a humantime duration string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let std_duration: std::time::Duration = humantime::parse_duration(value)
                .map_err(|e| E::custom(format!("Invalid duration format: {}", e)))?;
            Ok(Duration::from_std(std_duration)
                .map_err(|e| E::custom(format!("Duration conversion error: {}", e)))?)
        }
    }
}
