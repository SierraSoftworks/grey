use std::rc::Rc;

use chrono::Utc;
use grey_api::ProbeHistoryBucket;
use yew::prelude::*;
use yewi::prelude::*;

use crate::components::detail;
use crate::contexts::{Store, use_store};
use crate::formatters::{availability, compact_duration};
use crate::tones::{pass_tone, sample_tone};

/// The probe's current, quorum-derived health (see [`grey_api::Probe::passing`]) and how long it has
/// held it, used to render the most recent history segment (and its tooltip) from the live state
/// rather than the bucket's average.
#[derive(Clone, PartialEq, Debug)]
pub struct LiveStatus {
    pub healthy: bool,
    pub since: Option<chrono::DateTime<Utc>>,
}

impl LiveStatus {
    /// The live status of `probe`, or `None` for records (from older agents) that carry no streak at
    /// all and so only have their bucket averages to go on.
    pub fn of(probe: &grey_api::Probe) -> Option<Self> {
        (!probe.streak.is_empty() || !probe.observers.is_empty()).then(|| Self {
            healthy: probe.passing(),
            since: probe.since(),
        })
    }
}

#[derive(Properties, PartialEq)]
pub struct ProbeHistoryProps {
    pub samples: Vec<ProbeHistoryBucket>,

    /// The probe's live status, when known.
    #[prop_or_default]
    pub live: Option<LiveStatus>,
}

/// A probe's availability history: one cell per bucket, oldest first, each revealing the bucket's
/// detail in a popover on hover or keyboard focus. When the strip is too narrow for every cell (a
/// phone), the oldest are clipped rather than squeezed into unreadable slivers.
#[function_component(ProbeHistory)]
pub fn probe_history(props: &ProbeHistoryProps) -> Html {
    let store = use_store();
    let last = props.samples.len().saturating_sub(1);

    // The most recent segment is rendered from the probe's current state: a segment that is failing
    // right now is an error regardless of how well it performed on average, while one that has
    // recovered is at worst degraded. Older segments only have their averages to go on.
    let cells: Rc<[HistoryCell]> = props
        .samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            let live = props.live.as_ref().filter(|_| index == last);
            HistoryCell::new(
                sample_tone(live.map(|live| live.healthy), sample.max_availability()),
                format!(
                    "{}, {} available",
                    sample.start_time.format("%Y-%m-%d %H:%M UTC"),
                    availability(sample.total().success_rate())
                ),
            )
        })
        .collect();

    // Built only for the cell being looked at, so a strip costs one element per bucket.
    let detail = {
        let samples = props.samples.clone();
        let live = props.live.clone();
        Callback::from(move |index: usize| match samples.get(index) {
            Some(sample) => bucket_detail(
                sample,
                live.as_ref().filter(|_| index == last),
                store.is_authenticated().then_some(&store),
            ),
            None => HistoryDetail::default(),
        })
    };

    html! {
        <HistoryStrip class="probe-history" label="Availability history" {cells} {detail} />
    }
}

