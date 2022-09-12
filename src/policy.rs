use std::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(with="duration_ms")]
    pub interval: Duration,
    #[serde(with="duration_ms")]
    pub timeout: Duration,
    #[serde(default)]
    pub retries: Option<u8>,
}

mod duration_ms {
    use std::time::Duration;

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(value.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_u64(DurationMillisecondVisitor)
    }

    struct DurationMillisecondVisitor;
    impl <'de> serde::de::Visitor<'de> for DurationMillisecondVisitor {
        type Value = Duration;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a millisecond duration")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Duration::from_millis(value))
        }
    }
}