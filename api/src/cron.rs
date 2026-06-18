use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Mergeable;

/// How many recent runs are retained per cron. This bounded list is both the displayed history and
/// the input the detectors read (for the last run's start time).
pub const MAX_RUNS: usize = 20;

/// The grace applied to a crontab schedule when none is configured. For an `Every` schedule the
/// default is instead a tenth of the interval (see [`Cron::effective_grace`]).
const DEFAULT_CRON_GRACE: Duration = Duration::from_secs(5 * 60);

/// A cron's expected schedule: a fixed interval, or a standard crontab expression (evaluated in UTC).
/// Detection is deterministic — a run is due, and a late one flagged, relative to this declared
/// schedule rather than a learned cadence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CronSchedule {
    /// Runs every fixed interval — the simple "every N" form.
    Every(#[serde(with = "humantime_serde")] Duration),
    /// A standard 5-field crontab expression (`minute hour day month weekday`), evaluated in UTC.
    Cron(String),
}

impl CronSchedule {
    /// The next time a run is expected, strictly after `from`. For an interval that is `from +
    /// interval`; for a crontab it is the next matching wall-clock time. `None` if a crontab
    /// expression fails to parse (config load rejects those, so this is defensive).
    pub fn next_due_after(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            CronSchedule::Every(interval) => {
                Some(from + chrono::Duration::from_std(*interval).ok()?)
            }
            CronSchedule::Cron(expr) => expr
                .parse::<croner::Cron>()
                .ok()?
                .find_next_occurrence(&from, false)
                .ok(),
        }
    }

    /// Whether a crontab expression is valid — used to reject bad configuration at load time.
    pub fn is_valid(&self) -> bool {
        match self {
            CronSchedule::Every(_) => true,
            CronSchedule::Cron(expr) => expr.parse::<croner::Cron>().is_ok(),
        }
    }
}

/// The status a job reports for a run: it is `Running` while in flight and transitions to one of the
/// two terminal states when it finishes.
///
/// The variants are declared in **merge-precedence order** (`Running < Succeeded < Failed`), so the
/// derived [`Ord`] gives exactly the precedence run-set merging needs: a terminal status supersedes
/// `Running`, and a failure observed by any node supersedes a success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CronStatus {
    Running,
    Succeeded,
    Failed,
}

/// The derived, displayed health of a cron — what the UI renders "as if it were an active probe".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CronHealth {
    /// No check-in has ever been observed.
    Pending,
    /// A run is in flight and within its expected duration.
    Running,
    /// The most recent run finished successfully and the next run is not yet due.
    Succeeded,
    /// The most recent run reported a failure.
    Failed,
    /// No run has started within the expected schedule — a run was missed.
    Missing,
    /// A run started but has not reported completion within its `max_duration` — it is hung.
    Stuck,
}

impl CronHealth {
    /// Whether this health reads as passing for status-page purposes. `Pending` (awaiting the first
    /// check-in) and `Running` (an on-time in-flight run) are treated as passing, not as faults.
    pub fn passing(self) -> bool {
        matches!(
            self,
            CronHealth::Pending | CronHealth::Running | CronHealth::Succeeded
        )
    }

    /// A short human-readable label for display.
    pub fn label(self) -> &'static str {
        match self {
            CronHealth::Pending => "Awaiting check-in",
            CronHealth::Running => "Running",
            CronHealth::Succeeded => "Healthy",
            CronHealth::Failed => "Failed",
            CronHealth::Missing => "Missed run",
            CronHealth::Stuck => "Overrunning",
        }
    }

    /// A lowercase token suitable for a CSS class or serialized value.
    pub fn as_str(self) -> &'static str {
        match self {
            CronHealth::Pending => "pending",
            CronHealth::Running => "running",
            CronHealth::Succeeded => "succeeded",
            CronHealth::Failed => "failed",
            CronHealth::Missing => "missing",
            CronHealth::Stuck => "stuck",
        }
    }
}

/// One observed run of a cron job, identified by its start time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CronRun {
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    pub status: CronStatus,
    /// The run's duration, set once a terminal check-in arrives.
    #[serde(default, with = "humantime_serde::option")]
    pub duration: Option<Duration>,
}

