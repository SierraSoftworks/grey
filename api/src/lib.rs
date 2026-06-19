mod cron;
mod error;
mod etag;
mod identifier;
mod incident;
mod probe;
mod probe_history_bucket;
mod serializers;
mod streak;
mod ui;
mod peer;
mod observation;
mod webhook;

pub use cron::*;
pub use error::*;
pub use etag::*;
pub use identifier::*;
pub use incident::*;
pub use observation::*;
pub use peer::*;
pub use probe::*;
pub use probe_history_bucket::*;
pub use streak::*;
pub use ui::*;
pub use webhook::*;

pub trait Mergeable {
    fn merge(&mut self, other: &Self);
}
