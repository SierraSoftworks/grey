use chrono::Utc;
use grey_api::{Impact, Incident};

use crate::formatters::compact_duration;
use crate::styles::impact_class;

/// The status text shown alongside an incident, paired with its colour class: the current impact and
/// how long it has held (like a probe's streak), "Resolved" once it returns to operational, or
/// "Draft" while hidden.
pub fn incident_status(incident: &Incident) -> (String, &'static str) {
    match incident.current_impact() {
        Impact::Hidden => ("Draft".to_string(), impact_class(Impact::Hidden)),
        Impact::None => {
            let start = incident.started_at();
            let end = incident.ended_at();
            match (start, end) {
                (Some(s), Some(e)) if e > s => {
                    let duration = compact_duration(e.signed_duration_since(s));
                    (format!("Resolved after {duration}"), impact_class(Impact::None))
                }
                _ => ("Resolved".to_string(), impact_class(Impact::None)),
            }
        }
        impact @ (Impact::Offline | Impact::Degraded) => {
            let since = incident.impact_since().or_else(|| incident.started_at());
            let held = since
                .map(|s| format!(" for {}", compact_duration(Utc::now().signed_duration_since(s))))
                .unwrap_or_default();
            (format!("{}{held}", impact.label()), impact_class(impact))
        }
    }
}