impl CronRun {
    /// Whether this run is still in flight (reported `running`, no terminal status yet).
    pub fn is_in_flight(&self) -> bool {
        self.status == CronStatus::Running
    }
}

/// The most recent check-in, retained verbatim for display (the reported status and its message).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckIn {
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub at: DateTime<Utc>,
    pub status: CronStatus,
    #[serde(default)]
    pub message: String,
}

/// A cluster-converged record of a scheduled job's check-ins, surfaced like an active probe.
///
/// This is a DTO: it carries the replicated state and the derived-health *queries* the UI reads, plus
/// the CRDT [`Cron::merge`] used to converge it. The *mutation* that folds an incoming check-in into a
/// record lives in the agent (`crate::cron::CronCheckin`), mirroring how `grey_api::Probe` is a DTO
/// while `agent::result::ProbeResult` applies a sample.
///
/// All replicated state is a join-semilattice, so gossip merges, storage round-trips and display
/// pooling all use the one [`Cron::merge`] and converge regardless of order: `runs` is a bounded
/// union keyed by start time, and `last_checkin` is a last-write-wins register keyed by time. The
/// `schedule` / `max_duration` / `grace` fields are configuration echoed onto the record for display
/// and detection; they are re-stamped from local config after a merge (see the store) rather than
/// treated as authoritative cluster state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cron {
    pub name: String,

    #[serde(default)]
    pub tags: HashMap<String, String>,

    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub last_updated: DateTime<Utc>,

    /// The declared schedule: an interval or a crontab expression.
    pub schedule: CronSchedule,

    /// How long a run may stay in flight before it reads as `Stuck`. When unset, completion/timeout
    /// detection is disabled (a run is never declared hung).
    #[serde(default, with = "humantime_serde::option")]
    pub max_duration: Option<Duration>,

    /// Slack after the next-due time before a late run reads `Missing`. When unset, a
    /// schedule-derived default applies (see [`Cron::effective_grace`]).
    #[serde(default, with = "humantime_serde::option")]
    pub grace: Option<Duration>,

    /// The most recent runs (bounded to [`MAX_RUNS`]), oldest first.
    #[serde(default)]
    pub runs: Vec<CronRun>,

    /// The latest check-in, for displaying the reported status and message.
    #[serde(default)]
    pub last_checkin: Option<CheckIn>,
}

impl Cron {
    /// A bare record carrying only the configured fields, used to seed the pooled view before any
    /// check-in has arrived (and as the template the stored state is merged into).
    pub fn from_config(
        name: impl Into<String>,
        tags: HashMap<String, String>,
        schedule: CronSchedule,
        max_duration: Option<Duration>,
        grace: Option<Duration>,
    ) -> Self {
        Self {
            name: name.into(),
            tags,
            last_updated: DateTime::UNIX_EPOCH,
            schedule,
            max_duration,
            grace,
            runs: Vec::new(),
            last_checkin: None,
        }
    }

    // --- Run-list maintenance --------------------------------------------------------------------

    /// Appends a run, keeping `runs` sorted oldest-first and bounded to the most recent [`MAX_RUNS`].
    /// The agent's check-in folding (`CronCheckin::apply`) decides *when* to open a run; this owns the
    /// "how the list is bounded" invariant alongside [`Cron::merge`].
    pub fn push_run(&mut self, run: CronRun) {
        self.runs.push(run);
        self.trim_runs();
    }

    /// Whether the most recent run is still in flight (reported `running`, with no terminal status
    /// yet) — i.e. a further `running` check-in is a heartbeat rather than a new run.
    pub fn has_in_flight(&self) -> bool {
        self.runs.last().map(CronRun::is_in_flight).unwrap_or(false)
    }

    /// A mutable reference to the in-flight run (the latest run, while it is still running), for a
    /// terminal check-in to close out with its status and duration.
    pub fn in_flight_mut(&mut self) -> Option<&mut CronRun> {
        self.runs.last_mut().filter(|run| run.is_in_flight())
    }

    /// Keeps `runs` sorted oldest-first and bounded to the most recent [`MAX_RUNS`].
    fn trim_runs(&mut self) {
        self.runs.sort_by_key(|run| run.started_at);
        if self.runs.len() > MAX_RUNS {
            let excess = self.runs.len() - MAX_RUNS;
            self.runs.drain(0..excess);
        }
    }

