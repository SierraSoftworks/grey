//! Aggregated OpenTelemetry metrics for Grey's probes, crons, and cluster gossip.
//!
//! Spans carry the full detail of every probe execution, but querying them over long windows is
//! costly. The instruments here provide cheap, pre-aggregated counterparts (exported through the
//! `tracing-batteries` OpenTelemetry battery) so failure rates and latencies can be charted and
//! alerted on directly, with the detailed span or log record one trace-ID hop away.
//!
//! Every measurement is recorded while the relevant span is entered, so the active OpenTelemetry
//! context carries that span's IDs and exemplars attach to the data points once the Rust SDK
//! emits them.

use std::fmt::Display;
use std::sync::OnceLock;
use std::time::Duration;

use grey_api::{CronRunReason, CronStatus};
use tracing_batteries::prelude::opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
};

/// Histogram buckets (seconds) for probe latencies, which range from a few milliseconds for a
/// local TCP connect to tens of seconds for a slow script.
const PROBE_LATENCY_BOUNDARIES: [f64; 14] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0,
];

/// Histogram buckets (seconds) for cron run durations, which range from seconds to most of a day.
const CRON_DURATION_BOUNDARIES: [f64; 15] = [
    1.0, 5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0, 7200.0, 14400.0,
    28800.0, 86400.0,
];

/// The outcome of a scheduled probe execution, as reported on `probe_total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    Pass,
    Fail,
    Timeout,
}

impl ProbeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ProbeStatus::Pass => "pass",
            ProbeStatus::Fail => "fail",
            ProbeStatus::Timeout => "timeout",
        }
    }
}

/// Whether a gossip message was sent by this node or received from a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GossipDirection {
    Sent,
    Received,
}

impl GossipDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            GossipDirection::Sent => "sent",
            GossipDirection::Received => "received",
        }
    }
}

/// Resolves the `(origin, target)` node pair for a gossip message so that every message in one
/// Syn → SynAck → Ack exchange is labelled identically on both nodes: the origin is always the
/// node that initiated the exchange with a `syn`, and the target the node that answered with a
/// `synack`. A node therefore flips its own role when it emits or receives a `synack`.
pub fn handshake_parties(
    kind: &str,
    direction: GossipDirection,
    self_id: String,
    peer_id: String,
) -> (String, String) {
    let initiated_by_self = match (kind, direction) {
        ("synack", GossipDirection::Sent) => false,
        ("synack", GossipDirection::Received) => true,
        (_, GossipDirection::Sent) => true,
        (_, GossipDirection::Received) => false,
    };

    if initiated_by_self {
        (self_id, peer_id)
    } else {
        (peer_id, self_id)
    }
}

/// The instruments Grey reports on, created once against the global meter.
pub struct Metrics {
    host_name: String,
    probe_total: Counter<u64>,
    probe_retries_total: Counter<u64>,
    probe_latency: Histogram<f64>,
    cron_total: Counter<u64>,
    cron_duration: Histogram<f64>,
    gossip_message_total: Counter<u64>,
}

