use std::collections::HashMap;

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

#[derive(Debug, Clone)]
pub enum SampleValue {
    None,
    String(String),
    Double(f64),
    Int(i64),
    Bool(bool),
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