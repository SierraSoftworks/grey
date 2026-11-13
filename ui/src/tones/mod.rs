//! Pure value → [`yewi::Tone`] converters shared across the UI.
//!
//! These map domain values (incident impact, probe/streak state, peer health) to the design
//! system's status tones, keeping that vocabulary in one place rather than duplicated as inline
//! `match`es across components.

mod cluster;
mod cron;
mod incident;
mod probe;

pub use cluster::*;
pub use cron::*;
pub use incident::*;
pub use probe::*;
