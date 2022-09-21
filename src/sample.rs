use std::{collections::HashMap, fmt::Display};

use serde::{de::Visitor, Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct Sample {
    metadata: HashMap<String, SampleValue>,
}

impl Sample {
    pub fn with<K: ToString, V: Into<SampleValue>>(mut self, key: K, value: V) -> Self {
        self.metadata.insert(key.to_string(), value.into());
        self
    }

    pub fn get<K: ToString>(&self, key: K) -> &SampleValue {
        self.metadata.get(&key.to_string()).unwrap_or(&SampleValue::None)
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
            SampleValue::None => "none",
            SampleValue::String(_) => "string",
            SampleValue::Double(_) => "double",
            SampleValue::Int(_) => "int",
            SampleValue::Bool(_) => "bool",
            SampleValue::List(_) => "list",
        }
    }
}

impl From<i16> for SampleValue {
    fn from(value: i16) -> Self {
        SampleValue::Int(value.into())
    }
}

impl From<u16> for SampleValue {
    fn from(value: u16) -> Self {
        SampleValue::Int(value.into())
    }
}

impl From<i32> for SampleValue {
    fn from(value: i32) -> Self {
        SampleValue::Int(value.into())
    }
}

impl From<u32> for SampleValue {
    fn from(value: u32) -> Self {
        SampleValue::Int(value.into())
    }
}

impl From<i64> for SampleValue {
    fn from(value: i64) -> Self {
        SampleValue::Int(value)
    }
}

impl From<f64> for SampleValue {
    fn from(value: f64) -> Self {
        SampleValue::Double(value)
    }
}

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

impl<T : Into<SampleValue>> From<Vec<T>> for SampleValue {
    fn from(value: Vec<T>) -> Self {
        SampleValue::List(value.into_iter().map(|v| v.into()).collect())
    }
}

impl Display for SampleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SampleValue::None => write!(f, "None"),
            SampleValue::String(value) => write!(f, "{}", value),
            SampleValue::Double(value) => write!(f, "{}", value),
            SampleValue::Int(value) => write!(f, "{}", value),
            SampleValue::Bool(value) => write!(f, "{}", value),
            SampleValue::List(value) => write!(f, "{:?}", value),
        }
    }
}

impl Serialize for SampleValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
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
        D: serde::Deserializer<'de> {
        deserializer.deserialize_any(SampleValueVisitor)
    }
}

struct SampleValueVisitor;
impl <'de> Visitor<'de> for SampleValueVisitor {
    type Value = SampleValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("null, a string, a number, a boolean, or a list thereof")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(SampleValue::None)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(SampleValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(SampleValue::Int(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(SampleValue::Int(value as i64))
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