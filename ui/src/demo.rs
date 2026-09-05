//! Demo mode: a self-contained fixture that renders the whole SPA without an agent behind it.
//!
//! Append `?demo` to the URL of a `trunk serve` build and the app boots from the fixtures below
//! instead of hydrating the server-rendered payload; the store's polling and OIDC bootstrap are
//! disabled, so nothing ever reaches the network. The dataset deliberately covers the awkward cases
//! — long probe names, many tags, a full 48-bucket history strip, a full 50-cell cron run strip, and
//! every health state — because those are what break on narrow viewports.
//!
//! Compiled only under `debug_assertions`, so release builds carry neither the fixtures nor the
//! query-string check.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Duration as Delta, Utc};
use grey_api::{
    AdminUser, CheckIn, Cron, CronRun, CronRunReason, CronSchedule, CronStatus, Identifier, Impact,
    Incident, IncidentUpdate, IncidentUpdateId, IncidentView, NodeMetadata, Observation, Peer,
    PeerHealth, Probe, ProbeHistoryBucket, Streak, UiConfig, UiLink, ValidationResult,
};
use yew::prelude::*;

use crate::contexts::StoreProvider;

/// Whether the page was loaded with the `?demo` trigger.
#[cfg(feature = "wasm")]
pub fn enabled() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .map(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .any(|param| param == "demo" || param.starts_with("demo="))
        })
        .unwrap_or(false)
}

#[cfg(not(feature = "wasm"))]
pub fn enabled() -> bool {
    false
}

/// The demo app root: the same component tree as [`crate::App`], seeded from the fixtures and with
/// the store told never to contact the API.
#[function_component(DemoApp)]
pub fn demo_app() -> Html {
    html! {
        <div id="app">
            <StoreProvider
                demo=true
                config={config()}
                probes={probes()}
                crons={crons()}
                incidents={incidents()}
                peers={peers()}
                nodes={nodes()}
                user={Some(user())}
            >
                { crate::client::render_router("/") }
            </StoreProvider>
        </div>
    }
}

// --- Fixtures -------------------------------------------------------------------------------

pub fn config() -> UiConfig {
    UiConfig {
        title: "Grey Demo".into(),
        links: vec![
            UiLink {
                title: "Documentation".into(),
                url: "https://github.com/SierraSoftworks/grey".into(),
            },
            UiLink {
                title: "Support".into(),
                url: "https://github.com/SierraSoftworks/grey/issues".into(),
            },
        ],
        ..Default::default()
    }
}

/// A signed-in administrator, so the operator-only chrome (cluster status, observer breakdowns,
/// incident editing) renders too.
pub fn user() -> AdminUser {
    AdminUser {
        subject: "demo|000000".into(),
        email: Some("operator@example.com".into()),
        name: Some("Demo Operator".into()),
    }
}

pub fn peers() -> Vec<Peer> {
    let now = Utc::now();
    vec![
        peer("1p3x9kq2m7v4c8", PeerHealth::Online, now, true),
        peer("7h2wz0r5t9d1na", PeerHealth::Online, now - Delta::seconds(3), false),
        peer("c4j8l1y6b3f0ps", PeerHealth::Transitive, now - Delta::seconds(45), false),
        peer("m9e3q7u2k5x1gw", PeerHealth::Suspect, now - Delta::minutes(4), false),
        peer("t6a0n4v8h2r7zd", PeerHealth::Offline, now - Delta::hours(9), false),
    ]
}

/// The metadata the demo nodes publish about themselves. One member (the offline `t6a0...`) has
/// none, so the identifier fallback renders too.
pub fn nodes() -> Vec<NodeMetadata> {
    let now = Utc::now();
    vec![
        node(now, "1p3x9kq2m7v4c8", &[("hostname", "grey-syd-1"), ("version", "1.4.2"), ("cloud", "aws"), ("region", "ap-southeast-2"), ("az", "ap-southeast-2a"), ("cluster", "prod")]),
        node(now, "7h2wz0r5t9d1na", &[("hostname", "grey-lhr-1"), ("version", "1.4.2"), ("cloud", "hetzner"), ("region", "eu-west"), ("cluster", "prod")]),
        node(now, "c4j8l1y6b3f0ps", &[("hostname", "grey-iad-1"), ("version", "1.4.1"), ("cloud", "gcp"), ("region", "us-east4"), ("az", "us-east4-b"), ("cluster", "prod")]),
        node(now, "m9e3q7u2k5x1gw", &[("hostname", "grey-fra-1.internal.example.com"), ("version", "1.4.2"), ("cloud", "azure"), ("region", "germanywestcentral"), ("cluster", "prod")]),
    ]
}

