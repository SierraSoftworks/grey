use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tracing::Level;
use tracing_batteries::prelude::*;

use crate::Probe;
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub probes: Vec<Probe>,

    #[serde(default)]
    pub crons: Vec<CronConfig>,

    /// Webhook endpoints notified when a probe or cron changes state. Each receives the JSON event
    /// payload, signed with its shared secret, for every event its filter matches.
    #[serde(default)]
    pub webhooks: Vec<WebhookConfig>,

    #[serde(default)]
    pub ui: UiConfig,

    #[serde(default)]
    pub cluster: ClusterConfig,

    #[serde(rename = "state")]
    #[serde(default = "default::state")]
    pub state: PathBuf,
}

/// Configuration for a "deadman's switch" cron monitor. A scheduled job reports check-ins to the
/// agent; the schedule and completion detectors flag missed or hung runs relative to these settings.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CronConfig {
    pub name: String,

    /// The expected cadence as a fixed interval. Exactly one of `interval` / `schedule` must be set.
    #[serde(default, with = "humantime_serde::option")]
    pub interval: Option<std::time::Duration>,

    /// The expected cadence as a standard 5-field crontab expression (evaluated in UTC). Exactly one
    /// of `interval` / `schedule` must be set.
    #[serde(default)]
    pub schedule: Option<String>,

    /// How long a run may stay in flight before it reads as overrunning (optional; enables
    /// completion/timeout detection).
    #[serde(default, with = "humantime_serde::option")]
    pub max_duration: Option<std::time::Duration>,

    /// Slack after the next-due time before a late run is called missing (optional; a
    /// schedule-derived default applies otherwise).
    #[serde(default, with = "humantime_serde::option")]
    pub grace: Option<std::time::Duration>,

    /// An optional shared secret required on check-ins; when set, callers must supply it via the
    /// `X-Cron-Token` header or a `token` query parameter.
    #[serde(default)]
    pub token: Option<String>,

    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl CronConfig {
    /// The schedule this cron declares, preferring an explicit crontab `schedule` over `interval`.
    /// (Config validation guarantees exactly one is set; the fallback is purely defensive.)
    fn build_schedule(&self) -> grey_api::CronSchedule {
        match (&self.schedule, self.interval) {
            (Some(expr), _) => grey_api::CronSchedule::Cron(expr.clone()),
            (None, Some(interval)) => grey_api::CronSchedule::Every(interval),
            (None, None) => grey_api::CronSchedule::Every(std::time::Duration::from_secs(3600)),
        }
    }

    /// A bare [`grey_api::Cron`] carrying this configuration, used to seed the pooled view.
    pub fn to_cron(&self) -> grey_api::Cron {
        grey_api::Cron::from_config(
            self.name.clone(),
            self.tags.clone(),
            self.build_schedule(),
            self.max_duration,
            self.grace,
        )
    }

    /// Re-applies this configuration onto a (possibly gossiped) record so display and detection use
    /// the local operator's settings rather than whatever a peer last advertised.
    pub fn stamp(&self, cron: &mut grey_api::Cron) {
        cron.tags = self.tags.clone();
        cron.schedule = self.build_schedule();
        cron.max_duration = self.max_duration;
        cron.grace = self.grace;
    }
}

