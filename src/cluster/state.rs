pub trait Versioned {
    fn version(&self) -> u64;
    fn diff(&self, version: u64) -> Option<Self>
    where
        Self: Sized;
    fn apply(&mut self, other: &Self);
}
