use std::fmt::Debug;

use serde::{de::DeserializeOwned, Serialize, Deserialize};

pub trait Versioned: Sized {
    type Diff: Into<Self> + Serialize + DeserializeOwned + Debug + Clone;

    fn version(&self) -> u64;

    fn diff(&self, version: u64) -> Option<Self::Diff>
    where
        Self: Sized;

    fn apply(&mut self, diff: &Self::Diff);
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LastWriteWinsValue<T> {
    pub version: u64,
    pub value: T,
}

#[allow(dead_code)]
impl<T> LastWriteWinsValue<T> {
    pub fn new(value: T) -> Self {
        Self { version: 1, value }
    }

    pub fn with_version(self, version: u64) -> Self {
        Self { version, ..self }
    }
}

impl<T> From<(u64, T)> for LastWriteWinsValue<T> {
    fn from(value: (u64, T)) -> Self {
        Self {
            version: value.0,
            value: value.1,
        }
    }
}

impl<T: Clone + Debug + Serialize + DeserializeOwned> Versioned for LastWriteWinsValue<T> {
    type Diff = Self;

    fn version(&self) -> u64 {
        self.version
    }

    fn diff(&self, version: u64) -> Option<Self::Diff> {
        if version < self.version {
            Some(self.clone())
        } else {
            None
        }
    }

    fn apply(&mut self, diff: &Self::Diff) {
        if diff.version > self.version {
            *self = diff.clone();
        }
    }
}