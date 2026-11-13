use yewi::Tone;

/// The tone for a probe's current state: a passing probe is `Ok`, while a failing one is graded by
/// how bad its recent availability is (`Warn` above 80%, otherwise `Danger`).
pub fn probe_tone(passing: bool, recent_availability: f64) -> Tone {
    if passing {
        Tone::Ok
    } else if recent_availability > 80.0 {
        Tone::Warn
    } else {
        Tone::Danger
    }
}

/// The tone for a history segment. The most recent segment grades off the live streak state
/// (`current_passing`) so a recovery or fresh failure shows immediately; older segments fall back to
/// their average availability.
pub fn sample_tone(current_passing: Option<bool>, max_availability: f64) -> Tone {
    match (current_passing, max_availability) {
        (Some(false), _) => Tone::Danger,
        (Some(true), sli) if sli > 90.0 => Tone::Ok,
        (Some(true), _) => Tone::Warn,
        (None, sli) if sli > 90.0 => Tone::Ok,
        (None, sli) if sli < 50.0 => Tone::Danger,
        (None, _) => Tone::Warn,
    }
}

/// The tone for a simple pass/fail outcome (a streak, validation or observation).
pub fn pass_tone(passing: bool) -> Tone {
    if passing { Tone::Ok } else { Tone::Danger }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The newest segment follows the live state; older ones only have their averages.
    #[test]
    fn the_live_state_overrides_a_segments_average() {
        let cases = [
            (Some(false), 100.0, Tone::Danger),
            (Some(true), 95.0, Tone::Ok),
            (Some(true), 20.0, Tone::Warn),
            (None, 95.0, Tone::Ok),
            (None, 70.0, Tone::Warn),
            (None, 20.0, Tone::Danger),
        ];
        for (live, availability, expected) in cases {
            assert_eq!(sample_tone(live, availability), expected, "{live:?} at {availability}%");
        }
    }
}
