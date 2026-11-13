use grey_api::{CronHealth, CronRun, CronStatus};
use yewi::Tone;

/// The tone for a cron's derived health: a healthy run is `Ok`, an in-flight run is `Active`, an
/// overdue or overrunning run is `Warn`, a failed run is `Danger`, and a never-seen cron is
/// `Neutral`.
pub fn cron_tone(health: CronHealth) -> Tone {
    match health {
        CronHealth::Succeeded => Tone::Ok,
        CronHealth::Running => Tone::Active,
        CronHealth::Missing | CronHealth::Stuck => Tone::Warn,
        CronHealth::Failed => Tone::Danger,
        CronHealth::Pending => Tone::Neutral,
    }
}

/// The tone for a single run cell in the recent-runs strip: a successful run is `Ok`, an in-flight
/// run is `Active`, and a failed run is `Danger`. A monitor-synthesised placeholder for a
/// missed/stuck run is `Neutral` (grey), so a detected fault is visually distinct from a
/// job-reported failure.
pub fn cron_run_tone(run: &CronRun) -> Tone {
    if run.reason.is_some() {
        return Tone::Neutral;
    }
    match run.status {
        CronStatus::Succeeded => Tone::Ok,
        CronStatus::Running => Tone::Active,
        CronStatus::Failed => Tone::Danger,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_map_every_variant() {
        assert_eq!(cron_tone(CronHealth::Succeeded), Tone::Ok);
        assert_eq!(cron_tone(CronHealth::Running), Tone::Active);
        assert_eq!(cron_tone(CronHealth::Stuck), Tone::Warn);
        assert_eq!(cron_tone(CronHealth::Failed), Tone::Danger);
        assert_eq!(cron_tone(CronHealth::Missing), Tone::Warn);
        assert_eq!(cron_tone(CronHealth::Pending), Tone::Neutral);

        let run = |status, reason| CronRun {
            started_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            status,
            duration: None,
            reason,
        };
        assert_eq!(cron_run_tone(&run(CronStatus::Succeeded, None)), Tone::Ok);
        assert_eq!(cron_run_tone(&run(CronStatus::Running, None)), Tone::Active);
        assert_eq!(cron_run_tone(&run(CronStatus::Failed, None)), Tone::Danger);
        // A synthesised missed/stuck placeholder renders grey regardless of its underlying status.
        assert_eq!(cron_run_tone(&run(CronStatus::Failed, Some(grey_api::CronRunReason::Missed))), Tone::Neutral);
        assert_eq!(cron_run_tone(&run(CronStatus::Failed, Some(grey_api::CronRunReason::Stuck))), Tone::Neutral);
    }
}
