use super::ProbeHistory;
use crate::formatters::{availability, compact_duration};
use crate::styles::probe_class;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ProbeProps {
    pub probe: grey_api::Probe,
}

#[function_component(Probe)]
pub fn probe(props: &ProbeProps) -> Html {
    let recent_availability = props.probe.recent(2).success_rate();
    let streak = props.probe.streak.clone();

    // Key the status off the currently observed state so a recovery is reflected
    // immediately, using the recent average only to grade how severe an ongoing failure is.
    let probe_class = probe_class(props.probe.passing(), recent_availability);

    // How long the probe has held its current state, e.g. "healthy for 5d" or "unhealthy for 17m".
    let streak_text = streak.since().map(|since| {
        let held_for = compact_duration(chrono::Utc::now().signed_duration_since(since));
        if streak.passing() {
            format!("healthy for {held_for}")
        } else {
            format!("unhealthy for {held_for}")
        }
    });

    html! {
        <div class="probe">
            <div class="probe-title">
                <div class="probe-name-section">
                    <div class={format!("status-dot {}", probe_class)}></div>
                    <h3 class="probe-name">{&props.probe.name}</h3>

                    if !props.probe.tags.is_empty() {
                        <div class="probe-tags">
                            {for props.probe.tags.iter().filter(|(name, _)| *name != "service").map(|(name, value)| {
                                html! {
                                    <div class="probe-tag">
                                        <span class="tag-name">{name}{":"}</span>
                                        <strong class="tag-value">{value}</strong>
                                    </div>
                                }
                            })}
                        </div>
                    }
                </div>
                
                if let Some(streak_text) = streak_text {
                    <div class="probe-streak">{streak_text}</div>
                }
                <div class="availability">{availability(props.probe.availability())}</div>
            </div>
            <ProbeHistory samples={props.probe.history.clone()} streak={streak} />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::Streak;

    async fn render(streak: Streak) -> String {
        let probe = grey_api::Probe {
            name: "probe".into(),
            tags: Default::default(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: Default::default(),
            streak,
        };
        yew::ServerRenderer::<Probe>::with_props(move || ProbeProps { probe })
            .render()
            .await
    }

    #[tokio::test]
    async fn test_shows_healthy_streak_duration() {
        let mut streak = Streak::default();
        streak.observe(true, chrono::Utc::now() - chrono::Duration::days(5));

        let html = render(streak).await;
        assert!(html.contains("healthy for 5d"), "expected the healthy streak text, got: {html}");
    }

    #[tokio::test]
    async fn test_shows_unhealthy_streak_duration() {
        // An ongoing failure episode: failures observed continuously (within the recovery
        // window of each other) since 17 minutes ago.
        let mut streak = Streak::default();
        let now = chrono::Utc::now();
        for minutes_ago in (2..=17).rev().step_by(3) {
            streak.observe(false, now - chrono::Duration::minutes(minutes_ago));
        }

        let html = render(streak).await;
        assert!(html.contains("unhealthy for 17m"), "expected the unhealthy streak text, got: {html}");
    }

    #[tokio::test]
    async fn test_omits_streak_text_for_legacy_records() {
        // Records from older agents carry no streak observations at all.
        let html = render(Streak::default()).await;
        assert!(!html.contains("probe-streak"), "expected no streak text, got: {html}");
    }
}