    /// Merges another record's run set into this one: a union keyed by `started_at` (so the same run
    /// observed by two nodes collapses into one), taking the higher-precedence status and bounded to
    /// the most recent [`MAX_RUNS`].
    fn merge_runs(&mut self, other: &[CronRun]) {
        for run in other {
            if let Some(existing) = self
                .runs
                .iter_mut()
                .find(|r| r.started_at == run.started_at)
            {
                if run.status > existing.status {
                    existing.status = run.status;
                    existing.duration = run.duration.or(existing.duration);
                } else if run.status == existing.status {
                    existing.duration = existing.duration.or(run.duration);
                }
            } else {
                self.runs.push(run.clone());
            }
        }
        self.trim_runs();
    }

    // --- Detection (deterministic, derived queries) ----------------------------------------------

    /// The start time of the most recent run.
    pub fn last_start(&self) -> Option<DateTime<Utc>> {
        self.runs.last().map(|run| run.started_at)
    }

    /// The grace applied before a late run reads `Missing`: the configured value, or a
    /// schedule-derived default (a tenth of an `Every` interval, or [`DEFAULT_CRON_GRACE`] for a
    /// crontab schedule).
    fn effective_grace(&self) -> Duration {
        self.grace.unwrap_or_else(|| match &self.schedule {
            CronSchedule::Every(interval) => *interval / 10,
            CronSchedule::Cron(_) => DEFAULT_CRON_GRACE,
        })
    }

    /// When the next run is due after the most recent one started.
    pub fn next_due(&self) -> Option<DateTime<Utc>> {
        self.schedule.next_due_after(self.last_start()?)
    }

    /// The deadline after which a not-yet-started run reads `Missing`: the next-due time plus grace.
    fn schedule_deadline(&self) -> Option<DateTime<Utc>> {
        let due = self.next_due()?;
        Some(due + chrono::Duration::from_std(self.effective_grace()).unwrap_or_default())
    }

    /// The deadline after which an in-flight run reads `Stuck`: its start plus `max_duration`.
    /// `None` when nothing is in flight or no `max_duration` is configured.
    fn completion_deadline(&self) -> Option<DateTime<Utc>> {
        let run = self.runs.last()?;
        if !run.is_in_flight() {
            return None;
        }
        let max = self.max_duration?;
        Some(run.started_at + chrono::Duration::from_std(max).unwrap_or_default())
    }

    /// Whether a run is overdue to start (its `Missing` deadline has passed).
    pub fn schedule_overdue(&self, now: DateTime<Utc>) -> bool {
        self.schedule_deadline().map(|d| now > d).unwrap_or(false)
    }

    /// Whether an in-flight run has exceeded its `max_duration`.
    pub fn completion_overdue(&self, now: DateTime<Utc>) -> bool {
        self.completion_deadline().map(|d| now > d).unwrap_or(false)
    }

    /// The cron's displayed health at `now`.
    pub fn health(&self, now: DateTime<Utc>) -> CronHealth {
        let Some(latest) = self.runs.last() else {
            return CronHealth::Pending;
        };

        if latest.status == CronStatus::Failed {
            return CronHealth::Failed;
        }
        if self.schedule_overdue(now) {
            return CronHealth::Missing;
        }
        if self.completion_overdue(now) {
            return CronHealth::Stuck;
        }
        match latest.status {
            CronStatus::Running => CronHealth::Running,
            CronStatus::Succeeded => CronHealth::Succeeded,
            CronStatus::Failed => CronHealth::Failed,
        }
    }

    /// Whether the cron currently reads as passing.
    pub fn passing(&self, now: DateTime<Utc>) -> bool {
        self.health(now).passing()
    }

    /// When the current health state was entered, computed analytically (no sampling loop or streak
    /// register). Takes the already-computed [`Cron::health`] so the state machine — and any crontab
    /// parse it performs — isn't evaluated a second time.
    pub fn since(&self, health: CronHealth) -> Option<DateTime<Utc>> {
        match health {
            CronHealth::Pending => None,
            CronHealth::Failed => self.runs.last().map(|run| {
                run.started_at
                    + chrono::Duration::from_std(run.duration.unwrap_or_default()).unwrap_or_default()
            }),
            // The moment the deadline passed.
            CronHealth::Missing => self.schedule_deadline(),
            CronHealth::Stuck => self.completion_deadline(),
            // Passing since the latest run started (the most recent on-time milestone).
            CronHealth::Running | CronHealth::Succeeded => self.last_start(),
        }
    }
}

