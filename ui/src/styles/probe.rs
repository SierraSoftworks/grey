/// The colour class for a probe's current state: a passing probe is `ok`, while a failing one is
/// graded by how bad its recent availability is (`warn` above 80%, otherwise `error`).
pub fn probe_class(passing: bool, recent_availability: f64) -> &'static str {
    if passing {
        "ok"
    } else if recent_availability > 80.0 {
        "warn"
    } else {
        "error"
    }
}

/// The colour class for a history segment. The most recent segment grades off the live streak state
/// (`current_passing`) so a recovery or fresh failure shows immediately; older segments fall back to
/// their average availability.
pub fn sample_class(current_passing: Option<bool>, max_availability: f64) -> &'static str {
    match (current_passing, max_availability) {
        (Some(false), _) => "error",
        (Some(true), sli) if sli > 99.9 => "ok",
        (Some(true), _) => "warn",
        (None, sli) if sli > 99.9 => "ok",
        (None, sli) if sli < 90.0 => "error",
        (None, _) => "warn",
    }
}

/// The colour class for a simple pass/fail outcome (a streak, validation or observation): `ok` when
/// passing, `error` otherwise.
pub fn pass_class(passing: bool) -> &'static str {
    if passing {
        "ok"
    } else {
        "error"
    }
}
