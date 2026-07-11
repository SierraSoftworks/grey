use grey_api::{CronHealth, CronRun, CronStatus};

/// The colour class for a cron's derived health: a healthy run is `ok` (green), an in-flight run is
/// `running` (light-blue), an overdue or overrunning run is `warn` (orange), a failed run is `error`
/// (red), and a never-seen cron is `unknown`.
pub fn cron_class(health: CronHealth) -> &'static str {
    match health {
        CronHealth::Succeeded => "ok",
        CronHealth::Running => "running",
        CronHealth::Missing | CronHealth::Stuck => "warn",
        CronHealth::Failed => "error",
        CronHealth::Pending => "unknown",
    }
}

/// The colour class for a single run cell in the recent-runs strip: a successful run is `ok` (green),
/// an in-flight run is `running` (light-blue), and a failed run is `error` (red). A monitor-
/// synthesised placeholder for a missed/stuck run renders `unknown` (grey), so a detected fault is
/// visually distinct from a job-reported failure.
pub fn cron_run_class(run: &CronRun) -> &'static str {
    if run.reason.is_some() {
        return "unknown";
    }
    match run.status {
        CronStatus::Succeeded => "ok",
        CronStatus::Running => "running",
        CronStatus::Failed => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_map_every_variant() {
        assert_eq!(cron_class(CronHealth::Succeeded), "ok");
        assert_eq!(cron_class(CronHealth::Running), "running");
        assert_eq!(cron_class(CronHealth::Stuck), "warn");
        assert_eq!(cron_class(CronHealth::Failed), "error");
        assert_eq!(cron_class(CronHealth::Missing), "warn");
        assert_eq!(cron_class(CronHealth::Pending), "unknown");

        let run = |status, reason| CronRun {
            started_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            status,
            duration: None,
            reason,
        };
        assert_eq!(cron_run_class(&run(CronStatus::Succeeded, None)), "ok");
        assert_eq!(cron_run_class(&run(CronStatus::Running, None)), "running");
        assert_eq!(cron_run_class(&run(CronStatus::Failed, None)), "error");
        // A synthesised missed/stuck placeholder renders grey regardless of its underlying status.
        assert_eq!(cron_run_class(&run(CronStatus::Failed, Some(grey_api::CronRunReason::Missed))), "unknown");
        assert_eq!(cron_run_class(&run(CronStatus::Failed, Some(grey_api::CronRunReason::Stuck))), "unknown");
    }
}