pub fn probes() -> Vec<Probe> {
    let now = Utc::now();
    vec![
        probe(
            now,
            "api.sierrasoftworks.com/healthz",
            &[("service", "Public API"), ("region", "global"), ("tier", "critical")],
            Shape::Solid,
            1,
        ),
        probe(
            now,
            "api.sierrasoftworks.com/v1/accounts",
            &[("service", "Public API"), ("region", "au-east"), ("protocol", "https")],
            Shape::Flaky,
            2,
        ),
        probe(
            now,
            "identity.sierrasoftworks.com/.well-known/openid-configuration",
            &[("service", "Identity"), ("region", "global")],
            Shape::Recovered,
            3,
        ),
        probe(
            now,
            "identity.sierrasoftworks.com/token",
            &[("service", "Identity"), ("region", "eu-west"), ("tier", "critical")],
            Shape::Solid,
            4,
        ),
        probe(
            now,
            "storage.sierrasoftworks.com/blobs",
            &[("service", "Storage"), ("region", "us-east")],
            Shape::Failing,
            5,
        ),
        probe(now, "cdn.sierrasoftworks.com", &[("service", "Storage")], Shape::Solid, 6),
    ]
}

pub fn crons() -> Vec<Cron> {
    let now = Utc::now();
    vec![
        healthy_cron(now),
        running_cron(now),
        missed_cron(now),
        failed_cron(now),
        pending_cron(),
    ]
}

pub fn incidents() -> Vec<IncidentView> {
    let now = Utc::now();
    vec![
        incident(
            7_002,
            "Elevated error rates on blob storage reads",
            &[
                (
                    Impact::Offline,
                    now - Delta::minutes(95),
                    "We are investigating a complete loss of read availability for the `storage` service in `us-east`. Writes are unaffected.",
                ),
                (
                    Impact::Degraded,
                    now - Delta::minutes(40),
                    "A failed storage node has been drained and traffic is being served from the remaining replicas. Reads are succeeding, with elevated latency.",
                ),
            ],
        ),
        incident(
            7_001,
            "Scheduled maintenance: identity provider upgrade",
            &[
                (
                    Impact::Degraded,
                    now - Delta::days(3),
                    "Token issuance will be briefly interrupted while we roll out the new identity provider release.",
                ),
                (
                    Impact::None,
                    now - Delta::days(3) + Delta::minutes(50),
                    "The upgrade completed successfully and all services are operating normally.",
                ),
            ],
        ),
    ]
}

// --- Fixture builders -----------------------------------------------------------------------

/// The number of hourly history buckets a probe accumulates before the agent trims them — the widest
/// the availability strip ever gets.
const HISTORY_BUCKETS: i64 = 48;

/// How a probe's history behaves across the window.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// Uninterrupted success.
    Solid,
    /// Occasional single-sample blips that never trip the debounce.
    Flaky,
    /// A sustained outage part-way through the window which has since recovered.
    Recovered,
    /// Currently failing.
    Failing,
}

/// A tiny deterministic PRNG, so the fixture looks organic but renders identically on every load.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

fn tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn node(now: DateTime<Utc>, id: &str, labels: &[(&str, &str)]) -> NodeMetadata {
    NodeMetadata::new(
        id,
        labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        now - Delta::minutes(17),
    )
}

fn peer(id: &str, health: PeerHealth, last_seen: DateTime<Utc>, current: bool) -> Peer {
    Peer {
        id: id.into(),
        last_seen,
        health,
        current,
        node: None,
    }
}

fn probe(
    now: DateTime<Utc>,
    name: &str,
    tag_pairs: &[(&str, &str)],
    shape: Shape,
    seed: u64,
) -> Probe {
    let history = history(now, shape, seed);

    // The probe's headline availability is the sum of everything its observers reported.
    let mut observations: HashMap<String, Observation> = HashMap::new();
    for bucket in &history {
        for (observer, observation) in &bucket.observations {
            let entry = observations.entry(observer.clone()).or_default();
            entry.total_samples += observation.total_samples;
            entry.successful_samples += observation.successful_samples;
            entry.total_retries += observation.total_retries;
            entry.total_latency += observation.total_latency;
        }
    }

    Probe {
        name: name.into(),
        tags: tags(tag_pairs),
        last_updated: now,
        history,
        observations,
        streak: streak(now, shape),
        debounce: None,
        retired: false,
        observers: Default::default(),
        quorum: None,
    }
}

/// The observers reporting on every probe (by node identifier, as the agent records them — the
/// operator view resolves these to hostnames via [`nodes`]), with the baseline latency each
/// contributes.
const OBSERVERS: [(&str, u64); 3] = [("1p3x9kq2m7v4c8", 42), ("7h2wz0r5t9d1na", 180), ("c4j8l1y6b3f0ps", 96)];

