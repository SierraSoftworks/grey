use super::{LiveStatus, ProbeHistory, StatusDot};
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
    let live = LiveStatus::of(&props.probe);

    // With several observers, health is decided by quorum: say how many currently disagree with
    // the pooled verdict so an operator can tell a single bad vantage point from a real outage.
    let observers_text = (props.probe.observers.len() > 1).then(|| {
        let now = chrono::Utc::now();
        let failing = props.probe.failing_observers_at(now, props.probe.window());
        format!(
            "{failing}/{} observers failing (quorum {})",
            props.probe.observers.len(),
            props.probe.quorum_size()
        )
    });

    // Key the status off the currently observed (debounced) state so a recovery is reflected once it
    // settles, using the recent average only to grade how severe an ongoing failure is.
    let probe_class = probe_class(props.probe.passing(), recent_availability);

    // How long the probe has held its current state, e.g. "healthy for 5d" or "unhealthy for 17m".
    let streak_text = props.probe.since().map(|since| {
        let held_for = compact_duration(chrono::Utc::now().signed_duration_since(since));
        if props.probe.passing() {
            format!("healthy for {held_for}")
        } else {
            format!("unhealthy for {held_for}")
        }
    });

    html! {
        <div class="probe">
            <div class="probe__title">
                <div class="probe__name-section">
                    <StatusDot class={probe_class} active=true />
                    <h3 class="probe__name">{&props.probe.name}</h3>

                    if !props.probe.tags.is_empty() {
                        <div class="probe__tags">
                            {for props.probe.tags.iter().filter(|(name, _)| *name != "service").map(|(name, value)| {
                                html! {
                                    <div class="probe__tag">
                                        <span class="probe__tag-name">{name}{":"}</span>
                                        <strong class="probe__tag-value">{value}</strong>
                                    </div>
                                }
                            })}
                        </div>
                    }
                </div>
                
                if let Some(observers_text) = observers_text {
                    <div class="probe__observers">{observers_text}</div>
                }
                if let Some(streak_text) = streak_text {
                    <div class="probe__streak">{streak_text}</div>
                }
                <div class="probe__availability">{availability(props.probe.availability())}</div>
            </div>
            <ProbeHistory samples={props.probe.history.clone()} live={live} />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grey_api::Streak;

    #[derive(Properties, PartialEq)]
    struct HarnessProps {
        probe: grey_api::Probe,
    }

    /// Wraps `Probe` in a `StoreProvider`, which its `ProbeHistory` child requires for auth state.
    #[function_component(Harness)]
    fn harness(props: &HarnessProps) -> Html {
        html! {
            <crate::contexts::StoreProvider>
                <Probe probe={props.probe.clone()} />
            </crate::contexts::StoreProvider>
        }
    }

    async fn render(streak: Streak) -> String {
        let probe = grey_api::Probe {
            name: "probe".into(),
            tags: Default::default(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: Default::default(),
            streak,
            debounce: None,
            retired: false,
            observers: Default::default(),
            quorum: None,
        };
        yew::ServerRenderer::<Harness>::with_props(move || HarnessProps { probe })
            .render()
            .await
    }

    #[tokio::test]
    async fn test_shows_healthy_streak_duration() {
        let mut streak = Streak::default();
        streak.observe(true, chrono::Utc::now() - chrono::Duration::days(5), Streak::default_recovery_window());

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
            streak.observe(false, now - chrono::Duration::minutes(minutes_ago), Streak::default_recovery_window());
        }

        let html = render(streak).await;
        assert!(html.contains("unhealthy for 17m"), "expected the unhealthy streak text, got: {html}");
    }

    /// With several observers the displayed health follows the quorum, and the observer tally says
    /// how many currently disagree.
    #[tokio::test]
    async fn test_shows_quorum_health_and_observer_tally() {
        let now = chrono::Utc::now();
        let window = Streak::default_recovery_window();
        let failing = grey_api::ObserverState {
            streak: Streak { failing_since: Some(now - window * 3), failing_until: Some(now), covered_since: None },
            last_updated: now,
        };
        let passing = grey_api::ObserverState {
            streak: Streak { failing_since: None, failing_until: None, covered_since: Some(now - chrono::Duration::days(2)) },
            last_updated: now,
        };
        let probe = grey_api::Probe {
            name: "probe".into(),
            tags: Default::default(),
            last_updated: now,
            history: vec![],
            observations: Default::default(),
            streak: Streak::default(),
            debounce: None,
            retired: false,
            observers: [("a".to_string(), failing), ("b".to_string(), passing.clone()), ("c".to_string(), passing)].into_iter().collect(),
            quorum: None,
        };
        let html = yew::ServerRenderer::<Harness>::with_props(move || HarnessProps { probe })
            .render()
            .await;
        assert!(html.contains("1/3 observers failing (quorum 2)"), "expected the observer tally, got: {html}");
        assert!(html.contains("healthy for 2d"), "one dissenting observer must not read as unhealthy, got: {html}");
    }

    #[tokio::test]
    async fn test_omits_streak_text_for_legacy_records() {
        // Records from older agents carry no streak observations at all.
        let html = render(Streak::default()).await;
        assert!(!html.contains("probe__streak"), "expected no streak text, got: {html}");
    }
}
