use super::{Popover, PopoverAlign, StatusDot};
use crate::formatters::compact_duration;
use crate::styles::{cron_class, cron_run_class};
use grey_api::{CronHealth, CronRun, CronSchedule, CronStatus};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct CronProps {
    pub cron: grey_api::Cron,
}

/// A single cron card, rendered "as if it were an active probe": a status dot + health, the expected
/// schedule, a recent-runs strip, and the last reported check-in. Each run shows a hover popover in
/// the same style as the probe history.
#[function_component(Cron)]
pub fn cron(props: &CronProps) -> Html {
    let cron = &props.cron;
    let now = chrono::Utc::now();
    let health = cron.health(now);
    let class = cron_class(health);

    // Which run's popover is currently open (on hover).
    let hovered = use_state(|| Option::<usize>::None);

    // "healthy for 5d" / "missed run for 2h" — how long the cron has held its current state.
    let state_text = cron
        .since(now)
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

    let last_checkin = cron.last_checkin.as_ref().map(|checkin| {
        format!(
            "last check-in {} ago",
            compact_duration(now.signed_duration_since(checkin.at))
        )
    });

    let run_count = cron.runs.len();

    html! {
        <div class="cron">
            <div class="cron__title">
                <div class="cron__name-section">
                    <StatusDot class={class} active={health == CronHealth::Running} />
                    <h3 class="cron__name">{&cron.name}</h3>
                    <span class={classes!("cron__badge", class)}>{"cron"}</span>
                    if let Some(schedule) = schedule {
                        <span class="cron__schedule">{schedule}</span>
                    }
                    if !cron.tags.is_empty() {
                        <div class="cron__tags">
                            {for cron.tags.iter().filter(|(name, _)| *name != "service").map(|(name, value)| html! {
                                <div class="cron__tag">
                                    <span class="cron__tag-name">{name}{":"}</span>
                                    <strong class="cron__tag-value">{value}</strong>
                                </div>
                            })}
                        </div>
                    }
                </div>
                <div class="cron__state">{state_text}</div>
            </div>

            <div class="cron__runs">
                if cron.runs.is_empty() {
                    <span class="cron__no-runs">{"No runs recorded yet."}</span>
                } else {
                    {for cron.runs.iter().enumerate().map(|(index, run)| {
                        let is_hovered = *hovered == Some(index);
                        let onmouseenter = {
                            let hovered = hovered.clone();
                            Callback::from(move |_: MouseEvent| hovered.set(Some(index)))
                        };
                        let onmouseleave = {
                            let hovered = hovered.clone();
                            Callback::from(move |_: MouseEvent| hovered.set(None))
                        };
                        html! {
                            <span
                                class={classes!("cron-run", cron_run_class(run.status), is_hovered.then_some("tooltip-target"))}
                                {onmouseenter}
                                {onmouseleave}
                            >
                                if is_hovered {
                                    { render_run_popover(run, align_for(index, run_count)) }
                                }
                            </span>
                        }
                    })}
                }
            </div>

            if let Some(checkin) = cron.last_checkin.as_ref() {
                <div class="cron__meta">
                    if let Some(last_checkin) = last_checkin {
                        <span class="cron__last-checkin">{last_checkin}</span>
                    }
                    if !checkin.message.is_empty() {
                        <span class="cron__message">{&checkin.message}</span>
                    }
                </div>
            }
        </div>
    }
}

/// Anchors the popover inward at the ends of the strip so it never runs off the page.
fn align_for(index: usize, len: usize) -> PopoverAlign {
    if index == 0 {
        PopoverAlign::Left
    } else if index + 1 == len {
        PopoverAlign::Right
    } else {
        PopoverAlign::Center
    }
}

fn status_label(status: CronStatus) -> &'static str {
    match status {
        CronStatus::Running => "Running",
        CronStatus::Succeeded => "Succeeded",
        CronStatus::Failed => "Failed",
    }
}

fn render_run_popover(run: &CronRun, align: PopoverAlign) -> Html {
    let timestamp = run.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let duration = match run.duration {
        Some(duration) => Some(compact_duration(
            chrono::Duration::from_std(duration).unwrap_or_default(),
        )),
        None if run.status == CronStatus::Running => Some("in progress".to_string()),
        None => None,
    };

    html! {
        <Popover
            {align}
            status_class={cron_run_class(run.status)}
            status={status_label(run.status)}
            timestamp={timestamp}
        >
            <div class="tooltip__details">
                if let Some(duration) = duration {
                    <div class="tooltip__row">
                        <span class="tooltip__label">{"Duration:"}</span>
                        <span>{duration}</span>
                    </div>
                }
            </div>
        </Popover>
    }
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
        });
        cron.last_checkin = Some(grey_api::CheckIn {
            at: now,
            status: CronStatus::Succeeded,
            message: "done".into(),
        });
        let html = render(cron).await;
        assert!(html.contains("cron-run"), "expected a run cell, got: {html}");
        assert!(html.contains("done"), "expected the check-in message, got: {html}");
        assert!(html.contains("healthy for"), "{html}");
    }

    /// The run popover itself only appears on hover (which SSR can't reach), so render it directly to
    /// confirm it uses the shared popover styling.
    #[derive(Properties, PartialEq)]
    struct PopoverHarnessProps {
        run: CronRun,
    }

    #[function_component(PopoverHarness)]
    fn popover_harness(props: &PopoverHarnessProps) -> Html {
        render_run_popover(&props.run, PopoverAlign::Center)
    }

    #[tokio::test]
    async fn run_popover_uses_shared_style() {
        let run = CronRun {
            started_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            status: CronStatus::Succeeded,
            duration: Some(Duration::from_secs(90)),
        };
        let html = yew::ServerRenderer::<PopoverHarness>::with_props(move || PopoverHarnessProps { run })
            .render()
            .await;

        assert!(html.contains("popover"), "expected the shared popover, got: {html}");
        assert!(html.contains("popover__status"), "{html}");
        assert!(html.contains("Succeeded"), "{html}");
        assert!(html.contains("popover__time"), "expected the timestamp footer, got: {html}");
        assert!(html.contains("2023-11-14"), "{html}");
        assert!(html.contains("Duration:"), "{html}");
    }
}
