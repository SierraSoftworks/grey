# Grey
**Lightweight, clustered health probing with native OpenTelemetry support**

Grey is a health probing system designed to simplify the regular checking of service
health from external locations. It is built in Rust, ships as a single self-contained
binary, and integrates native support for OpenTelemetry — giving operators a detailed
view into how their systems behave and how they fail.

Grey is small enough to run on every server in your fleet, and its built-in
[clustering](#clustering) lets those instances pool their results so that many vantage
points contribute to a single, shared view of your platform's health.

## Highlights
- **Clustered probing.** Run Grey on as many machines as you like and have them
  cooperate: every node probes independently and gossips its results, so the cluster
  converges on one view of service health measured from many locations at once. Workers
  can be headless while a central node serves the status page.
- **Cron (deadman's-switch) monitoring.** Track scheduled jobs — backups, batch jobs,
  anything that can't be probed from the outside — by having them *check in*. Grey raises
  the alarm when an expected run goes missing or overruns.
- **Many probe types.** HTTP, gRPC, TCP, DNS, and custom JavaScript probes.
- **Expressive checks.** Validate responses with [`filt-rs`](https://docs.rs/filt-rs)
  expressions: ranges, regular expressions, membership, and relationships between fields.
- **Webhook notifications.** Deliver a signed JSON event to incident-management and
  automation tools whenever a probe or cron changes state, filtered with the same
  [`filt-rs`](https://docs.rs/filt-rs) expression language as checks.
- **Native OpenTelemetry.** Export traces (with propagation) to any OTLP endpoint.
- **Built-in status page.** An optional, brandable status page with customer-facing
  incident management.

## Quick start
Grey is a single binary that takes a YAML configuration file as its only required input.
Create a `config.yaml`:

```yaml
probes:
  - name: google.search
    policy:
      interval: 5s
      timeout: 2s
      retries: 3
    target: !Http
      url: https://google.com?q=grey+healthcheck+system
    checks:
      - http.status == 200
      - http.header.content-type contains "text/html"
    tags:
      service: Google

ui:
  enabled: true
  listen: 127.0.0.1:3002
```

Then run Grey against it:

```bash
grey --config config.yaml
```

Grey will start probing `https://google.com` every five seconds and serve a status page
at <http://127.0.0.1:3002>. Each probe runs on its own schedule, retries on failure
according to its `policy`, and fails as soon as any of its `checks` does not match.

> 📖 Full documentation lives at **<https://grey.sierrasoftworks.com>**.

## Configuration overview
A configuration file is composed of a handful of top-level sections:

```yaml
probes: []   # Services to actively probe (see Probes & targets).
crons: []    # Scheduled jobs that check in with Grey (see Cron monitoring).
cluster: {}  # Optional clustering between Grey instances (see Clustering).
ui: {}       # Optional status page (see Status page).
state: state.redb  # Where Grey persists its probe/cron history (defaults to ./state.redb).
```

Grey watches the configuration file and reloads it automatically when it changes, so you
can add or adjust probes without restarting.

## Probes & targets
A **probe** describes a service to check, how often to check it (`policy`), the `target`
to reach, and the `checks` that decide whether the result is healthy. Each target type
exposes a set of fields that your checks can assert against.

### `!Http`
Makes requests to HTTP(S) endpoints. Options:

- `url` (required): The URL to make the request to.
- `method` (optional, default: `GET`): The HTTP method to use for the request.
- `headers` (optional): A map of headers to include in the request.
- `body` (optional): The body to include in the request.
- `no_verify` (optional, default: `false`): Whether to disable TLS verification.

Fields for checks:

- `http.status`: The HTTP status code of the response.
- `http.header.<header>`: The value of a response header (header names are lowercased).
- `http.version`: The HTTP version of the response.
- `http.body`: The body of the response.

### `!Grpc`
Performs health checks against gRPC services using the standard gRPC Health Checking
Protocol. Options:

- `url` (required): The gRPC endpoint URL (e.g. `https://api.example.com:443`).
- `service` (optional): A specific service to check. If omitted, checks overall server health.
- `ca_cert` (optional): Custom CA certificate in PEM format for TLS validation.

Fields for checks:

- `grpc.status`: The health status (`UNKNOWN`, `SERVING`, `NOT_SERVING`, `SERVICE_UNKNOWN`).
- `grpc.status_code`: The numeric representation of the health status (0-3).

### `!Tcp`
Opens a TCP connection to confirm a host and port is accepting connections. Options:

- `host` (required): The host and port to connect to (e.g. `example.com:80`).

Fields for checks:

- `net.ip`: The IP address of the host that was connected to.

### `!Dns`
Runs a DNS query and lets you assert on the response. Options:

- `domain` (required): The domain name to query.
- `record_type` (optional, default: `A`): The type of DNS record to query for.

Fields for checks:

- `dns.answers`: The list of answers to the DNS query.

### `!Script`
Runs a custom JavaScript probe for complex evaluations and workflows. Options:

- `code` (required): The JavaScript code to execute for the probe.
- `args` (optional): A list of string arguments to pass to the script.

The script environment provides standard Web APIs like `fetch`, `console`, `JSON`, and
`setTimeout`. Set custom outputs via the `output` object, and retrieve trace information
with `getTraceId()` and `getTraceHeaders()`.

Fields for checks:

- `script.exit_code`: The exit code of the script execution (0 for success).
- Custom fields set via `output[key] = value` in the script code.

## Checks
Validation is performed with `checks`: expressions written in the
[`filt-rs`](https://docs.rs/filt-rs/latest/filt_rs/) filter language and evaluated against
the probe's result. Each expression references the sample's fields and supports comparisons
(`==`, `!=`, `<`, `>`, …), membership (`in`, `contains`), pattern matching (`like`,
`matches`), and boolean composition (`&&`, `||`, `!`). A probe fails as soon as any of its
checks does not match.

```yaml
    checks:
      - http.status == 200
      - http.status in [200, 204]
      - http.status >= 200 && http.status < 300
      - http.header.content-type contains "text/html"
      - http.header.content-type matches r"^text/html"
```

Each check is parsed when the configuration loads, so a malformed expression is reported
immediately rather than at run time, and each is reported as its own pass/fail result in
the dashboard and in telemetry. See the [Checks guide](https://grey.sierrasoftworks.com/checks/)
for the full expression language.

## Cron monitoring
Some work can't be probed from the outside — backups, batch jobs, and scheduled tasks only
tell you they're healthy by *running*. Grey's cron monitors are a passive "deadman's
switch" for exactly this case: instead of Grey calling your job, your job checks in with
Grey on its expected cadence, and Grey raises the alarm when an expected run goes missing
or overruns.

Declare crons under a top-level `crons` key:

```yaml
crons:
  - name: backup.nightly
    schedule: '0 2 * * *'  # crontab (UTC) — or use `interval` instead
    max_duration: 30m      # a run still in flight after this reads as overrunning (optional)
    grace: 1h              # slack after the due time before a no-show reads as missing (optional)
    token: 's3cr3t'        # shared secret required on check-ins (optional)
    tags:
      service: Backups

  - name: sync.hourly
    interval: 1h           # the simple fixed-cadence form — or use `schedule` instead
```

Your job then reports to `/api/v1/cron/{name}/check-in` (a `POST` with a JSON body, or a
`GET` with query parameters) with a `status` of `running`, `succeeded`, or `failed`:

```bash
# Report that a run has started, then that it succeeded.
curl -X POST https://status.example.com/api/v1/cron/backup.nightly/check-in \
  -d '{"status":"running"}'
curl -X POST https://status.example.com/api/v1/cron/backup.nightly/check-in \
  -d '{"status":"succeeded","message":"42 GB written"}'
```

Crons appear on the status page alongside the probes that share their `service` tag. See
the [Cron monitoring guide](https://grey.sierrasoftworks.com/guide/crons.html) for the
detection rules and authentication options.

## Clustering
Grey's clustering lets multiple instances pool their measurements into a single, shared
view of your platform's health — so the same service can be probed from several network
locations at once, with redundancy if any one node goes down.

When clustering is enabled, nodes discover each other through an encrypted (AES-256-GCM)
gossip protocol and replicate their probe and cron state as
[CRDTs](https://en.wikipedia.org/wiki/Conflict-free_replicated_data_type), converging on a
common view without a central coordinator. Worker nodes can run headless while a central
node (configured with some, all, or none of the same probes) serves the status page.

```yaml
state: ./state.redb

ui:
  enabled: true
  listen: 0.0.0.0:3000
  title: "Grey Cluster - Primary"

cluster:
  enabled: true
  listen: 0.0.0.0:8888
  peers:
    - 10.0.0.2:8888
    - 10.0.0.3:8888
  secrets:
    - /pL7XKDj1UrAGjNMv3t9jmb9leDOZT+64KkYE8k7UH8=  # 32 bytes, base64 (e.g. `openssl rand -base64 32`)
```

All nodes share the same `secrets` (which support zero-downtime key rotation), and we
recommend configuring at least two `peers` on every node for reliable discovery and
recovery after a network partition. See the
[Clustering guide](https://grey.sierrasoftworks.com/guide/clustering.html) for tuning,
peer discovery, and failure detection.

> ℹ️ The gossip protocol version is tied to Grey's **major** version — every node in a
> cluster must run the same major version.

## Status page
Grey can serve a status page for your customers, useful for sharing the health of your
infrastructure. It is disabled by default and is brandable with your own logo, title, and
links.

```yaml
ui:
  enabled: true
  listen: 127.0.0.1:3002

  title: My Status Page
  logo: https://example.com/logo.png

  links:
    - title: GitHub
      url: https://github.com/SierraSoftworks/grey
```

### Incidents
The status page includes customer-facing **incident management**: timestamped, updatable
events that you publish to keep customers informed during an outage or maintenance window.
Incidents are created and updated by signed-in administrators through the UI, with access
controlled via OIDC and an access-control list:

```yaml
ui:
  enabled: true
  admin:
    # A filt-rs expression over the signed-in user's claims; defaults to deny-all.
    acl: 'claims.email == "you@example.com"'
    oidc:
      endpoint: https://auth.example.com
      client_id: grey-status-page
      client_secret: '00000000000000000000000000000000'
      scopes: [profile, email]
```

The agent holds the `client_secret` and exchanges the authorization code server-side, so
the secret never reaches the browser. See the
[Incidents guide](https://grey.sierrasoftworks.com/ui/incidents.html) for details.

## OpenTelemetry
Grey uses OpenTelemetry to export trace information about each probe it executes. Traces
can be emitted to any gRPC-compatible OpenTelemetry endpoint by configuring the
`OTEL_EXPORTER_OTLP_ENDPOINT` environment variable. For example, to emit traces to a local
Jaeger instance:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317" \
    grey --config config.yaml
```

If you need to provide headers for the OTLP exporter, set the `OTEL_EXPORTER_OTLP_HEADERS`
environment variable. For example, to provide a Honeycomb API key:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT="https://api.honeycomb.io" \
OTEL_EXPORTER_OTLP_HEADERS="x-honeycomb-team=YOUR_API_KEY" \
    grey --config config.yaml
```

## Learn more
- **Documentation:** <https://grey.sierrasoftworks.com>
- **Downloads:** <https://github.com/SierraSoftworks/grey/releases>
- **Report an issue:** <https://github.com/SierraSoftworks/grey/issues/new>
- **Example configurations:** see the [`example/`](./example) directory.
