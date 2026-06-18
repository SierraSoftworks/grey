use super::{Cron as CronComponent, Probe as ProbeComponent};
use crate::contexts::use_store;
use crate::formatters::availability;
use std::collections::HashMap;
use yew::prelude::*;

/// The probes and crons that share a `service` tag, rendered together under one service group.
#[derive(Default)]
struct ServiceGroup<'a> {
    probes: Vec<&'a grey_api::Probe>,
    crons: Vec<&'a grey_api::Cron>,
}

impl ServiceGroup<'_> {
    /// The service's health class plus the summary shown on the right of its title: the
    /// probe-averaged availability when the service has probes, otherwise a healthy-count (a
    /// cron-only service has no availability percentage to report).
    fn summary(&self, now: chrono::DateTime<chrono::Utc>) -> (&'static str, String) {
        let total = self.probes.len() + self.crons.len();
        if total == 0 {
            return ("unknown", String::new());
        }

        let healthy = self.probes.iter().filter(|p| p.passing()).count()
            + self.crons.iter().filter(|c| c.passing(now)).count();

        let health = if healthy == total {
            "ok"
        } else if healthy * 2 >= total {
            "warn"
        } else {
            "error"
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

/// The status page's service list: every probe and cron grouped by its `service` tag (falling back
/// to "Other"), so a service's active probes and its scheduled jobs appear together.
#[function_component(ServiceList)]
pub fn service_list() -> Html {
    let store = use_store();
    let now = chrono::Utc::now();

    let mut groups: HashMap<String, ServiceGroup> = HashMap::new();
    for probe in store.probes() {
        let service = probe
            .tags
            .get("service")
            .cloned()
            .unwrap_or_else(|| "Other".to_string());
        groups.entry(service).or_default().probes.push(probe);
    }
    for cron in store.crons() {
        let service = cron
            .tags
            .get("service")
            .cloned()
            .unwrap_or_else(|| "Other".to_string());
        groups.entry(service).or_default().crons.push(cron);
    }

    // Sort service names alphabetically, but keep "Other" at the end.
    let mut service_names: Vec<String> = groups.keys().cloned().collect();
    service_names.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("Other", "Other") => std::cmp::Ordering::Equal,
        ("Other", _) => std::cmp::Ordering::Greater,
        (_, "Other") => std::cmp::Ordering::Less,
        _ => a.cmp(b),
    });

    html! {
        <>
            {for service_names.iter().map(|service_name| {
                let group = groups.get(service_name).unwrap();
                let (service_health, summary) = group.summary(now);

                html! {
                    <div class={format!("section service {}", service_health)}>
                        <div class="service__title">
                            <h2 class="service__name">{service_name}</h2>
                            <span class="service__availability">{summary}</span>
                        </div>
                        {for group.probes.iter().map(|probe| html! {
                            <ProbeComponent probe={(*probe).clone()} />
                        })}
                        {for group.crons.iter().map(|cron| html! {
                            <CronComponent cron={(*cron).clone()} />
                        })}
                    </div>
                }
            })}
        </>
    }
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
            html.matches("service__name").count(),
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