/// The process-wide instruments. These are created on first use, which must happen after the
/// telemetry session has installed its meter provider (instruments stay bound to whichever
/// provider was global when they were built).
pub fn metrics() -> &'static Metrics {
    static METRICS: OnceLock<Metrics> = OnceLock::new();
    METRICS.get_or_init(Metrics::new)
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("grey");

        Self {
            host_name: gethostname::gethostname().to_string_lossy().into_owned(),
            probe_total: meter
                .u64_counter("probe_total")
                .with_description("Scheduled probe executions by outcome.")
                .build(),
            probe_retries_total: meter
                .u64_counter("probe_retries_total")
                .with_description("Probe attempts which failed and were retried.")
                .build(),
            probe_latency: meter
                .f64_histogram("probe_latency_histogram")
                .with_description("Duration of the probe attempt that decided the outcome.")
                .with_unit("s")
                .with_boundaries(PROBE_LATENCY_BOUNDARIES.to_vec())
                .build(),
            cron_total: meter
                .u64_counter("cron_total")
                .with_description(
                    "Cron check-ins and monitor detections (missed or stuck runs) by status.",
                )
                .build(),
            cron_duration: meter
                .f64_histogram("cron_duration_histogram")
                .with_description("Duration of completed cron runs.")
                .with_unit("s")
                .with_boundaries(CRON_DURATION_BOUNDARIES.to_vec())
                .build(),
            gossip_message_total: meter
                .u64_counter("gossip_message_total")
                .with_description("Cluster gossip messages sent and received by kind.")
                .build(),
        }
    }

    fn common(&self, node_id: &dyn Display) -> [KeyValue; 2] {
        [
            KeyValue::new("host.name", self.host_name.clone()),
            KeyValue::new("node.id", node_id.to_string()),
        ]
    }

    /// Records the outcome and latency of one scheduled probe execution.
    pub fn record_probe(
        &self,
        probe: &str,
        node_id: &dyn Display,
        status: ProbeStatus,
        latency: Duration,
    ) {
        let mut attributes = self.common(node_id).to_vec();
        attributes.push(KeyValue::new("probe.name", probe.to_owned()));
        attributes.push(KeyValue::new("status", status.as_str()));

        self.probe_total.add(1, &attributes);
        self.probe_latency.record(latency.as_secs_f64(), &attributes);
    }

    /// Records that a failed probe attempt is about to be retried.
    pub fn record_probe_retry(&self, probe: &str, node_id: &dyn Display) {
        let mut attributes = self.common(node_id).to_vec();
        attributes.push(KeyValue::new("probe.name", probe.to_owned()));

        self.probe_retries_total.add(1, &attributes);
    }

    /// Records a cron check-in or a monitor detection. `reason` is set for synthetic runs the
    /// monitor materialised (missed or stuck), and `duration` when a terminal check-in closed a run
    /// whose start was reported.
    pub fn record_cron(
        &self,
        cron: &str,
        node_id: &dyn Display,
        status: CronStatus,
        reason: Option<CronRunReason>,
        duration: Option<Duration>,
    ) {
        let mut attributes = self.common(node_id).to_vec();
        attributes.push(KeyValue::new("cron.name", cron.to_owned()));
        attributes.push(KeyValue::new("status", status.as_str()));
        if let Some(reason) = reason {
            attributes.push(KeyValue::new("reason", reason.as_str()));
        }

        self.cron_total.add(1, &attributes);
        if let Some(duration) = duration {
            self.cron_duration.record(duration.as_secs_f64(), &attributes);
        }
    }

    /// Records a gossip message. `peer_id` is `None` when a message is sent to a seed address whose
    /// node has not yet identified itself.
    pub fn record_gossip(
        &self,
        kind: &str,
        direction: GossipDirection,
        self_id: &dyn Display,
        peer_id: Option<&dyn Display>,
        ok: bool,
    ) {
        let peer_id = peer_id
            .map(ToString::to_string)
            .unwrap_or_else(|| "unknown".to_owned());
        let (origin, target) =
            handshake_parties(kind, direction, self_id.to_string(), peer_id);

        let mut attributes = self.common(self_id).to_vec();
        attributes.push(KeyValue::new("kind", kind.to_owned()));
        attributes.push(KeyValue::new("direction", direction.as_str()));
        attributes.push(KeyValue::new("status", if ok { "ok" } else { "error" }));
        attributes.push(KeyValue::new("origin.node", origin));
        attributes.push(KeyValue::new("target.node", target));

        self.gossip_message_total.add(1, &attributes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both sides of a handshake label every message with the same origin/target pair: the origin
    /// is the node that sent the Syn, whichever node is emitting the measurement.
    #[test]
    fn handshake_parties_are_stable_across_both_nodes() {
        let cases = [
            // (kind, direction, expected initiator is self)
            ("syn", GossipDirection::Sent, true),
            ("syn", GossipDirection::Received, false),
            ("synack", GossipDirection::Sent, false),
            ("synack", GossipDirection::Received, true),
            ("ack", GossipDirection::Sent, true),
            ("ack", GossipDirection::Received, false),
            ("members", GossipDirection::Sent, true),
            ("members", GossipDirection::Received, false),
        ];

        for (kind, direction, self_is_origin) in cases {
            let (origin, target) = handshake_parties(kind, direction, "me".into(), "peer".into());
            let expected = if self_is_origin { ("me", "peer") } else { ("peer", "me") };
            assert_eq!(
                (origin.as_str(), target.as_str()),
                expected,
                "{kind} {direction:?}"
            );
        }
    }

    /// Recording through the instruments must be safe even when no meter provider has been
    /// installed (tests, or a deployment without an OTLP endpoint).
    #[test]
    fn recording_without_a_provider_is_a_no_op() {
        let m = metrics();
        m.record_probe("probe", &"node", ProbeStatus::Fail, Duration::from_millis(5));
        m.record_probe_retry("probe", &"node");
        m.record_cron(
            "cron",
            &"node",
            CronStatus::Failed,
            Some(CronRunReason::Missed),
            None,
        );
        m.record_cron(
            "cron",
            &"node",
            CronStatus::Succeeded,
            None,
            Some(Duration::from_secs(3)),
        );
        m.record_gossip("syn", GossipDirection::Sent, &"node", None, true);
    }
}
