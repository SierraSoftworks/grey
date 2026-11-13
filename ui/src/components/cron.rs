use std::rc::Rc;

use super::{detail, probe::tags};
use crate::formatters::compact_duration;
use crate::tones::{cron_run_tone, cron_tone};
use grey_api::{CronHealth, CronRun, CronRunReason, CronSchedule, CronStatus};
use yew::prelude::*;
use yewi::prelude::*;

#[derive(Properties, PartialEq)]
pub struct CronProps {
    pub cron: grey_api::Cron,
}

/// A single cron card, rendered "as if it were an active probe": a status dot + health, the expected
/// schedule, a recent-runs strip, and how long ago the job last succeeded. Each run shows a hover
/// popover in the same style as the probe history.
#[function_component(Cron)]
pub fn cron(props: &CronProps) -> Html {
    let cron = &props.cron;
    let now = chrono::Utc::now();
    let health = cron.health(now, cron.window());
    let tone = cron_tone(health);

    // "healthy for 5d" / "missed run for 2h" — how long the cron has held its current state.
    let state_text = cron
        .since(health)
        .map(|since| {
            let held = compact_duration(now.signed_duration_since(since));
            if health.passing() {
                format!("healthy for {held}")
            } else {
                format!("{} for {held}", health.label().to_lowercase())
            }
        })
        .unwrap_or_else(|| health.label().to_string());

    let schedule = match &cron.schedule {
        CronSchedule::Every(interval) => chrono::Duration::from_std(*interval)
            .ok()
            .map(|d| format!("every {}", compact_duration(d))),
        CronSchedule::Cron(expr) => Some(expr.clone()),
    };

    // How stale the job's last success is, shown terse ("22m ago") on the right of the title.
    // Derived from the converged streak (see `Cron::last_success`): it ages from the most recent run
    // while the job is healthy (resetting as runs land) and from the failure onset while it is
    // failing/stuck/missing (climbing), so it stays consistent with the streak-derived status dot.
    let last_success = cron.last_success(now).map(|at| {
        format!("{} ago", compact_duration(now.signed_duration_since(at)))
    });

    let cells: Rc<[HistoryCell]> = cron
        .runs
        .iter()
        .map(|run| HistoryCell::new(
            cron_run_tone(run),
            format!("{}, {}", run.started_at.format("%Y-%m-%d %H:%M UTC"), run_status(run)),
        ))
        .collect();
    let detail = {
        let runs = cron.runs.clone();
        Callback::from(move |index: usize| runs.get(index).map(run_detail).unwrap_or_default())
    };

    html! {
        <div class="check cron">
            <div class="check__title">
                <div class="check__identity">
                    <div class="check__label">
                        <StatusDot {tone} active={health == CronHealth::Running} />
                        <h3 class="check__name">{ &cron.name }</h3>
                    </div>
                    <Pill {tone} dot=false>{ "cron" }</Pill>
                    if let Some(schedule) = schedule {
                        <span class="cron__schedule">{ schedule }</span>
                    }
                    { tags(&cron.tags) }
                </div>
                <div class="check__state">
                    <span class="cron__state">{ state_text }</span>
                    if let Some(last_success) = last_success {
                        <span class="check__figure cron__last-checkin">{ last_success }</span>
                    }
                </div>
            </div>

            if cron.runs.is_empty() {
                <span class="cron__no-runs">{ "No runs recorded yet." }</span>
            } else {
                <HistoryStrip
                    class="cron__runs"
                    label="Recent runs"
                    variant={HistoryStripVariant::Discrete}
                    {cells}
                    {detail}
                />
            }
        </div>
    }
}

/// A run's status in words. A synthesised placeholder is labelled by its detected fault
/// (missed/stuck) rather than the underlying status it carries to progress the streak.
fn run_status(run: &CronRun) -> &'static str {
    match run.reason {
        Some(CronRunReason::Missed) => "Missed run",
        Some(CronRunReason::Stuck) => "Overrunning",
        None => run.status.label(),
    }
}

