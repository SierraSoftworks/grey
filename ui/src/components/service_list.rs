use super::filter::use_filter;
use super::{Cron as CronComponent, Probe as ProbeComponent};
use crate::contexts::use_store;
use crate::formatters::availability;
use std::collections::HashMap;
use yew::prelude::*;
use yewi::prelude::*;

/// The probes and crons that share a `service` tag, rendered together under one service group.
#[derive(Default)]
pub struct ServiceGroup<'a> {
    pub probes: Vec<&'a grey_api::Probe>,
    pub crons: Vec<&'a grey_api::Cron>,
}

impl ServiceGroup<'_> {
    /// The service's health plus the summary shown on the right of its title: the probe-averaged
    /// availability when the service has probes, otherwise a healthy-count (a cron-only service has
    /// no availability percentage to report).
    pub fn summary(&self, now: chrono::DateTime<chrono::Utc>) -> (Tone, String) {
        let total = self.probes.len() + self.crons.len();
        if total == 0 {
            return (Tone::Neutral, String::new());
        }

        let healthy = self.probes.iter().filter(|p| p.passing()).count()
            + self.crons.iter().filter(|c| c.passing(now, c.window())).count();

        let health = if healthy == total {
            Tone::Ok
        } else if healthy * 2 >= total {
            Tone::Warn
        } else {
            Tone::Danger
        };

        let summary = if !self.probes.is_empty() {
            let avg = self.probes.iter().map(|p| p.availability()).sum::<f64>()
                / self.probes.len() as f64;
            availability(avg)
        } else {
            format!("{healthy}/{total} healthy")
        };

        (health, summary)
    }
}

/// The name of the service an entry belongs to: its `service` tag, falling back to "Other".
fn service_of(tags: &HashMap<String, String>) -> &str {
    tags.get("service").map(String::as_str).unwrap_or("Other")
}

/// Every probe and cron grouped by service, sorted by name with "Other" kept at the end.
pub fn services<'a>(
    probes: &'a [grey_api::Probe],
    crons: &'a [grey_api::Cron],
) -> Vec<(&'a str, ServiceGroup<'a>)> {
    let mut groups: HashMap<&str, ServiceGroup> = HashMap::new();
    for probe in probes {
        groups.entry(service_of(&probe.tags)).or_default().probes.push(probe);
    }
    for cron in crons {
        groups.entry(service_of(&cron.tags)).or_default().crons.push(cron);
    }

    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|(a, _), (b, _)| (*a == "Other").cmp(&(*b == "Other")).then_with(|| a.cmp(b)));
    groups
}

/// The id of a service's card, which the sidebar links to.
pub fn service_anchor(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    format!("service-{slug}")
}

/// Whether an entry matches the app bar's filter, by its name, its service or any `name:value` tag.
fn matches(filter: &super::Filter, name: &str, service: &str, tags: &HashMap<String, String>) -> bool {
    let tags: Vec<String> = tags.iter().map(|(name, value)| format!("{name}:{value}")).collect();
    filter.matches([name, service].into_iter().chain(tags.iter().map(String::as_str)))
}

/// The status page's service list: every probe and cron grouped by its `service` tag (falling back
/// to "Other"), so a service's active probes and its scheduled jobs appear together. The app bar's
/// filter narrows the entries shown; a service's summary always covers the whole service.
#[function_component(ServiceList)]
pub fn service_list() -> Html {
    let store = use_store();
    let filter = use_filter();
    let now = chrono::Utc::now();

    let cards: Vec<Html> = services(store.probes(), store.crons())
        .into_iter()
        .filter_map(|(name, group)| {
            let (health, summary) = group.summary(now);
            let probes: Vec<_> =
                group.probes.iter().filter(|p| matches(&filter, &p.name, name, &p.tags)).collect();
            let crons: Vec<_> =
                group.crons.iter().filter(|c| matches(&filter, &c.name, name, &c.tags)).collect();
            if probes.is_empty() && crons.is_empty() {
                return None;
            }

            Some(html! {
                <Card
                    key={name.to_string()}
                    id={service_anchor(name)}
                    class={classes!("service", format!("service--{}", health.as_str()))}
                    title={name.to_string()}
                    actions={html! { <span class="service__availability">{ summary }</span> }}
                >
                    { for probes.into_iter().map(|probe| html! {
                        <ProbeComponent key={probe.name.clone()} probe={(*probe).clone()} />
                    }) }
                    { for crons.into_iter().map(|cron| html! {
                        <CronComponent key={cron.name.clone()} cron={(*cron).clone()} />
                    }) }
                </Card>
            })
        })
        .collect();

    if cards.is_empty() && !filter.is_empty() {
        return html! {
            <EmptyState title="Nothing matches the filter" icon={icons::SEARCH}>
                { "No probe or scheduled job has a name, service or tag that matches." }
            </EmptyState>
        };
    }

    html! { <div class="services">{ for cards }</div> }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexts::StoreProvider;
    use std::time::Duration;

    #[derive(Properties, PartialEq)]
    struct HarnessProps {
        probes: Vec<grey_api::Probe>,
        crons: Vec<grey_api::Cron>,
    }

    #[function_component(Harness)]
    fn harness(props: &HarnessProps) -> Html {
        html! {
            <StoreProvider probes={props.probes.clone()} crons={props.crons.clone()}>
                <ServiceList />
            </StoreProvider>
        }
    }

    fn tag(service: &str) -> std::collections::HashMap<String, String> {
        [("service".to_string(), service.to_string())].into_iter().collect()
    }

    fn probe(name: &str, service: &str) -> grey_api::Probe {
        grey_api::Probe {
            name: name.into(),
            tags: tag(service),
            last_updated: chrono::Utc::now(),
            history: vec![],
            observations: Default::default(),
            streak: Default::default(),
            debounce: None,
            retired: false,
            observers: Default::default(),
            quorum: None,
        }
    }

    fn cron(name: &str, service: &str) -> grey_api::Cron {
        grey_api::Cron::from_config(
            name,
            tag(service),
            grey_api::CronSchedule::Every(Duration::from_secs(3600)),
            None,
            None,
        )
    }

    async fn render(probes: Vec<grey_api::Probe>, crons: Vec<grey_api::Cron>) -> String {
        yew::ServerRenderer::<Harness>::with_props(move || HarnessProps { probes, crons })
            .render()
            .await
    }

    #[tokio::test]
    async fn probes_and_crons_share_a_service_group() {
        let html = render(
            vec![probe("api.health", "Backups")],
            vec![cron("backup.nightly", "Backups")],
        )
        .await;

        // A single "Backups" service group contains both the probe and the cron, and there is no
        // separate cron section any more.
        assert_eq!(
            html.matches("service__availability").count(),
            1,
            "expected exactly one service group: {html}"
        );
        assert!(html.contains("Backups"));
        assert!(html.contains("api.health"), "the probe should render: {html}");
        assert!(html.contains("backup.nightly"), "the cron should render: {html}");
        assert!(!html.contains("Scheduled Jobs"), "the separate cron section is gone: {html}");
    }

    #[tokio::test]
    async fn cron_only_service_shows_a_healthy_count() {
        let html = render(vec![], vec![cron("backup.nightly", "Backups")]).await;
        assert!(
            html.contains("1/1 healthy"),
            "a cron-only service reports a healthy count rather than an availability %: {html}"
        );
    }
}