/// A webhook endpoint notified when a probe or cron changes state. The agent posts the JSON event
/// payload to `endpoint`, signs it with `secret` (see the HMAC scheme on [`WebhookConfig::secret`]),
/// and only delivers events for which `filter` evaluates to true.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct WebhookConfig {
    /// A descriptive name for this webhook, used in logs and traces. Defaults to the endpoint when
    /// omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The destination endpoint that receives the JSON event payload via an HTTP `POST`.
    pub endpoint: String,

    /// An optional shared secret. When set, every delivery carries a `Grey-Webhook-Signature` header
    /// of the form `t=<unix-seconds>,v1=<hex>`, where the signature is the HMAC-SHA256 of
    /// `"<timestamp>.<raw-json-body>"` keyed by the secret — the scheme documented for Tailscale
    /// webhooks (<https://tailscale.com/docs/features/webhooks#verifying-an-event-signature>). The
    /// receiver recomputes it to authenticate the payload. When unset, deliveries are unsigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,

    /// Additional headers attached to every delivery (for example an `Authorization` token expected
    /// by the receiving platform). These are sent alongside Grey's own signature/metadata headers,
    /// but they are **not** covered by the signature (which authenticates only the timestamp and
    /// body), so a receiver must not treat them as authenticated or rely on them being unmodified in
    /// transit.
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// A `filt-rs` expression — the same language as probe `checks` — evaluated against each event to
    /// decide whether it is delivered to this endpoint. The available fields are documented in
    /// `docs/guide/webhooks.md` (`event`, `entity.type`, `entity.name`, `entity.tags.<key>`,
    /// `state.current`, `state.previous`, `state.healthy`, `state.was_healthy`, and
    /// `state.availability`). Defaults to matching every event.
    #[serde(default = "default_webhook_filter")]
    pub filter: filt_rs::Filter,

    /// The per-delivery HTTP timeout.
    #[serde(default = "default_webhook_timeout", with = "humantime_serde")]
    pub timeout: std::time::Duration,
}

impl WebhookConfig {
    /// A label for logs/traces: the configured `name`, falling back to the endpoint.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.endpoint)
    }
}

/// The default webhook filter matches every event, so a webhook with no `filter` receives all state
/// changes.
fn default_webhook_filter() -> filt_rs::Filter {
    filt_rs::Filter::new("true").expect("the match-all webhook filter must parse")
}

fn default_webhook_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(10)
}

impl Config {
    #[cfg(test)]
    pub fn test(temp_dir: &PathBuf) -> Self {
        Self {
            probes: vec![
                Probe::test(),
            ],
            crons: vec![],
            webhooks: vec![],
            ui: UiConfig::default(),
            cluster: ClusterConfig::default(),
            state: temp_dir.join("test_state.redb"),
        }
    }

    #[tracing::instrument(name = "config.load", skip(path), err(Debug))]
    pub async fn load_from_path(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config = tokio::fs::read_to_string(path).await.map_err(|e| {
            error!(name: "config.load", { config.path=%path.display(), exception = %e }, "Failed to load configuration file from {}: {}", path.display(), e);
            let err: Box<dyn std::error::Error> = format!("Failed to load configuration file from {}: {}", path.display(), e).into();
            err
        })?;

        let config: Self = serde_yaml::from_str(&config)?;
        config.validate_crons()?;
        config.validate_webhooks()?;
        Ok(config)
    }

