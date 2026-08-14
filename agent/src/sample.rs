use std::{borrow::Cow, collections::HashMap, fmt::Display};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize, de::Visitor};

#[derive(Debug, Clone, Default)]
pub struct Sample {
    metadata: HashMap<String, SampleValue>,
}

impl Sample {
    pub fn with<K: ToString, V: Into<SampleValue>>(mut self, key: K, value: V) -> Self {
        self.metadata.insert(key.to_string(), value.into());
        self
    }

    pub fn set<K: ToString, V: Into<SampleValue>>(&mut self, key: K, value: V) {
        self.metadata.insert(key.to_string(), value.into());
    }

    pub fn get<K: ToString>(&self, key: K) -> &SampleValue {
        self.metadata
            .get(&key.to_string())
            .unwrap_or(&SampleValue::None)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SampleValue {
    None,
    String(String),
    Double(f64),
    Int(i64),
    Bool(bool),
    DateTime(DateTime<Utc>),
    Duration(Duration),
    List(Vec<SampleValue>),
}

impl SampleValue {
    pub fn get_type(&self) -> &'static str {
        match self {
            SampleValue::None => "null",
            SampleValue::String(_) => "string",
            SampleValue::Double(_) => "double",
            SampleValue::Int(_) => "int",
            SampleValue::Bool(_) => "bool",
            SampleValue::DateTime(_) => "datetime",
            SampleValue::Duration(_) => "duration",
            SampleValue::List(_) => "list",
        }
    }
}

macro_rules! number {
    ($type:ident, $base:ty => $target:ty) => {
        impl From<$base> for SampleValue {
            fn from(value: $base) -> Self {
                SampleValue::$type(value as $target)
            }
        }
    };
}

number!(Int, i8 => i64);
number!(Int, i16 => i64);
number!(Int, u16 => i64);
number!(Int, i32 => i64);
number!(Int, u32 => i64);
number!(Int, i64 => i64);
number!(Double, f32 => f64);
number!(Double, f64 => f64);

impl From<String> for SampleValue {
    fn from(value: String) -> Self {
        SampleValue::String(value)
    }
}

impl From<bool> for SampleValue {
    fn from(value: bool) -> Self {
        SampleValue::Bool(value)
    }
}

impl From<&str> for SampleValue {
    fn from(value: &str) -> Self {
        SampleValue::String(value.to_string())
    }
}

impl From<DateTime<Utc>> for SampleValue {
    fn from(value: DateTime<Utc>) -> Self {
        SampleValue::DateTime(value)
    }
}

impl From<Duration> for SampleValue {
    fn from(value: Duration) -> Self {
        SampleValue::Duration(value)
    }
}

impl<T: Into<SampleValue>> From<Vec<T>> for SampleValue {
    fn from(value: Vec<T>) -> Self {
        SampleValue::List(value.into_iter().map(|v| v.into()).collect())
    }
}

impl<T: Into<SampleValue>> From<Option<T>> for SampleValue {
    fn from(value: Option<T>) -> Self {
        value.map_or(SampleValue::None, Into::into)
    }
}

impl Display for SampleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SampleValue::None => write!(f, "null"),
            SampleValue::String(value) => write!(f, "\"{}\"", value),
            SampleValue::Double(value) => write!(f, "{}", value),
            SampleValue::Int(value) => write!(f, "{}", value),
            SampleValue::Bool(value) => write!(f, "{}", value),
            SampleValue::DateTime(value) => write!(f, "{}", format_datetime(value)),
            SampleValue::Duration(value) => write!(f, "{}", format_duration(value)),
            SampleValue::List(value) => write!(f, "[{}]", value.iter().map(SampleValue::to_string).collect::<Vec<_>>().join(", ")),
        }
    }
}

/// Renders a timestamp in the same RFC 3339 form that `filt-rs` uses for its
/// own datetime values, so check failures and expression literals agree.
fn format_datetime(value: &DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// Renders a duration in `filt-rs`'s compact form (e.g. `30d`, `1h30m`) so it
/// matches the duration literals a check would compare it against.
fn format_duration(value: &Duration) -> String {
    filt_rs::FilterValue::Duration(*value).to_string()
}

impl filt_rs::Filterable for Sample {
    /// Exposes the sample's collected fields to the `filt-rs` expression
    /// language so probes can be validated with `checks`. Unknown keys resolve
    /// to `null`, matching both `Sample::get` and `filt-rs`'s own convention.
    fn get(&self, key: &str) -> filt_rs::FilterValue<'_> {
        self.metadata
            .get(key)
            .map(filt_rs::FilterValue::from)
            .unwrap_or(filt_rs::FilterValue::Null)
    }
}

