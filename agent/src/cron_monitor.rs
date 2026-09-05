//! A background daemon that materialises *time-derived* cron faults — a run that never started
//! (`missing`, the deadman-switch case) or one that overran its `max_duration` (`stuck`) — into
//! persisted state.
//!
//! Unlike a probe sample or a cron check-in, these faults are driven purely by the passage of time,
//! so nothing writes them to the store on its own. This monitor re-derives each configured cron's
//! schedule/completion detectors on a fixed cadence and, when it finds a *new* fault, records a
//! synthetic run (a [`grey_api::CronRunReason`]-tagged placeholder) plus a failing streak
//! observation and a `last_checkin`. That has two effects: the fault surfaces as a run placeholder in
//! the UI (rendered distinctly, "grey", rather than leaving a silent gap), and it progresses the same
//! [`grey_api::Streak`] that terminal check-ins do — so the streak is the single health interface the
//! notifier debounces and alerts on, for missed/stuck runs exactly as for reported failures.
//!
//! Detections are idempotent. A missed slot advances the record's last-run time to the slot it
//! materialises, so the schedule detector only fires again once the *next* slot is genuinely overdue
//! (one placeholder per missed occurrence, not one per evaluation). A stuck run is marked in place, so
//! the overrun is recorded once rather than re-appended.

use std::time::Duration;

use chrono::Utc;
use grey_api::{CronHealth, CronRunReason};
use tracing_batteries::prelude::*;

use crate::state::{CronStore, State};

/// How often the monitor re-derives cron state to look for missed/stuck runs. Crons are not expected
/// to run frequently, so a relatively long cadence keeps the store quiet while still materialising a
/// fault within a few minutes of its deadline.
const EVALUATION_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Watches configured crons for time-derived faults and persists them as run placeholders.
pub struct CronMonitor {
    state: State,
}

impl CronMonitor {
    pub fn new(state: State) -> Self {
        Self { state }
    }

    /// Runs the evaluation loop forever.
    pub async fn run(self) {
        loop {
            if let Err(e) = self.evaluate().await {
                warn!(name: "cron.monitor.evaluate", { exception = %e }, "Failed to evaluate cron monitor state.");
            }
            tokio::time::sleep(EVALUATION_INTERVAL).await;
        }
    }

