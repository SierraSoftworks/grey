use grey_api::{CronHealth, CronStatus};

/// The colour class for a cron's derived health: passing states are `ok`, an overrunning run is the
/// intermediate `warn`, a missed or failed run is `error`, and a never-seen cron is `unknown`.
pub fn cron_class(health: CronHealth) -> &'static str {
    match health {
        CronHealth::Succeeded | CronHealth::Running => "ok",
        CronHealth::Stuck => "warn",
        CronHealth::Failed | CronHealth::Missing => "error",
        CronHealth::Pending => "unknown",
    }
}

/// The colour class for a single run cell in the recent-runs strip.
pub fn cron_run_class(status: CronStatus) -> &'static str {
    match status {
        CronStatus::Succeeded => "ok",
        CronStatus::Running => "warn",
        CronStatus::Failed => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_map_every_variant() {
        assert_eq!(cron_class(CronHealth::Succeeded), "ok");
        assert_eq!(cron_class(CronHealth::Running), "ok");
        assert_eq!(cron_class(CronHealth::Stuck), "warn");
        assert_eq!(cron_class(CronHealth::Failed), "error");
        assert_eq!(cron_class(CronHealth::Missing), "error");
        assert_eq!(cron_class(CronHealth::Pending), "unknown");

        assert_eq!(cron_run_class(CronStatus::Succeeded), "ok");
        assert_eq!(cron_run_class(CronStatus::Running), "warn");
        assert_eq!(cron_run_class(CronStatus::Failed), "error");
    }
}
