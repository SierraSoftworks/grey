//! A cron check-in and the logic that folds it into a [`grey_api::Cron`] record.
//!
//! The API `Cron` type is a DTO with the derived-health queries the UI reads; the *mutation* of
//! applying a check-in lives here in the agent, mirroring how [`crate::result::ProbeResult`] applies a
//! probe sample to a `grey_api::Probe`.

use chrono::{DateTime, Utc};
use grey_api::{CheckIn, Cron, CronRun, CronStatus, MAX_RUNS};

/// A check-in reported by a scheduled job: the status it is reporting, an optional message, and the
/// (server-stamped) time it was received.
#[derive(Debug, Clone)]
pub struct CronCheckin {
    pub status: CronStatus,
    pub message: String,
    pub at: DateTime<Utc>,
}

impl CronCheckin {
    pub fn new(status: CronStatus, message: String, at: DateTime<Utc>) -> Self {
        Self {
            status,
            message,
            at,
        }
    }

    /// Folds this check-in into a cron record. A `running` check-in opens a new run (or, if one is
    /// already in flight, simply refreshes the heartbeat); a terminal check-in closes the in-flight
    /// run with its duration, or — for jobs that only report on completion — records an instantaneous
    /// run with an unknown duration.
    pub fn apply(&self, cron: &mut Cron) {
        // Record at millisecond precision (what the markers survive serialization with) so an
        // in-memory run and its gossiped copy compare equal in the run-set merge.
        let at = DateTime::from_timestamp_millis(self.at.timestamp_millis()).unwrap_or(self.at);

        match self.status {
            CronStatus::Running => {
                if !has_in_flight(cron) {
                    push_run(
                        cron,
                        CronRun {
                            started_at: at,
                            status: CronStatus::Running,
                            duration: None,
                        },
                    );
                }
                // Otherwise this is a heartbeat for the run already in flight: it advances
                // `last_checkin` without opening a new run.
            }
            CronStatus::Succeeded | CronStatus::Failed => {
                if let Some(run) = in_flight_mut(cron) {
                    let duration = (at - run.started_at).to_std().unwrap_or_default();
                    run.status = self.status;
                    run.duration = Some(duration);
                } else {
                    push_run(
                        cron,
                        CronRun {
                            started_at: at,
                            status: self.status,
                            duration: None,
                        },
                    );
                }
            }
        }

        cron.last_checkin = Some(CheckIn {
            at,
            status: self.status,
            message: self.message.clone(),
        });
        cron.last_updated = cron.last_updated.max(at);
    }
}

fn has_in_flight(cron: &Cron) -> bool {
    cron.runs.last().map(CronRun::is_in_flight).unwrap_or(false)
}

fn in_flight_mut(cron: &mut Cron) -> Option<&mut CronRun> {
    cron.runs.last_mut().filter(|run| run.is_in_flight())
}

/// Appends a run, keeping `runs` sorted oldest-first and bounded to the most recent [`MAX_RUNS`].
fn push_run(cron: &mut Cron, run: CronRun) {
    cron.runs.push(run);
    cron.runs.sort_by_key(|r| r.started_at);
    if cron.runs.len() > MAX_RUNS {
        let excess = cron.runs.len() - MAX_RUNS;
        cron.runs.drain(0..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::CronSchedule;
    use std::collections::HashMap;
    use std::time::Duration;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn cron() -> Cron {
        Cron::from_config(
            "job",
            HashMap::new(),
            CronSchedule::Every(Duration::from_secs(60)),
            None,
            None,
        )
    }

    fn checkin(status: CronStatus, message: &str, at: i64) -> CronCheckin {
        CronCheckin::new(status, message.into(), ts(at))
    }

    #[test]
    fn running_then_terminal_records_one_run_with_duration() {
        let mut c = cron();
        checkin(CronStatus::Running, "", 100).apply(&mut c);
        assert_eq!(c.runs.len(), 1);
        assert!(c.runs[0].is_in_flight());

        // A heartbeat while in flight does not open a second run.
        checkin(CronStatus::Running, "", 110).apply(&mut c);
        assert_eq!(c.runs.len(), 1);

        checkin(CronStatus::Succeeded, "ok", 130).apply(&mut c);
        assert_eq!(c.runs.len(), 1);
        assert_eq!(c.runs[0].status, CronStatus::Succeeded);
        assert_eq!(c.runs[0].duration, Some(Duration::from_secs(30)));
        assert_eq!(c.last_checkin.as_ref().unwrap().message, "ok");
        assert_eq!(c.last_updated, ts(130));
    }

    #[test]
    fn completion_only_job_records_instantaneous_runs_with_unknown_duration() {
        let mut c = cron();
        checkin(CronStatus::Succeeded, "", 100).apply(&mut c);
        checkin(CronStatus::Succeeded, "", 160).apply(&mut c);
        assert_eq!(c.runs.len(), 2);
        assert!(c.runs.iter().all(|r| r.duration.is_none()));
    }

    #[test]
    fn apply_bounds_runs_to_max() {
        let mut c = cron();
        for i in 0..(MAX_RUNS as i64 + 10) {
            checkin(CronStatus::Succeeded, "", 1_000 + i * 60).apply(&mut c);
        }
        assert_eq!(c.runs.len(), MAX_RUNS);
        assert_eq!(c.runs.last().unwrap().started_at, ts(1_000 + (MAX_RUNS as i64 + 9) * 60));
    }
}
