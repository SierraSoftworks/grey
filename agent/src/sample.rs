use std::{collections::HashMap, fmt::Display};

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

impl<T: Into<SampleValue>> From<Vec<T>> for SampleValue {
    fn from(value: Vec<T>) -> Self {
        SampleValue::List(value.into_iter().map(|v| v.into()).collect())
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
            SampleValue::List(value) => write!(f, "[{}]", value.iter().map(SampleValue::to_string).collect::<Vec<_>>().join(", ")),
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
            SampleValue::List(value) => serializer.collect_seq(value),
        }
    }
}

impl<'de> Deserialize<'de> for SampleValue {
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

    fn round_trip(value: &SampleValue) -> SampleValue {
        let serialized = serde_json::to_string(value).unwrap();
        println!("Serialized: {serialized} (from {value})");
        serde_json::from_str(&serialized).unwrap()
    }
}