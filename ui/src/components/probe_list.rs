use super::Probe as ProbeComponent;
use crate::contexts::use_probes;
use std::collections::HashMap;
use yew::prelude::*;
use crate::formatters::availability;

#[function_component(ProbeList)]
pub fn probe_list() -> Html {
    let probes_ctx = use_probes();

    // Group probes by service tag
    let mut service_groups: HashMap<String, Vec<&grey_api::Probe>> = HashMap::new();

    for probe in &probes_ctx.probes {
        let service = probe
            .tags
            .get("service")
            .cloned()
            .unwrap_or_else(|| "Other".to_string());

        service_groups.entry(service).or_default().push(probe);
    }

    // Sort service names, but put "Other" at the end
    let mut service_names: Vec<String> = service_groups.keys().cloned().collect();
    service_names.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("Other", "Other") => std::cmp::Ordering::Equal,
        ("Other", _) => std::cmp::Ordering::Greater,
        (_, "Other") => std::cmp::Ordering::Less,
        _ => a.cmp(b),
    });

    html! {
        <>
            {for service_names.iter().map(|service_name| {
                let probes = service_groups.get(service_name).unwrap();

                // Calculate service health and availability
                let (service_health, service_availability) = calculate_service_health_and_availability(probes);

                html! {
                    <div class={format!("section service-group {}", service_health)}>
                        <div class="service-title">
                            <h2 class="service-name">{service_name}</h2>
                            <span class="service-availability">{availability(service_availability)}</span>
                        </div>
                        {for probes.iter().map(|probe| {
                            html! {
                                <ProbeComponent
                                    probe={(*probe).clone()}
                                />
                            }
                        })}
                    </div>
                }
            })}
        </>
    }
}

fn calculate_service_health_and_availability(probes: &[&grey_api::Probe]) -> (String, f64) {
    if probes.is_empty() {
        return ("unknown".to_string(), 0.0);
    }

    let mut total_availability = 0.0;
    let mut healthy_probes = 0;
    let mut total_probes = 0;

    for probe in probes {
        total_availability += probe.availability();
        total_probes += 1;

        // Check if the probe is currently healthy based on recent history
        if let Some(recent_result) = probe.history.last() {
            if recent_result.pass {
                healthy_probes += 1;
            }
        }
    }

    let average_availability = total_availability / total_probes as f64;

    // Determine service health based on probe health ratio
    let health_ratio = healthy_probes as f64 / total_probes as f64;
    let service_health = if health_ratio == 1.0 {
        "ok"
    } else if health_ratio >= 0.5 {
        "warn"
    } else {
        "error"
    };

    (service_health.to_string(), average_availability)
}
