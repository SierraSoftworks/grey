//! Pure value → CSS-class converters shared across the UI.
//!
//! These map domain values (incident impact, probe/streak state, peer health) to the colour class
//! names defined in the stylesheet, keeping that vocabulary in one place rather than duplicated as
//! inline `match`es across components. The SCSS partials living alongside this module own the
//! visual definitions of those classes.

mod cluster;
mod cron;
mod incident;
mod probe;

pub use cluster::*;
pub use cron::*;
pub use incident::*;
pub use probe::*;