/// Builds a bucket's popover. The per-observer breakdown names nodes and so is operator-only: it is
/// included only when `observers` carries the store to resolve node identifiers against (their
/// published hostnames, falling back to the identifier), and omitted for anonymous viewers.
fn bucket_detail(bucket: &ProbeHistoryBucket, live: Option<&LiveStatus>, observers: Option<&Store>) -> HistoryDetail {
    let observer_name = |id: &str| observers.map(|store| store.node_name(id)).unwrap_or_else(|| id.to_string());

    let (status, tone) = match live {
        Some(live) => {
            let since = live
                .since
                .map(|t| format!(" for {}", compact_duration(Utc::now() - t)))
                .unwrap_or_default();
            let label = if live.healthy { "Passing" } else { "Failing" };
            (format!("{label}{since}"), pass_tone(live.healthy))
        }
        None => (
            (if bucket.max_availability() == 100.0 { "Passed" } else { "Failed" }).to_string(),
            pass_tone(bucket.pass),
        ),
    };

    let stats = bucket.total();

    // The observers that agree least come first; there is only room for a few.
    let mut worst_observers = bucket.observations.iter().collect::<Vec<_>>();
    worst_observers.sort_by(|a, b| {
        a.1.success_rate().partial_cmp(&b.1.success_rate()).unwrap_or(std::cmp::Ordering::Equal)
    });
    worst_observers.truncate(bucket.validations.len().max(3));
    let hidden_observers = bucket.observations.len() - worst_observers.len();
    let show_observers = observers.is_some() && bucket.observations.len() > 1;

    let content = html! {
        <div class="detail">
            if !bucket.message.is_empty() {
                <div class="detail__message">{ &bucket.message }</div>
            }
            { detail::row("Latency", humantime::format_duration(stats.average_latency()).to_string()) }
            { detail::row("Availability", format!(
                "{} ± {:.1}%",
                availability(stats.success_rate()),
                stats.success_rate_error_margin()
            )) }
            if stats.total_retries > 0 {
                { detail::row("Retry rate", format!("{:.1}%", stats.retry_rate())) }
            }

            if show_observers {
                { detail::section("Observers", html! {
                    <>
                        { for worst_observers.iter().map(|(id, observation)| detail::entry(
                            pass_tone(observation.success_rate() > 99.0),
                            availability(observation.success_rate()),
                            Some(html! { <span title={format!("Node {id}")}>{ observer_name(id) }</span> }),
                        )) }
                        if hidden_observers > 0 {
                            <div class="detail__more">{ format!("and {hidden_observers} more...") }</div>
                        }
                    </>
                }) }
            }

            if !bucket.validations.is_empty() {
                { detail::section("Checks", html! {
                    { for bucket.validations.iter().map(|(name, validation)| detail::entry(
                        pass_tone(validation.pass),
                        name.clone(),
                        validation.message.as_ref().map(|message| html! { { message } }),
                    )) }
                }) }
            }
        </div>
    };

    HistoryDetail::new(content)
        .header(detail::status_line(tone, status))
        .footer(html! { { bucket.start_time.format("%Y-%m-%d %H:%M:%S UTC").to_string() } })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::contexts::{StoreProvider, use_store};

    #[derive(Properties, PartialEq, Clone)]
    struct HarnessProps {
        bucket: ProbeHistoryBucket,
        live: Option<LiveStatus>,
        /// Whether to render as an operator (with the observer breakdown) or anonymously.
        operator: bool,
        nodes: Vec<grey_api::NodeMetadata>,
    }

    /// Renders the popover's parts directly — in the app they only appear on hover, which SSR can't
    /// reach.
    #[function_component(Tooltip)]
    fn tooltip(props: &HarnessProps) -> Html {
        let store = use_store();
        let detail = bucket_detail(&props.bucket, props.live.as_ref(), props.operator.then_some(&store));
        html! { <div>{ detail.header }{ detail.content }{ detail.footer }</div> }
    }

    #[function_component(Harness)]
    fn harness(props: &HarnessProps) -> Html {
        html! {
            <StoreProvider nodes={props.nodes.clone()}>
                <Tooltip ..props.clone() />
            </StoreProvider>
        }
    }

    fn bucket_with_observers(ids: &[&str]) -> ProbeHistoryBucket {
        ProbeHistoryBucket {
            start_time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            pass: true,
            message: String::new(),
            validations: Default::default(),
            observations: ids.iter().map(|id| (id.to_string(), grey_api::Observation::default())).collect(),
        }
    }

    async fn render_full(bucket: ProbeHistoryBucket, live: Option<LiveStatus>, operator: bool, nodes: Vec<grey_api::NodeMetadata>) -> String {
        yew::ServerRenderer::<Harness>::with_props(move || HarnessProps { bucket, live, operator, nodes })
            .render()
            .await
    }

    async fn render(live: Option<LiveStatus>) -> String {
        render_full(bucket_with_observers(&[]), live, true, vec![]).await
    }

    /// Operators see the observer breakdown with node identifiers resolved to their published
    /// hostnames (unresolved ones keep the identifier); anonymous viewers see no observers at all.
    #[tokio::test]
    async fn test_observers_are_named_for_operators_only() {
        let bucket = bucket_with_observers(&["1p3x9k", "zz9plural"]);
        let nodes = vec![grey_api::NodeMetadata::new(
            "1p3x9k",
            [("hostname".to_string(), "grey-syd-1".to_string())].into_iter().collect(),
            chrono::Utc::now(),
        )];

        let html = render_full(bucket.clone(), None, true, nodes.clone()).await;
        assert!(html.contains("Observers"), "expected the observer section, got: {html}");
        assert!(html.contains("grey-syd-1"), "expected the resolved hostname, got: {html}");
        assert!(!html.contains(">1p3x9k<"), "the resolved id must not be shown as the name, got: {html}");
        assert!(html.contains(">zz9plural<"), "an unresolved id falls back to itself, got: {html}");

        let html = render_full(bucket, None, false, nodes).await;
        assert!(!html.contains("Observers"), "anonymous viewers must not see observers, got: {html}");
        assert!(!html.contains("grey-syd-1") && !html.contains("1p3x9k"), "no node names for anonymous viewers, got: {html}");
    }

    /// A failing bucket explains itself: its message, each check with its reason, and only the
    /// least healthy observers, with the rest counted.
    #[tokio::test]
    async fn test_tooltip_explains_a_failing_bucket() {
        let mut bucket = bucket_with_observers(&["a1", "b2", "c3", "d4", "e5"]);
        bucket.pass = false;
        bucket.message = "connection refused".into();
        bucket.validations =
            [("http.status".to_string(), grey_api::ValidationResult::fail("expected 200, got 503"))].into_iter().collect();

        let html = render_full(bucket, None, true, vec![]).await;
        assert!(html.contains("connection refused"), "expected the bucket's message, got: {html}");
        assert!(html.contains("http.status") && html.contains("expected 200, got 503"), "expected the failed check, got: {html}");
        assert!(html.contains("and 2 more"), "expected the remaining observers to be counted, got: {html}");
    }

    #[tokio::test]
    async fn test_tooltip_shows_bucket_footer_and_streak_since() {
        let live = LiveStatus { healthy: true, since: Some(chrono::Utc::now() - chrono::Duration::days(5)) };

        let html = render(Some(live)).await;
        assert!(html.contains("2023-11-14"), "expected the bucket timestamp in the footer, got: {html}");
        assert!(html.contains("Passing for 5d"), "expected the live status, got: {html}");
    }

    #[tokio::test]
    async fn test_tooltip_omits_streak_row_for_legacy_records() {
        let html = render(None).await;
        assert!(html.contains("2023-11-14"), "expected the bucket timestamp in the footer, got: {html}");
        assert!(!html.contains("Passing"), "expected no live status, got: {html}");
    }

    #[test]
    fn live_status_is_absent_for_legacy_records() {
        let mut probe = grey_api::Probe {
            name: "p".into(),
            tags: Default::default(),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: Default::default(),
            streak: Default::default(),
            debounce: None,
            retired: false,
            observers: Default::default(),
            quorum: None,
        };
        assert!(LiveStatus::of(&probe).is_none());
        probe.observers.insert("a".into(), grey_api::ObserverState::default());
        assert!(LiveStatus::of(&probe).is_some());
    }
}