    /// Validates each webhook's destination: an endpoint must be present and an absolute `http(s)`
    /// URL, so a typo fails the load rather than silently dropping every notification. The `filter`
    /// expression is already validated during deserialization (it is a parsed [`filt_rs::Filter`]).
    fn validate_webhooks(&self) -> Result<(), Box<dyn std::error::Error>> {
        for webhook in &self.webhooks {
            let endpoint = webhook.endpoint.trim();
            if endpoint.is_empty() {
                return Err(format!(
                    "Webhook '{}' must declare a non-empty `endpoint`.",
                    webhook.label()
                )
                .into());
            }

            if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
                return Err(format!(
                    "Webhook '{}' has an invalid `endpoint` '{}'; it must be an http(s) URL.",
                    webhook.label(),
                    webhook.endpoint
                )
                .into());
            }
        }
        Ok(())
    }

    /// Validates that each cron declares exactly one of `interval` / `schedule`, that any crontab
    /// expression parses, and that no cron shares a name with a probe — so a misconfiguration fails
    /// the load rather than silently misbehaving. The name check is what lets gossip key replicated
    /// state by the bare entity name (the `ReplicatedEntity` variant carries the type); without it a
    /// same-named probe and cron would collide in a peer's per-node diff map.
    fn validate_crons(&self) -> Result<(), Box<dyn std::error::Error>> {
        for cron in &self.crons {
            if self.probes.iter().any(|probe| probe.name == cron.name) {
                return Err(format!(
                    "Cron '{}' has the same name as a probe; names must be unique across probes and crons.",
                    cron.name
                )
                .into());
            }

            match (&cron.schedule, cron.interval) {
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "Cron '{}' sets both `interval` and `schedule`; set exactly one.",
                        cron.name
                    )
                    .into());
                }
                (None, None) => {
                    return Err(format!(
                        "Cron '{}' must set either `interval` or `schedule`.",
                        cron.name
                    )
                    .into());
                }
                (Some(expr), None) => {
                    if !grey_api::CronSchedule::Cron(expr.clone()).is_valid() {
                        return Err(format!(
                            "Cron '{}' has an invalid crontab `schedule`: '{expr}'.",
                            cron.name
                        )
                        .into());
                    }
                }
                (None, Some(_)) => {}
            }
        }
        Ok(())
    }

    #[tracing::instrument(name = "config.reload", level=Level::DEBUG, skip(path), err(Debug))]
    pub async fn load_if_modified_since(
        path: &Path,
        last_modified: SystemTime,
    ) -> Result<Option<(Config, SystemTime)>, Box<dyn std::error::Error>> {
        let metadata = tokio::fs::metadata(path).await.map_err(|e| {
            error!(name: "config.reload", { config.path=%path.display(), exception = %e }, "Failed to get metadata for {}: {}", path.display(), e);
            let err: Box<dyn std::error::Error> = format!("Failed to get metadata for {}: {}", path.display(), e).into();
            err
        })?;

        let modified = metadata.modified()?;
        if modified > last_modified {
            let config = Self::load_from_path(path).await?;
            Ok(Some((config, modified)))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default::ui::listen")]
    pub listen: String,

    #[serde(default = "default::ui::title")]
    pub title: String,
    #[serde(default = "default::ui::logo")]
    pub logo: String,

    #[serde(default)]
    pub links: Vec<grey_api::UiLink>,

    #[serde(default = "default::ui::reload_interval")]
    #[serde(with = "humantime_serde")]
    pub reload_interval: std::time::Duration,

    /// Optional administrative access configuration. When present, the admin API is protected by
    /// OIDC bearer-token validation plus the configured access-control list. When absent, the admin
    /// API is closed entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin: Option<AdminConfig>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: default::ui::listen(),
            title: default::ui::title(),
            logo: default::ui::logo(),
            links: vec![],
            reload_interval: default::ui::reload_interval(),
            admin: None,
        }
    }
}

/// Administrative access configuration for the incident-management API.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminConfig {
    /// A `filt-rs` expression evaluated against the validated token claims (exposed under the
    /// `claims.` prefix) plus the request `method`/`path`. It must evaluate to true for a request to
    /// be authorized. Defaults to denying every request, so the admin area is closed until an ACL is
    /// explicitly configured.
    #[serde(default = "default_admin_acl")]
    pub acl: filt_rs::Filter,

    /// OIDC parameters. The agent validates bearer tokens against this provider; the public subset
    /// (issuer, client id, scopes) is also surfaced to the SPA so it can run the browser-side login.
    pub oidc: OidcConfig,
}