fn run_detail(run: &CronRun) -> HistoryDetail {
    let duration = match run.duration {
        Some(duration) => Some(compact_duration(
            chrono::Duration::from_std(duration).unwrap_or_default(),
        )),
        None if run.status == CronStatus::Running => Some("in progress".to_string()),
        None => None,
    };

    let content = html! {
        <div class="detail">
            if let Some(duration) = duration {
                { detail::row("Duration", duration) }
            }
        </div>
    };

    HistoryDetail::new(content)
        .header(detail::status_line(cron_run_tone(run), run_status(run)))
        .footer(html! { { run.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string() } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn render(cron: grey_api::Cron) -> String {
        yew::ServerRenderer::<Cron>::with_props(move || CronProps { cron })
            .render()
            .await
    }

    fn cron(name: &str) -> grey_api::Cron {
        grey_api::Cron::from_config(
            name,
            Default::default(),
            grey_api::CronSchedule::Every(Duration::from_secs(3600)),
            None,
            None,
        )
    }

    #[tokio::test]
    async fn renders_name_schedule_and_pending_state() {
        let html = render(cron("backup.nightly")).await;
        assert!(html.contains("backup.nightly"), "{html}");
        assert!(html.contains("every 1h"), "{html}");
        assert!(html.contains("No runs recorded yet."), "{html}");
        assert!(html.contains("Awaiting check-in"), "{html}");
    }

    #[tokio::test]
    async fn renders_a_completed_run() {
        let mut cron = cron("job");
        let now = chrono::Utc::now();
        cron.runs.push(CronRun {
            started_at: now - chrono::Duration::seconds(30),
            status: CronStatus::Succeeded,
            duration: Some(Duration::from_secs(30)),
            reason: None,
        });
        cron.last_checkin = Some(grey_api::CheckIn {
            at: now,
            status: CronStatus::Succeeded,
            message: "done".into(),
        });
        let html = render(cron).await;
        assert!(html.contains("Succeeded"), "expected a labelled run cell, got: {html}");
        assert!(html.contains("cron__last-checkin"), "expected the since-last-success time, got: {html}");
        assert!(html.contains("healthy for"), "{html}");
    }

    /// While the job reads healthy the since-last-success block ages from the most recent run, so it
    /// resets as fresh runs land.
    #[tokio::test]
    async fn since_last_success_tracks_the_last_run_while_healthy() {
        let mut cron = cron("job");
        let now = chrono::Utc::now();
        cron.runs.push(CronRun {
            started_at: now - chrono::Duration::minutes(2),
            status: CronStatus::Succeeded,
            duration: Some(Duration::from_secs(5)),
            reason: None,
        });
        let html = render(cron).await;
        assert!(html.contains("cron__last-checkin"), "expected the since-last-success time, got: {html}");
        assert!(html.contains("2m ago"), "expected time since the last run, got: {html}");
    }

    /// While the streak reads failing, the since-last-success block ages from the failure onset, so a
    /// job that has gone bad shows this figure climbing.
    #[tokio::test]
    async fn since_last_success_climbs_from_the_failure_onset() {
        let mut cron = cron("job");
        let now = chrono::Utc::now();
        // A fault that began three hours ago and is still being observed → debounced-failing.
        cron.streak = grey_api::Streak {
            failing_since: Some(now - chrono::Duration::hours(3)),
            failing_until: Some(now - chrono::Duration::minutes(1)),
            covered_since: Some(now - chrono::Duration::days(1)),
        };
        let html = render(cron).await;
        assert!(html.contains("cron__last-checkin"), "expected the since-last-success time, got: {html}");
        assert!(html.contains("3h ago"), "expected time since the failure onset, got: {html}");
    }

    /// A pending job (no runs and no fault) has nothing to age, so the block is omitted entirely.
    #[tokio::test]
    async fn since_last_success_is_absent_when_pending() {
        let html = render(cron("job")).await;
        assert!(!html.contains("cron__last-checkin"), "expected no since-last-success block, got: {html}");
    }

    /// The run popover itself only appears on hover (which SSR can't reach), so render its parts
    /// directly.
    #[derive(Properties, PartialEq)]
    struct PopoverHarnessProps {
        run: CronRun,
    }

    #[function_component(PopoverHarness)]
    fn popover_harness(props: &PopoverHarnessProps) -> Html {
        let detail = run_detail(&props.run);
        html! { <div>{ detail.header }{ detail.content }{ detail.footer }</div> }
    }

    #[tokio::test]
    async fn run_popover_shows_status_duration_and_time() {
        let run = CronRun {
            started_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            status: CronStatus::Succeeded,
            duration: Some(Duration::from_secs(90)),
            reason: None,
        };
        let html = yew::ServerRenderer::<PopoverHarness>::with_props(move || PopoverHarnessProps { run })
            .render()
            .await;

        assert!(html.contains("Succeeded"), "{html}");
        assert!(html.contains("2023-11-14"), "expected the run's start time, got: {html}");
        assert!(html.contains("Duration"), "{html}");
    }
}
