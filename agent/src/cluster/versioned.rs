use std::fmt::Debug;

use serde::{de::DeserializeOwned, Serialize};

pub trait Versioned: Sized {
    type Diff: Into<Self> + Serialize + DeserializeOwned + Debug + Clone;

    fn version(&self) -> u64;

    fn diff(&self, version: u64) -> Option<Self::Diff>
    where
        Self: Sized;

    fn apply(&mut self, diff: &Self::Diff);
}