/// OIDC provider configuration. The browser runs the Authorization Code flow but hands the code to
/// the agent for exchange, so the agent holds the confidential `client_secret`; it never reaches the
/// browser.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct OidcConfig {
    /// The provider's issuer / base URL, used to discover endpoints and JWKS and as the expected
    /// token issuer.
    pub endpoint: String,
    /// The OAuth2 client id, also the expected audience of validated ID tokens. Surfaced to the SPA.
    pub client_id: String,
    /// The OAuth2 client secret, used by the agent (only) to exchange authorization codes for
    /// tokens. Never exposed to the browser.
    pub client_secret: String,
    /// Additional scopes the SPA should request beyond the implicit `openid`.
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// The default admin ACL denies every request, so administrative access is closed until an operator
/// opts in with an explicit expression.
fn default_admin_acl() -> filt_rs::Filter {
    filt_rs::Filter::new("false").expect("the deny-all ACL expression must parse")
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ClusterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default::cluster::listen")]
    pub listen: String,
    /// The address other nodes should use to reach this one, advertised through the membership
    /// gossip so peers can be discovered transitively. When unset it falls back to `listen` if that
    /// is a concrete (non-wildcard) address; a wildcard `listen` with no `advertised_address` means
    /// this node self-advertises nothing (it is still discovered via the source address of its
    /// packets).
    #[serde(default)]
    pub advertised_address: Option<String>,
    pub peers: Vec<String>,
    pub secret: String,
    #[serde(default)]
    pub secrets: Vec<String>,

    #[serde(default = "default::cluster::gossip_interval")]
    #[serde(with = "humantime_serde")]
    pub gossip_interval: std::time::Duration,
    #[serde(default = "default::cluster::gossip_factor")]
    pub gossip_factor: usize,

    /// The maximum size, in bytes, of a gossip datagram this node will emit; larger messages are
    /// partitioned across rounds. Accepts the former `max_message_size` name for compatibility.
    #[serde(default = "default::cluster::message_mtu")]
    #[serde(alias = "max_message_size")]
    pub message_mtu: usize,

    /// Phi-accrual suspicion threshold; a peer whose phi exceeds this is considered suspect/dead.
    #[serde(default = "default::cluster::phi_threshold")]
    pub phi_threshold: f64,
    /// How long a peer has to answer a gossip message before that send counts as a missed exchange
    /// for the link's health (driving the per-address retry backoff).
    #[serde(default = "default::cluster::reply_timeout")]
    #[serde(with = "humantime_serde")]
    pub reply_timeout: std::time::Duration,

    #[serde(default = "default::cluster::peer_resolve_interval")]
    #[serde(with = "humantime_serde")]
    pub peer_resolve_interval: std::time::Duration,

    #[serde(default = "default::cluster::gc_interval")]
    #[serde(with = "humantime_serde")]
    pub gc_interval: std::time::Duration,
    #[serde(default = "default::cluster::gc_probe_expiry")]
    #[serde(with = "humantime_serde")]
    pub gc_probe_expiry: std::time::Duration,
    #[serde(default = "default::cluster::gc_peer_expiry")]
    #[serde(with = "humantime_serde")]
    pub gc_peer_expiry: std::time::Duration,
}

impl ClusterConfig {
    /// The addresses this node advertises about itself through membership gossip: the configured
    /// `advertised_address`, falling back to `listen` when that is a concrete (non-wildcard)
    /// address. Empty when neither yields a routable address, in which case the node is still
    /// discovered via the source address of its gossip messages.
    pub fn advertised_addresses(&self) -> Vec<String> {
        self.advertised_address
            .clone()
            .or_else(|| match self.listen.parse::<std::net::SocketAddr>() {
                Ok(addr) if !addr.ip().is_unspecified() => Some(self.listen.clone()),
                _ => None,
            })
            .into_iter()
            .collect()
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: default::cluster::listen(),
            advertised_address: None,
            peers: vec![],
            secret: "".into(),
            secrets: vec![],
            gossip_interval: default::cluster::gossip_interval(),
            gossip_factor: default::cluster::gossip_factor(),
            message_mtu: default::cluster::message_mtu(),
            phi_threshold: default::cluster::phi_threshold(),
            reply_timeout: default::cluster::reply_timeout(),
            peer_resolve_interval: default::cluster::peer_resolve_interval(),
            gc_interval: default::cluster::gc_interval(),
            gc_probe_expiry: default::cluster::gc_probe_expiry(),
            gc_peer_expiry: default::cluster::gc_peer_expiry(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped `checks` example must parse through the real configuration
    /// loader, exercising `filt-rs` deserialization end-to-end and guarding the
    /// example against drift.
    #[tokio::test]
    async fn loads_checks_example() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../example/checks.yml");
        let config = Config::load_from_path(&path)
            .await
            .expect("example/checks.yml should load");

        let probe = config
            .probes
            .iter()
            .find(|p| p.name == "example.checks")
            .expect("example.checks probe should be present");
        assert_eq!(probe.checks.len(), 2);

        // A check renders as its raw expression, which is what gets reported.
        let github = config
            .probes
            .iter()
            .find(|p| p.name == "github.repo")
            .expect("github.repo probe should be present");
        assert_eq!(github.checks.len(), 2);
        assert_eq!(
            github.checks[1].to_string(),
            r#"http.header.content-type matches r"^text/html""#
        );
    }

    /// The shipped `crons` example must parse through the real configuration loader, guarding the
    /// example against drift and exercising the `CronConfig` (humantime) deserialization.
    #[tokio::test]
    async fn loads_crons_example() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../example/crons.yml");
        let config = Config::load_from_path(&path)
            .await
            .expect("example/crons.yml should load");

        let backup = config
            .crons
            .iter()
            .find(|c| c.name == "backup.nightly")
            .expect("backup.nightly cron should be present");
        assert_eq!(backup.schedule.as_deref(), Some("0 2 * * *"));
        assert_eq!(backup.interval, None);
        assert_eq!(backup.max_duration, Some(std::time::Duration::from_secs(30 * 60)));
        assert_eq!(backup.grace, Some(std::time::Duration::from_secs(60 * 60)));

        let sync = config
            .crons
            .iter()
            .find(|c| c.name == "sync.hourly")
            .expect("sync.hourly cron should be present");
        assert_eq!(sync.interval, Some(std::time::Duration::from_secs(60 * 60)));
        assert_eq!(sync.schedule, None);
        assert_eq!(sync.token.as_deref(), Some("change-me"));
    }