    /// Performs one evaluation pass over every pooled cron, materialising any newly detected missed or
    /// stuck run.
    async fn evaluate(&self) -> Result<(), Box<dyn std::error::Error>> {
        let now = Utc::now();
        let crons = self.state.get_cron_states().await?;
        let evaluated = crons.len();
        let mut detected = 0usize;

        for (name, cron) in crons {
            // An in-flight run that has overrun its `max_duration` is stuck. Mark it once (a marked
            // run no longer reads as in-flight, so this won't re-fire), taking precedence over the
            // schedule detector: a job that is overrunning hasn't *missed* its next slot, it's hung.
            let already_stuck = cron
                .runs
                .last()
                .map(|run| run.reason == Some(CronRunReason::Stuck))
                .unwrap_or(false);

            if cron.completion_overdue(now) && !already_stuck {
                let at = cron.since(CronHealth::Stuck).unwrap_or(now);
                if self
                    .state
                    .record_cron_detection(&name, CronRunReason::Stuck, at)
                    .await?
                {
                    detected += 1;
                }
                continue;
            }

            // A run that was due but never started (past the schedule grace) is missing. The slot's
            // due time anchors the placeholder, so successive ticks advance to — and only fire on —
            // the next genuinely-overdue slot.
            if cron.schedule_overdue(now) {
                let Some(due) = cron.next_due() else {
                    continue;
                };
                if self
                    .state
                    .record_cron_detection(&name, CronRunReason::Missed, due)
                    .await?
                {
                    detected += 1;
                }
            }
        }

        debug!(
            name: "cron.monitor.pass",
            { crons.evaluated = evaluated, crons.detected = detected },
            "Completed cron monitor pass.",
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::config::CronConfig;

    async fn state_with_cron(dir: &std::path::Path, cfg: CronConfig) -> State {
        let state = State::test(dir.to_path_buf()).await;
        let mut config = Config::test(&dir.to_path_buf());
        config.crons = vec![cfg];
        state.set_config_for_test(config);
        state
    }

    fn cron_config(name: &str, interval_secs: u64) -> CronConfig {
        CronConfig {
            name: name.into(),
            interval: Some(Duration::from_secs(interval_secs)),
            schedule: None,
            max_duration: None,
            grace: Some(Duration::from_secs(1)),
            token: None,
            tags: Default::default(),
            visible: crate::config::default_visible_filter(),
            alerting: Default::default(),
        }
    }

    /// A cron whose scheduled run never arrives is materialised as a `missing` run placeholder, with a
    /// failing streak observation — so it both shows in the UI and drives alerting.
    #[tokio::test]
    async fn materialises_a_missed_run() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_cron(dir.path(), cron_config("backup", 60)).await;

        // A completed run well in the past, so the next slot is long overdue.
        state
            .record_cron_checkin(
                "backup",
                crate::cron::CronCheckin::new(
                    grey_api::CronStatus::Succeeded,
                    "ok".into(),
                    Utc::now() - chrono::Duration::hours(1),
                ),
            )
            .await
            .unwrap();

        let monitor = CronMonitor::new(state.clone());
        monitor.evaluate().await.unwrap();

        let cron = state.get_cron_states().await.unwrap().remove("backup").unwrap();
        let placeholder = cron.runs.last().expect("a placeholder run should be recorded");
        assert_eq!(placeholder.reason, Some(CronRunReason::Missed));
        assert!(
            !cron.streak.is_empty() && cron.streak.failing_since.is_some(),
            "the missed run must progress the streak as a failure"
        );
    }

    /// A second evaluation immediately after does not record a duplicate placeholder for the same
    /// slot — the materialised run advances the schedule so the detector waits for the next slot.
    #[tokio::test]
    async fn missed_run_detection_is_idempotent_per_slot() {
        let dir = tempfile::tempdir().unwrap();
        // A long interval so, once the first slot is materialised, the next slot is not yet overdue.
        let state = state_with_cron(dir.path(), cron_config("backup", 3600)).await;
        state
            .record_cron_checkin(
                "backup",
                crate::cron::CronCheckin::new(
                    grey_api::CronStatus::Succeeded,
                    "ok".into(),
                    Utc::now() - chrono::Duration::hours(2),
                ),
            )
            .await
            .unwrap();

        let monitor = CronMonitor::new(state.clone());
        monitor.evaluate().await.unwrap();
        monitor.evaluate().await.unwrap();

        let cron = state.get_cron_states().await.unwrap().remove("backup").unwrap();
        let missed = cron
            .runs
            .iter()
            .filter(|r| r.reason == Some(CronRunReason::Missed))
            .count();
        assert_eq!(missed, 1, "only one placeholder per missed slot");
    }

    /// An in-flight run that overruns its `max_duration` is marked stuck once, in place.
    #[tokio::test]
    async fn marks_an_overrunning_run_stuck() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = cron_config("job", 3600);
        cfg.max_duration = Some(Duration::from_secs(1));
        let state = state_with_cron(dir.path(), cfg).await;

        // A run that started running two minutes ago and never completed.
        state
            .record_cron_checkin(
                "job",
                crate::cron::CronCheckin::new(
                    grey_api::CronStatus::Running,
                    "start".into(),
                    Utc::now() - chrono::Duration::minutes(2),
                ),
            )
            .await
            .unwrap();

        let monitor = CronMonitor::new(state.clone());
        monitor.evaluate().await.unwrap();
        monitor.evaluate().await.unwrap();

        let cron = state.get_cron_states().await.unwrap().remove("job").unwrap();
        let stuck = cron
            .runs
            .iter()
            .filter(|r| r.reason == Some(CronRunReason::Stuck))
            .count();
        assert_eq!(stuck, 1, "the overrun is marked once, in place");
        assert!(cron.streak.failing_since.is_some(), "a stuck run progresses the streak");
    }
}
