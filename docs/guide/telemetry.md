# Telemetry
Grey emits OpenTelemetry traces, metrics, and logs to a single OTLP endpoint. Traces carry the
full detail of every probe execution, metrics provide cheap pre-aggregated counters and histograms
for dashboards and long-lookback queries, and logs record failures with the trace they belong to.

To configure telemetry emission for Grey, you should use the following environment variables.

```bash
# Jaeger running on your local machine
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317

# Honeycomb
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.honeycomb.io:443
export OTEL_EXPORTER_OTLP_HEADERS=x-honeycomb-team=YOUR_API_KEY

# New Relic
export OTEL_EXPORTER_OTLP_ENDPOINT=https://otlp.eu01.nr-data.net:443
export OTEL_EXPORTER_OTLP_HEADERS=api-key=YOUR_API_KEY

# Lightstep
export OTEL_EXPORTER_OTLP_ENDPOINT=https://ingest.lightstep.com:443
export OTEL_EXPORTER_OTLP_HEADERS=lightstep-access-token=YOUR_API_KEY
```

## `OTEL_EXPORTER_OTLP_ENDPOINT`
This environment variable should be set to the URL of the OpenTelemetry collector endpoint
that you wish to emit telemetry to. This should be a gRPC compatible endpoint and should
use a trusted TLS certificate (e.g. a certificate signed by a well known CA).

## `OTEL_EXPORTER_OTLP_HEADERS`
This environment variable should be set to a comma separated list of headers that should be
sent with each telemetry emission. This is useful for providing authentication credentials
to the OpenTelemetry collector endpoint and is required by some providers (e.g. Honeycomb).

You can provide multiple headers by separating them with a comma. For example, to provide
a legacy Honeycomb team and dataset, you would specify:
`x-honeycomb-team=YOUR_TEAM,x-honeycomb-dataset=YOUR_DATASET`.

## Metrics
Metrics are exported every `OTEL_METRIC_EXPORT_INTERVAL` milliseconds (default 60s) with cumulative
temporality. Every series carries `host.name` and `node.id` alongside the attributes listed below
(Prometheus-based backends render these as `host_name`, `node_id`, `probe_name`, and so on).

| Metric | Attributes | Description |
|---|---|---|
| `probe_total` | `probe.name`, `status` (`pass`, `fail`, `timeout`) | Scheduled probe executions by outcome. |
| `probe_retries_total` | `probe.name` | Attempts which failed and were retried under the probe's policy. |
| `probe_latency_histogram` | `probe.name`, `status` | Duration (seconds) of the attempt that decided the outcome. |
| `cron_total` | `cron.name`, `status` (`running`, `succeeded`, `failed`), `reason` (`missed`, `stuck`; detections only) | Cron check-ins and monitor detections. |
| `cron_duration_histogram` | `cron.name`, `status` | Duration (seconds) of runs which reported both a start and a completion. |
| `gossip_message_total` | `kind` (`syn`, `synack`, `ack`, `members`), `direction` (`sent`, `received`), `status` (`ok`, `error`), `origin.node`, `target.node` | Cluster gossip traffic. |

Gossip messages are labelled by handshake rather than by sender: `origin.node` is always the node
that opened the exchange with a `syn` and `target.node` the node that answered with a `synack`, so
both sides of one exchange share the same pair and can be correlated directly.

Measurements are recorded inside the corresponding span, so exemplars linking a data point to its
trace will attach automatically once the Rust OpenTelemetry SDK emits them.

## Logs
Log records carry the trace and span IDs of the span they were emitted within, so a failure can be
found by aggregating logs and then opened as a trace. Severity follows the operational impact:

- **Error**: a probe that failed after exhausting its attempts or timed out (with the target, the
  failing checks, and the error), a cron check-in reporting a failure, and a missed or stuck cron
  run detected by the monitor.
- **Warning**: a probe attempt that failed and is being retried, with the error that caused it.
- **Info**: startup and shutdown, configuration reloads, garbage collection passes, and the start
  of background tasks (gossip, notifier, cron monitor).
- **Debug**: successful probe runs, routine cron check-ins, and cron monitor passes.

The exported level is controlled by `LOG_LEVEL` (`error`, `warn`, `info`, `debug`, or `trace`) and
defaults to `info`, so routine probe activity is only exported when explicitly requested.
