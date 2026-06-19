use grey_api::{CronHealth, CronStatus};

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
/// an in-flight run is `running` (light-blue), and a failed run is `error` (red).
pub fn cron_run_class(status: CronStatus) -> &'static str {
    match status {
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

        assert_eq!(cron_run_class(CronStatus::Succeeded), "ok");
        assert_eq!(cron_run_class(CronStatus::Running), "running");
        assert_eq!(cron_run_class(CronStatus::Failed), "error");
    }
}