    /// A cron with an invalid crontab `schedule`, or that sets neither/both of `interval`/`schedule`,
    /// must fail to load rather than silently misbehaving.
    #[tokio::test]
    async fn rejects_invalid_cron_schedules() {
        let dir = tempfile::tempdir().unwrap();

        let cases = [
            // Invalid crontab expression.
            "crons:\n  - name: bad\n    schedule: 'not a cron'\n",
            // Neither interval nor schedule.
            "crons:\n  - name: bad\n    max_duration: 1m\n",
            // Both interval and schedule.
            "crons:\n  - name: bad\n    interval: 1h\n    schedule: '* * * * *'\n",
        ];

        for (i, body) in cases.iter().enumerate() {
            let path = dir.path().join(format!("bad-{i}.yml"));
            tokio::fs::write(&path, body).await.unwrap();
            assert!(
                Config::load_from_path(&path).await.is_err(),
                "config #{i} should be rejected: {body}"
            );
        }

        // A well-formed crontab cron loads.
        let ok = dir.path().join("ok.yml");
        tokio::fs::write(&ok, "crons:\n  - name: good\n    schedule: '*/5 * * * *'\n")
            .await
            .unwrap();
        assert!(Config::load_from_path(&ok).await.is_ok());
    }

    /// The shipped `webhooks` example must parse through the real configuration loader, guarding the
    /// example against drift and exercising the `WebhookConfig` (filt-rs filter + humantime timeout)
    /// deserialization.
    #[tokio::test]
    async fn loads_webhooks_example() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../example/webhooks.yml");
        let config = Config::load_from_path(&path)
            .await
            .expect("example/webhooks.yml should load");

        assert_eq!(config.webhooks.len(), 3);

        let pagerduty = config
            .webhooks
            .iter()
            .find(|w| w.name.as_deref() == Some("pagerduty"))
            .expect("the pagerduty webhook should be present");
        assert!(pagerduty.secret.is_some());
        assert_eq!(pagerduty.filter.raw(), "state.healthy == false");
        assert_eq!(
            pagerduty.headers.get("Authorization").map(String::as_str),
            Some("Token token=xxxxxxxxxxxxxxxxxxxx")
        );
        // The default timeout applies when none is configured.
        assert_eq!(pagerduty.timeout, std::time::Duration::from_secs(10));

        // An explicit timeout is honoured.
        let chat = config
            .webhooks
            .iter()
            .find(|w| w.name.as_deref() == Some("platform-chat"))
            .expect("the platform-chat webhook should be present");
        assert_eq!(chat.timeout, std::time::Duration::from_secs(5));