impl<'a> From<&'a SampleValue> for filt_rs::FilterValue<'a> {
    fn from(value: &'a SampleValue) -> Self {
        match value {
            SampleValue::None => filt_rs::FilterValue::Null,
            SampleValue::String(value) => filt_rs::FilterValue::String(Cow::Borrowed(value)),
            SampleValue::Double(value) => filt_rs::FilterValue::Number(*value),
            SampleValue::Int(value) => filt_rs::FilterValue::Number(*value as f64),
            SampleValue::Bool(value) => filt_rs::FilterValue::Bool(*value),
            SampleValue::DateTime(value) => filt_rs::FilterValue::DateTime(*value),
            SampleValue::Duration(value) => filt_rs::FilterValue::Duration(*value),
            SampleValue::List(value) => {
                filt_rs::FilterValue::Tuple(value.iter().map(filt_rs::FilterValue::from).collect())
            }
        }
    }
}

impl Serialize for SampleValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SampleValue::None => serializer.serialize_none(),
            SampleValue::String(value) => serializer.serialize_str(value),
            SampleValue::Double(value) => serializer.serialize_f64(*value),
            SampleValue::Int(value) => serializer.serialize_i64(*value),
            SampleValue::Bool(value) => serializer.serialize_bool(*value),
            SampleValue::DateTime(value) => serializer.serialize_str(&format_datetime(value)),
            SampleValue::Duration(value) => serializer.serialize_str(&format_duration(value)),
            SampleValue::List(value) => serializer.collect_seq(value),
        }
    }
}

impl<'de> Deserialize<'de> for SampleValue {
    /// Datetimes and durations serialize to their string forms and are read
    /// back as [`SampleValue::String`]; samples are only ever deserialized for
    /// debugging, never round-tripped through storage or the cluster.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SampleValueVisitor)
    }
}