fn history(now: DateTime<Utc>, shape: Shape, seed: u64) -> Vec<ProbeHistoryBucket> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    // Buckets are hour-aligned, oldest first, ending with the hour currently in progress.
    let latest = now - Delta::seconds(now.timestamp().rem_euclid(3600));

    (0..HISTORY_BUCKETS)
        .map(|index| {
            let start_time = latest - Delta::hours(HISTORY_BUCKETS - 1 - index);
            let failure_rate = failure_rate(shape, index, &mut rng);

            let observations: HashMap<String, Observation> = OBSERVERS
                .iter()
                .map(|(observer, base_latency)| {
                    let total_samples = 60;
                    let failed = (total_samples as f64 * failure_rate).round() as u64;
                    let jitter = rng.below(30);
                    (
                        (*observer).to_string(),
                        Observation {
                            total_samples,
                            successful_samples: total_samples - failed.min(total_samples),
                            total_retries: failed / 2,
                            total_latency: Duration::from_millis(
                                (base_latency + jitter) * total_samples,
                            ),
                        },
                    )
                })
                .collect();

            let pass = failure_rate == 0.0;
            ProbeHistoryBucket {
                start_time,
                pass,
                message: if pass {
                    String::new()
                } else {
                    "upstream returned HTTP 503".into()
                },
                validations: [
                    ("http.status_code == 200".to_string(), if pass {
                        ValidationResult::pass()
                    } else {
                        ValidationResult::fail("http.status_code was 503")
                    }),
                    ("http.body.status == 'ok'".to_string(), ValidationResult::pass()),
                ]
                .into_iter()
                .collect(),
                observations,
            }
        })
        .collect()
}

/// The fraction of samples that failed in a bucket, by position in the window.
fn failure_rate(shape: Shape, index: i64, rng: &mut Rng) -> f64 {
    match shape {
        Shape::Solid => 0.0,
        Shape::Flaky if rng.below(9) == 0 => 0.02,
        Shape::Flaky => 0.0,
        Shape::Recovered if (26..=31).contains(&index) => 0.55,
        Shape::Recovered => 0.0,
        Shape::Failing if index >= HISTORY_BUCKETS - 3 => 0.75,
        Shape::Failing => 0.0,
    }
}

fn streak(now: DateTime<Utc>, shape: Shape) -> Streak {
    match shape {
        Shape::Solid => Streak {
            covered_since: Some(now - Delta::days(23)),
            ..Default::default()
        },
        Shape::Flaky => Streak {
            failing_since: Some(now - Delta::hours(5)),
            failing_until: Some(now - Delta::hours(5) + Delta::seconds(20)),
            covered_since: Some(now - Delta::days(23)),
        },
        Shape::Recovered => Streak {
            failing_since: Some(now - Delta::hours(21)),
            failing_until: Some(now - Delta::hours(16)),
            covered_since: Some(now - Delta::days(23)),
        },
        Shape::Failing => Streak {
            failing_since: Some(now - Delta::minutes(94)),
            failing_until: Some(now - Delta::seconds(20)),
            covered_since: Some(now - Delta::days(23)),
        },
    }
}

/// A nightly backup that has been running cleanly — and has accumulated the full run strip.
fn healthy_cron(now: DateTime<Utc>) -> Cron {
    let mut cron = Cron::from_config(
        "backups.nightly-snapshot",
        tags(&[("service", "Storage"), ("owner", "platform"), ("retention", "30d")]),
        CronSchedule::Cron("0 2 * * *".into()),
        Some(Duration::from_secs(45 * 60)),
        None,
    );
    cron.last_updated = now;
    cron.runs = (0..50)
        .map(|index| CronRun {
            started_at: now - Delta::hours(24 * (49 - index)) - Delta::minutes(18),
            status: CronStatus::Succeeded,
            duration: Some(Duration::from_secs(600 + (index as u64 % 7) * 45)),
            reason: None,
        })
        .collect();
    cron.last_checkin = Some(CheckIn {
        at: now - Delta::minutes(8),
        status: CronStatus::Succeeded,
        message: "snapshot uploaded (12.4 GiB)".into(),
    });
    cron.streak.covered_since = Some(now - Delta::days(49));
    cron
}

/// A job that is in flight right now.
fn running_cron(now: DateTime<Utc>) -> Cron {
    let mut cron = Cron::from_config(
        "search.index-rebuild",
        tags(&[("service", "Public API"), ("owner", "search")]),
        CronSchedule::Every(Duration::from_secs(6 * 3600)),
        Some(Duration::from_secs(2 * 3600)),
        None,
    );
    cron.last_updated = now;
    cron.runs = (0..11)
        .map(|index| CronRun {
            started_at: now - Delta::hours(6 * (10 - index)) - Delta::minutes(24),
            status: if index == 10 {
                CronStatus::Running
            } else {
                CronStatus::Succeeded
            },
            duration: (index != 10).then(|| Duration::from_secs(1_400 + index as u64 * 30)),
            reason: None,
        })
        .collect();
    cron.last_checkin = Some(CheckIn {
        at: now - Delta::minutes(12),
        status: CronStatus::Running,
        message: "rebuilding shard 4 of 8".into(),
    });
    cron.streak.covered_since = Some(now - Delta::days(12));
    cron
}