        // A webhook with no filter defaults to matching everything, and one with no secret is
        // unsigned.
        let orchestrator = config
            .webhooks
            .iter()
            .find(|w| w.name.as_deref() == Some("job-orchestrator"))
            .expect("the job-orchestrator webhook should be present");
        assert!(orchestrator.secret.is_none());
    }

    /// A webhook with a missing or non-http(s) endpoint must fail to load rather than silently
    /// dropping every notification.
    #[tokio::test]
    async fn rejects_invalid_webhook_endpoints() {
        let dir = tempfile::tempdir().unwrap();

        let cases = [
            // Empty endpoint.
            "webhooks:\n  - endpoint: ''\n",
            // Not an http(s) URL.
            "webhooks:\n  - endpoint: 'ftp://example.com/hook'\n",
        ];

        for (i, body) in cases.iter().enumerate() {
            let path = dir.path().join(format!("bad-webhook-{i}.yml"));
            tokio::fs::write(&path, body).await.unwrap();
            assert!(
                Config::load_from_path(&path).await.is_err(),
                "webhook config #{i} should be rejected: {body}"
            );
        }

        // A well-formed webhook loads, defaulting its filter to match-all.
        let ok = dir.path().join("ok.yml");
        tokio::fs::write(&ok, "webhooks:\n  - endpoint: https://example.com/hook\n")
            .await
            .unwrap();
        let config = Config::load_from_path(&ok).await.expect("a valid webhook should load");
        assert_eq!(config.webhooks[0].filter.raw(), "true");
    }

    /// A cron may not share a name with a probe: gossip keys replicated state by the bare entity
    /// name (the type is carried by the `ReplicatedEntity` variant), so a clash must be rejected at
    /// load rather than colliding on the wire.
    #[tokio::test]
    async fn rejects_cron_sharing_a_probe_name() {
        let dir = tempfile::tempdir().unwrap();
        let probe = "probes:\n  - name: backup\n    policy: { interval: 5s, timeout: 2s }\n    target: !Http\n      url: https://example.com\n";

        let clash = dir.path().join("clash.yml");
        tokio::fs::write(&clash, format!("{probe}crons:\n  - name: backup\n    interval: 1h\n"))
            .await
            .unwrap();
        assert!(Config::load_from_path(&clash).await.is_err(), "a cron named like a probe must be rejected");

        // Distinct names load fine.
        let ok = dir.path().join("ok.yml");
        tokio::fs::write(&ok, format!("{probe}crons:\n  - name: backup.cron\n    interval: 1h\n"))
            .await
            .unwrap();
        assert!(Config::load_from_path(&ok).await.is_ok());
    }
}

mod default {
    use super::*;

    pub fn state() -> PathBuf {
        PathBuf::from("state.redb")
    }

    pub mod ui {
        pub fn listen() -> String {
            "0.0.0.0:8888".into()
        }

        pub fn title() -> String {
            "Grey".into()
        }

        pub fn logo() -> String {
            "https://cdn.sierrasoftworks.com/logos/icon.svg".into()
        }

        pub fn reload_interval() -> std::time::Duration {
            std::time::Duration::from_secs(60)
        }
    }

    pub mod cluster {
        pub fn listen() -> String {
            "0.0.0.0:8888".into()
        }

        pub fn gossip_interval() -> std::time::Duration {
            std::time::Duration::from_secs(30)
        }

        pub fn gossip_factor() -> usize {
            2
        }

        pub fn message_mtu() -> usize {
            // A conservative default: small enough that a lost datagram costs little and large
            // enough to carry plenty per round. Raise it (up to ~65507) for fewer rounds on
            // reliable links, or lower it below the path MTU to avoid IP fragmentation. Over-large
            // gossip messages are partitioned across rounds regardless.
            8 * 1024
        }

        pub fn peer_resolve_interval() -> std::time::Duration {
            std::time::Duration::from_secs(60)
        }

        pub fn phi_threshold() -> f64 {
            8.0
        }

        pub fn reply_timeout() -> std::time::Duration {
            // UDP replies arrive within a network round trip; five seconds tolerates slow links
            // and processing delays without conflating latency with loss.
            std::time::Duration::from_secs(5)
        }

        pub fn gc_interval() -> std::time::Duration {
            std::time::Duration::from_secs(5 * 60)
        }

        pub fn gc_probe_expiry() -> std::time::Duration {
            std::time::Duration::from_secs(7 * 24 * 60 * 60)
        }

        pub fn gc_peer_expiry() -> std::time::Duration {
            std::time::Duration::from_secs(30 * 60)
        }
    }
}