impl Mergeable for Cron {
    fn merge(&mut self, other: &Self) {
        // Identity/labels follow the most recently updated record; configuration echo fields are
        // intentionally left to the local copy (the store re-stamps them from local config).
        if other.last_updated > self.last_updated {
            self.name = other.name.clone();
            self.tags = other.tags.clone();
        }
        self.last_updated = self.last_updated.max(other.last_updated);
        self.merge_runs(&other.runs);
        self.last_checkin = match (self.last_checkin.take(), other.last_checkin.clone()) {
            (Some(mine), Some(theirs)) => Some(if theirs.at > mine.at { theirs } else { mine }),
            (mine, theirs) => mine.or(theirs),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn every(secs: u64) -> CronSchedule {
        CronSchedule::Every(Duration::from_secs(secs))
    }

    fn run(start: i64, status: CronStatus, dur: Option<u64>) -> CronRun {
        CronRun {
            started_at: ts(start),
            status,
            duration: dur.map(Duration::from_secs),
        }
    }

    /// Builds a cron with the given schedule and runs (sorted), plus optional `max_duration`/`grace`.
    fn cron_with(
        schedule: CronSchedule,
        runs: Vec<CronRun>,
        max_duration: Option<Duration>,
        grace: Option<Duration>,
    ) -> Cron {
        let mut c = Cron::from_config("job", HashMap::new(), schedule, max_duration, grace);
        c.runs = runs;
        c.runs.sort_by_key(|r| r.started_at);
        c
    }

    #[test]
    fn cold_start_is_pending() {
        let c = Cron::from_config("c", HashMap::new(), every(60), None, None);
        assert_eq!(c.health(ts(10_000)), CronHealth::Pending);
        assert!(c.passing(ts(10_000)));
        assert!(!c.schedule_overdue(ts(10_000)));
        assert!(!c.completion_overdue(ts(10_000)));
        assert_eq!(c.since(c.health(ts(10_000))), None);
    }

    #[test]
    fn a_failed_latest_run_reads_failed() {
        let c = cron_with(every(60), vec![run(100, CronStatus::Failed, Some(0))], None, None);
        assert_eq!(c.health(ts(101)), CronHealth::Failed);
        assert!(!c.passing(ts(101)));

        // A newer run that is merely running reads as running, not the previous failure.
        let c = cron_with(
            every(60),
            vec![run(100, CronStatus::Failed, Some(0)), run(160, CronStatus::Running, None)],
            None,
            None,
        );
        assert_eq!(c.health(ts(161)), CronHealth::Running);
    }

    #[test]
    fn schedule_detector_flags_after_interval_plus_grace() {
        // interval 60s, default grace = 6s ⇒ deadline at last_start + 66s.
        let c = cron_with(every(60), vec![run(1_000, CronStatus::Succeeded, Some(5))], None, None);
        assert_eq!(c.health(ts(1_065)), CronHealth::Succeeded);
        assert!(c.passing(ts(1_065)));
        assert_eq!(c.health(ts(1_067)), CronHealth::Missing);
        assert!(!c.passing(ts(1_067)));
    }

    #[test]
    fn schedule_detector_judges_against_the_declared_interval_not_observed_cadence() {
        // Runs actually arrive every 90s, but the configured interval is 60s; a 67s gap past the
        // last run is still missing — the case an adaptive learned mean would have masked.
        let runs = (0..5)
            .map(|i| run(1_000 + i * 90, CronStatus::Succeeded, Some(5)))
            .collect();
        let c = cron_with(every(60), runs, None, None);
        let last = 1_000 + 4 * 90;
        assert_eq!(c.health(ts(last + 67)), CronHealth::Missing);
    }

    #[test]
    fn completion_detector_needs_max_duration() {
        // No max_duration: an in-flight run is never stuck (a long interval keeps the schedule
        // detector quiet so we isolate completion detection).
        let c = cron_with(every(3600), vec![run(1_000, CronStatus::Running, None)], None, None);
        assert_eq!(c.health(ts(2_000)), CronHealth::Running);

        // With max_duration, it goes stuck once exceeded, and clears once it completes.
        let c = cron_with(
            every(3600),
            vec![run(1_000, CronStatus::Running, None)],
            Some(Duration::from_secs(60)),
            None,
        );
        assert_eq!(c.health(ts(1_055)), CronHealth::Running);
        assert_eq!(c.health(ts(1_061)), CronHealth::Stuck);

        let c = cron_with(
            every(3600),
            vec![run(1_000, CronStatus::Succeeded, Some(65))],
            Some(Duration::from_secs(60)),
            None,
        );
        assert_eq!(c.health(ts(1_070)), CronHealth::Succeeded);
    }

    #[test]
    fn missing_since_is_the_deadline() {
        let c = cron_with(every(60), vec![run(1_000, CronStatus::Succeeded, Some(5))], None, None);
        let now = ts(2_000);
        assert_eq!(c.health(now), CronHealth::Missing);
        // last_start + interval + grace = 1000 + 60 + 6.
        assert_eq!(c.since(c.health(now)), Some(ts(1_066)));
    }

    #[test]
    fn since_reports_failure_and_stuck_onsets() {
        // A failed terminal run reads as failing since the run finished (start + duration).
        let c = cron_with(every(60), vec![run(1_000, CronStatus::Failed, Some(5))], None, None);
        assert_eq!(c.health(ts(1_006)), CronHealth::Failed);
        assert_eq!(c.since(c.health(ts(1_006))), Some(ts(1_005)));

        // A stuck in-flight run reads as overrunning since start + max_duration.
        let c = cron_with(
            every(3600),
            vec![run(1_000, CronStatus::Running, None)],
            Some(Duration::from_secs(60)),
            None,
        );
        let now = ts(2_000);
        assert_eq!(c.health(now), CronHealth::Stuck);
        assert_eq!(c.since(c.health(now)), Some(ts(1_060)));
    }

    #[test]
    fn next_due_is_one_interval_after_the_last_run() {
        let c = cron_with(every(60), vec![run(1_000, CronStatus::Succeeded, Some(5))], None, None);
        assert_eq!(c.next_due(), Some(ts(1_060)));

        // With no runs there is no next-due time.
        let empty = Cron::from_config("c", HashMap::new(), every(60), None, None);
        assert_eq!(empty.next_due(), None);
    }

    #[test]
    fn cron_health_labels_and_tokens_cover_every_variant() {
        let all = [
            CronHealth::Pending,
            CronHealth::Running,
            CronHealth::Succeeded,
            CronHealth::Failed,
            CronHealth::Missing,
            CronHealth::Stuck,
        ];
        for health in all {
            assert!(!health.label().is_empty());
            assert!(!health.as_str().is_empty());
        }
        assert!(CronHealth::Pending.passing());
        assert!(CronHealth::Running.passing());
        assert!(!CronHealth::Missing.passing());
        assert!(!CronHealth::Stuck.passing());
        assert_eq!(CronHealth::Stuck.label(), "Overrunning");
        assert_eq!(CronHealth::Missing.as_str(), "missing");
    }

    #[test]
    fn crontab_schedule_detects_a_missed_minute() {
        // "every minute"; default grace for a crontab is 5 minutes.
        let start = DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z").unwrap().with_timezone(&Utc);
        let c = cron_with(
            CronSchedule::Cron("* * * * *".into()),
            vec![CronRun { started_at: start, status: CronStatus::Succeeded, duration: None }],
            None,
            None,
        );

        // Next due 12:01:00; deadline 12:06:00 (5m grace).
        let before = DateTime::parse_from_rfc3339("2026-01-01T12:05:00Z").unwrap().with_timezone(&Utc);
        assert_eq!(c.health(before), CronHealth::Succeeded);
        let after = DateTime::parse_from_rfc3339("2026-01-01T12:06:30Z").unwrap().with_timezone(&Utc);
        assert_eq!(c.health(after), CronHealth::Missing);
    }

    #[test]
    fn invalid_crontab_is_detectable() {
        assert!(every(60).is_valid());
        assert!(CronSchedule::Cron("* * * * *".into()).is_valid());
        assert!(!CronSchedule::Cron("not a cron".into()).is_valid());
    }

    fn merge(a: &Cron, b: &Cron) -> Cron {
        let mut j = a.clone();
        j.merge(b);
        j
    }

    fn with_runs(runs: Vec<CronRun>, last_updated: i64, checkin: Option<CheckIn>) -> Cron {
        let mut c = cron_with(every(60), runs, None, None);
        c.last_updated = ts(last_updated);
        c.last_checkin = checkin;
        c
    }

    /// The merge must be idempotent, commutative and associative so every node converges on the same
    /// record regardless of the order gossip delivers updates.
    #[test]
    fn merge_is_a_semilattice() {
        let registers = [
            with_runs(vec![], 0, None),
            with_runs(vec![run(100, CronStatus::Running, None)], 100, None),
            with_runs(vec![run(100, CronStatus::Succeeded, Some(10))], 110, None),
            with_runs(
                vec![run(100, CronStatus::Failed, Some(5)), run(200, CronStatus::Succeeded, Some(8))],
                208,
                Some(CheckIn { at: ts(208), status: CronStatus::Succeeded, message: "x".into() }),
            ),
        ];

        for a in &registers {
            assert_eq!(merge(a, a), *a, "idempotent: {a:?}");
            for b in &registers {
                assert_eq!(merge(a, b), merge(b, a), "commutative");
                for c in &registers {
                    assert_eq!(merge(&merge(a, b), c), merge(a, &merge(b, c)), "associative");
                }
            }
        }
    }

    #[test]
    fn merge_takes_terminal_status_for_the_same_run() {
        // Node A saw the run start; node B saw it fail. Merging either way yields the failure.
        let a = with_runs(vec![run(100, CronStatus::Running, None)], 100, None);
        let b = with_runs(vec![run(100, CronStatus::Failed, Some(7))], 107, None);

        let ab = merge(&a, &b);
        assert_eq!(ab, merge(&b, &a));
        assert_eq!(ab.runs.len(), 1);
        assert_eq!(ab.runs[0].status, CronStatus::Failed);
        assert_eq!(ab.runs[0].duration, Some(Duration::from_secs(7)));
    }

    #[test]
    fn merge_keeps_latest_checkin() {
        let a = with_runs(vec![], 100, Some(CheckIn { at: ts(100), status: CronStatus::Running, message: "a".into() }));
        let b = with_runs(vec![], 200, Some(CheckIn { at: ts(200), status: CronStatus::Succeeded, message: "b".into() }));
        assert_eq!(merge(&a, &b).last_checkin.unwrap().message, "b");
        assert_eq!(merge(&b, &a).last_checkin.unwrap().message, "b");
    }

    #[test]
    fn merge_bounds_runs() {
        let many: Vec<CronRun> = (0..(MAX_RUNS as i64 + 10))
            .map(|i| run(1_000 + i * 60, CronStatus::Succeeded, Some(1)))
            .collect();
        let a = with_runs(many, 99_999, None);
        let merged = merge(&a, &with_runs(vec![], 0, None));
        assert_eq!(merged.runs.len(), MAX_RUNS);
        assert_eq!(merged.runs.last().unwrap().started_at, ts(1_000 + (MAX_RUNS as i64 + 9) * 60));
    }

    #[test]
    fn msgpack_roundtrip() {
        for schedule in [every(86400), CronSchedule::Cron("0 2 * * *".into())] {
            let c = cron_with(
                schedule,
                vec![run(100, CronStatus::Succeeded, Some(30))],
                Some(Duration::from_secs(60)),
                None,
            );

            let packed = rmp_serde::to_vec(&c).unwrap();
            assert_eq!(c, rmp_serde::from_slice::<Cron>(&packed).unwrap());

            let packed = rmp_serde::to_vec_named(&c).unwrap();
            assert_eq!(c, rmp_serde::from_slice::<Cron>(&packed).unwrap());
        }
    }

    #[test]
    fn json_uses_lowercase_status_and_schedule() {
        let mut c = cron_with(every(60), vec![run(100, CronStatus::Succeeded, Some(5))], None, None);
        c.last_checkin = Some(CheckIn { at: ts(100), status: CronStatus::Succeeded, message: "ok".into() });
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"succeeded\""), "{json}");
        assert!(json.contains("\"every\""), "schedule tag is lowercase: {json}");
    }
}