/// A job that never checked in, so the monitor synthesised a missed-run placeholder.
fn missed_cron(now: DateTime<Utc>) -> Cron {
    let mut cron = Cron::from_config(
        "billing.usage-rollup",
        tags(&[("service", "Public API"), ("owner", "billing"), ("tier", "critical")]),
        CronSchedule::Every(Duration::from_secs(3600)),
        None,
        None,
    );
    cron.last_updated = now;
    cron.runs = (0..24)
        .map(|index| {
            let missed = index >= 22;
            CronRun {
                started_at: now - Delta::hours(23 - index) - Delta::hours(2),
                status: if missed {
                    CronStatus::Failed
                } else {
                    CronStatus::Succeeded
                },
                duration: (!missed).then(|| Duration::from_secs(95)),
                reason: missed.then_some(CronRunReason::Missed),
            }
        })
        .collect();
    cron.streak = Streak {
        failing_since: Some(now - Delta::hours(2)),
        failing_until: Some(now - Delta::minutes(3)),
        covered_since: Some(now - Delta::days(30)),
    };
    cron
}

/// A job whose most recent run reported a failure.
fn failed_cron(now: DateTime<Utc>) -> Cron {
    let mut cron = Cron::from_config(
        "identity.key-rotation",
        tags(&[("service", "Identity"), ("owner", "security")]),
        CronSchedule::Cron("30 3 * * 0".into()),
        Some(Duration::from_secs(15 * 60)),
        None,
    );
    cron.last_updated = now;
    cron.runs = (0..8)
        .map(|index| CronRun {
            started_at: now - Delta::days(7 * (7 - index)) - Delta::hours(5),
            status: if index == 7 {
                CronStatus::Failed
            } else {
                CronStatus::Succeeded
            },
            duration: Some(Duration::from_secs(48)),
            reason: None,
        })
        .collect();
    cron.last_checkin = Some(CheckIn {
        at: now - Delta::hours(5),
        status: CronStatus::Failed,
        message: "HSM refused the rotation request".into(),
    });
    cron.streak = Streak {
        failing_since: Some(now - Delta::hours(5)),
        failing_until: Some(now - Delta::minutes(1)),
        covered_since: Some(now - Delta::days(56)),
    };
    cron
}

/// A newly configured job that has never checked in.
fn pending_cron() -> Cron {
    Cron::from_config(
        "reports.monthly-invoice-export",
        tags(&[("service", "Other"), ("owner", "finance")]),
        CronSchedule::Cron("0 6 1 * *".into()),
        None,
        None,
    )
}

fn incident(id: u64, title: &str, updates: &[(Impact, DateTime<Utc>, &str)]) -> IncidentView {
    let incident_id = Identifier::new(id);
    IncidentView::new(
        Incident {
            id: incident_id,
            title: title.into(),
            version: 1,
            deleted: false,
        },
        updates
            .iter()
            .enumerate()
            .map(|(index, (impact, timestamp, message))| IncidentUpdate {
                id: IncidentUpdateId::compose(incident_id, index as u64 + 1),
                impact: *impact,
                timestamp: *timestamp,
                message: (*message).to_string(),
                version: 1,
                deleted: false,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture must exercise every health state the UI can render, since that is the whole point
    /// of demo mode.
    #[test]
    fn fixtures_cover_every_health_state() {
        let now = Utc::now();

        let probes = probes();
        assert!(probes.iter().any(|p| p.passing()), "expected a passing probe");
        assert!(probes.iter().any(|p| !p.passing()), "expected a failing probe");
        assert!(
            probes.iter().all(|p| p.history.len() == HISTORY_BUCKETS as usize),
            "every probe should carry a full history strip"
        );

        let healths: Vec<_> = crons().iter().map(|c| c.health(now, c.window())).collect();
        for expected in [
            grey_api::CronHealth::Succeeded,
            grey_api::CronHealth::Running,
            grey_api::CronHealth::Missing,
            grey_api::CronHealth::Failed,
            grey_api::CronHealth::Pending,
        ] {
            assert!(healths.contains(&expected), "missing {expected:?} in {healths:?}");
        }

        let incidents = incidents();
        assert!(incidents.iter().any(|i| i.is_active()), "expected an active incident");
        assert!(incidents.iter().any(|i| !i.is_active()), "expected a resolved incident");
    }
}