struct SampleValueVisitor;
impl<'de> Visitor<'de> for SampleValueVisitor {
    type Value = SampleValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("null, a string, a number, a boolean, or a list thereof")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(SampleValue::Bool(value))
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    {
        Ok(SampleValue::Int(v as i64))
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    {
        Ok(SampleValue::Int(v as i64))
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    {
        Ok(SampleValue::Int(v as i64))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(SampleValue::Int(value))
    }

    fn visit_u8<E>(self, value: u8) -> Result<Self::Value, E> {
        Ok(SampleValue::Int(value as i64))
    }

    fn visit_u16<E>(self, value: u16) -> Result<Self::Value, E> {
        Ok(SampleValue::Int(value as i64))
    }

    fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E> {
        Ok(SampleValue::Int(value as i64))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(SampleValue::Int(value as i64))
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E> {
        Ok(SampleValue::Double(value as f64))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(SampleValue::Double(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(SampleValue::String(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(SampleValue::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(SampleValue::None)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    {
        Ok(SampleValue::None)
    }

    fn visit_seq<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
    where
        V: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = visitor.next_element()? {
            values.push(value);
        }
        Ok(SampleValue::List(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn datetime() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 12, 13, 30, 45).unwrap()
    }

    #[test]
    fn test_sample_value_from() {
        let sv: SampleValue = 42i32.into();
        assert_eq!(sv, SampleValue::Int(42));

        let sv: SampleValue = 3.14f64.into();
        assert_eq!(sv, SampleValue::Double(3.14));

        let sv: SampleValue = "hello".into();
        assert_eq!(sv, SampleValue::String("hello".to_string()));

        let sv: SampleValue = true.into();
        assert_eq!(sv, SampleValue::Bool(true));

        let sv: SampleValue = datetime().into();
        assert_eq!(sv, SampleValue::DateTime(datetime()));

        let sv: SampleValue = Duration::days(30).into();
        assert_eq!(sv, SampleValue::Duration(Duration::days(30)));

        let sv: SampleValue = vec![1, 2, 3].into();
        assert_eq!(
            sv,
            SampleValue::List(vec![
                SampleValue::Int(1),
                SampleValue::Int(2),
                SampleValue::Int(3)
            ])
        );
    }

    #[test]
    fn test_sample_value_get_type() {
        let sv = SampleValue::Int(42);
        assert_eq!(sv.get_type(), "int");

        let sv = SampleValue::Double(3.14);
        assert_eq!(sv.get_type(), "double");

        let sv = SampleValue::String("hello".to_string());
        assert_eq!(sv.get_type(), "string");

        let sv = SampleValue::Bool(true);
        assert_eq!(sv.get_type(), "bool");

        let sv = SampleValue::DateTime(datetime());
        assert_eq!(sv.get_type(), "datetime");

        let sv = SampleValue::Duration(Duration::days(30));
        assert_eq!(sv.get_type(), "duration");

        let sv = SampleValue::None;
        assert_eq!(sv.get_type(), "null");

        let sv = SampleValue::List(vec![]);
        assert_eq!(sv.get_type(), "list");
    }

    #[test]
    fn test_sample_value_display() {
        let sv = SampleValue::List(vec![
            SampleValue::Int(42),
            SampleValue::Double(3.14),
            SampleValue::String("hello".to_string()),
            SampleValue::Bool(true),
            SampleValue::None,
        ]);

        let display = format!("{}", sv);
        assert_eq!(display, "[42, 3.14, \"hello\", true, null]");
    }

    /// Temporal values render exactly as `filt-rs` renders its own, so a failing
    /// check reports a value in the same notation its expression is written in.
    #[test]
    fn test_sample_value_display_temporal() {
        assert_eq!(
            SampleValue::DateTime(datetime()).to_string(),
            "2026-06-12T13:30:45Z"
        );
        assert_eq!(
            SampleValue::Duration(Duration::minutes(90)).to_string(),
            "1h30m"
        );
    }

    #[test]
    fn test_sample_value_serialize_deserialize() {
        let sv = SampleValue::Int(42);
        assert_eq!(round_trip(&sv), sv);

        let sv = SampleValue::Double(3.14);
        assert_eq!(round_trip(&sv), sv);

        let sv = SampleValue::String("hello".to_string());
        assert_eq!(round_trip(&sv), sv);

        let sv = SampleValue::Bool(true);
        assert_eq!(round_trip(&sv), sv);

        let sv = SampleValue::None;
        assert_eq!(round_trip(&sv), sv);

        let sv = SampleValue::List(vec![
            SampleValue::Int(42),
            SampleValue::Double(3.14),
            SampleValue::String("hello".to_string()),
            SampleValue::Bool(true),
            SampleValue::None,
        ]);
        assert_eq!(round_trip(&sv), sv);
    }

    /// Temporal values are written out in their string form, which is all the
    /// debug-oriented serialization of a sample needs to convey.
    #[test]
    fn test_sample_value_serialize_temporal() {
        assert_eq!(
            round_trip(&SampleValue::DateTime(datetime())),
            SampleValue::String("2026-06-12T13:30:45Z".to_string())
        );
        assert_eq!(
            round_trip(&SampleValue::Duration(Duration::minutes(90))),
            SampleValue::String("1h30m".to_string())
        );
    }

    fn round_trip(value: &SampleValue) -> SampleValue {
        let serialized = serde_json::to_string(value).unwrap();
        println!("Serialized: {serialized} (from {value})");
        serde_json::from_str(&serialized).unwrap()
    }

    #[test]
    fn test_sample_value_into_filter_value() {
        use filt_rs::FilterValue;

        assert_eq!(FilterValue::from(&SampleValue::None), FilterValue::Null);
        assert_eq!(
            FilterValue::from(&SampleValue::Bool(true)),
            FilterValue::Bool(true)
        );
        assert_eq!(
            FilterValue::from(&SampleValue::Int(42)),
            FilterValue::Number(42.0)
        );
        assert_eq!(
            FilterValue::from(&SampleValue::Double(3.5)),
            FilterValue::Number(3.5)
        );
        assert_eq!(
            FilterValue::from(&SampleValue::String("hello".into())),
            FilterValue::String("hello".into())
        );
        assert_eq!(
            FilterValue::from(&SampleValue::DateTime(datetime())),
            FilterValue::DateTime(datetime())
        );
        assert_eq!(
            FilterValue::from(&SampleValue::Duration(Duration::days(30))),
            FilterValue::Duration(Duration::days(30))
        );
        assert_eq!(
            FilterValue::from(&SampleValue::List(vec![
                SampleValue::Int(1),
                SampleValue::String("a".into()),
            ])),
            FilterValue::Tuple(vec![FilterValue::Number(1.0), FilterValue::String("a".into())])
        );
    }

    #[test]
    fn test_sample_is_filterable() {
        use filt_rs::{Filter, FilterValue, Filterable};

        let sample = Sample::default()
            .with("http.status", 200)
            .with("http.header.content-type", "text/html");

        // Present keys resolve to their values; missing keys resolve to null.
        // (Call the trait method explicitly, since the inherent `Sample::get`
        // shadows it for direct `sample.get(..)` calls.)
        assert_eq!(
            Filterable::get(&sample, "http.status"),
            FilterValue::Number(200.0)
        );
        assert_eq!(
            Filterable::get(&sample, "missing.key"),
            FilterValue::Null
        );

        // Hyphenated and dotted property names are usable directly in expressions.
        let filter =
            Filter::new(r#"http.status >= 200 && http.status < 300 && http.header.content-type contains "html""#)
                .expect("parse filter");
        assert!(filter.matches(&sample).expect("evaluate filter"));

        let failing = Filter::new("http.status == 500").expect("parse filter");
        assert!(!failing.matches(&sample).expect("evaluate filter"));
    }

    /// Temporal fields are what make expiry checks readable, so verify they can
    /// be compared against `now()` and against duration literals like `30d`.
    #[test]
    fn test_sample_temporal_checks() {
        use filt_rs::Filter;

        let sample = Sample::default()
            .with("tls.not_after", Utc::now() + Duration::days(60))
            .with("tls.expires_in", Duration::days(60));

        for check in [
            "tls.not_after > now() + 30d",
            "tls.expires_in > 30d",
            "tls.expires_in < 90d",
        ] {
            let filter = Filter::new(check).expect("parse filter");
            assert!(
                filter.matches(&sample).expect("evaluate filter"),
                "expected {check} to match"
            );
        }

        let filter = Filter::new("tls.expires_in < 30d").expect("parse filter");
        assert!(!filter.matches(&sample).expect("evaluate filter"));
    }
}
