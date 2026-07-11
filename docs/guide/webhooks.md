# Webhooks
Grey can deliver a webhook notification whenever a probe or a cron changes state, letting you
integrate with incident-management platforms (PagerDuty, Opsgenie, ...), chat tools, or your own
automation. Each notification is a JSON document describing the transition and the full state of the
entity, optionally signed with a shared secret so the receiver can authenticate it.

```yaml
webhooks:
  - name: pagerduty
    endpoint: https://events.pagerduty.com/integration/abc123/enqueue
    secret: 'a-long-random-shared-secret'
    filter: 'state.healthy == false'
    headers:
      Authorization: 'Token token=xxxxxxxxxxxxxxxxxxxx'
```

A full, runnable example lives in [`example/webhooks.yml`](https://github.com/SierraSoftworks/grey/blob/main/example/webhooks.yml).

## What triggers a notification
Grey continuously re-derives the displayed state of every probe and cron — exactly the state the
status page renders — and sends an event whenever an entity crosses between **healthy** and
**unhealthy**. Movement *within* a health class is not notified, so a healthy job running on a tight
schedule does not produce a stream of events.

- **Probes** are healthy while `passing` and unhealthy while `failing`. The `failing` state includes
  a probe that has stopped responding: recovery is implicit, so a probe reads as failing until no
  failure has been observed for the recovery window, then transitions back to `passing`.
- **Crons** are healthy while `pending`, `running`, or `succeeded`, and unhealthy while `failed`,
  `missing` (a run was not started in time), or `stuck` (a run is overrunning its `max_duration`).
  An event fires only when a cron crosses between those two groups — a normal run cycling
  `succeeded` → `running` → `succeeded` stays healthy throughout and is silent. The specific state
  it moved to is carried in `state.current` (and the health axis in `state.healthy`).

The recovery window (probes) and the schedule grace / `max_duration` (crons) act as a settling time
on these transitions, so a transient blip that clears within those windows never produces an event.

Because state is re-derived on a short cadence, both *event-driven* changes (a fresh probe sample or
cron check-in) and *time-driven* changes (a probe recovering, or a cron run going missing) are
reported by the same mechanism.

When Grey starts it records the current state of every entity **silently**, so a restart never
replays the state your services are already in — only genuine transitions observed afterwards are
delivered.

## Tuning sensitivity: `alerting`
By default a probe or cron is considered to have changed health only once the new state has held
**continuously for five minutes** — both when it becomes unhealthy and when it recovers. This
debounce suppresses brief flaps so a single failed sample, or a job that is a few seconds late, does
not page anyone. You can tune it per entity with an `alerting` block:

```yaml
probes:
  - name: example.web
    policy: { interval: 5s, timeout: 2s }
    target: !Http
      url: https://example.com
    alerting:
      enabled: true      # deliver webhooks for this probe's transitions (the default)
      debounce: 10m      # require the new state to hold for 10 minutes before reporting it

crons:
  - name: backup.nightly
    schedule: '0 2 * * *'
    alerting:
      debounce: 0s       # report as soon as the state is derived (no debounce)
```

| Field | Default | Description |
| ----- | ------- | ----------- |
| `alerting.enabled` | `true` | Whether webhook notifications fire for this entity. When `false`, its health is still tracked and shown on the status page, but no webhook is delivered. |
| `alerting.debounce` | `5m` | How long a new health state must hold before it is reported. Applied symmetrically to both the onset of a fault and the recovery from it. |

The `debounce` also governs the entity's **displayed** health: a fault is shown (and a recovery
cleared) only once it has held for this long, so the status page and the webhooks always agree. For a
probe this replaces the previously fixed five-minute recovery window with your configured value; set
a shorter `debounce` for faster, noisier alerting or a longer one to ride out routine flapping.

### Missed and overrunning cron runs
A cron that never checks in (a missed run — the deadman-switch case) or one that overruns its
`max_duration` has no check-in to record, so Grey synthesises a placeholder run for it. These
placeholders appear on the status page in grey (distinct from a job-reported failure) and progress
the same health signal, so a missed run is debounced and alerted on exactly like a reported failure.


## The event payload
The payload mirrors the probe/cron API representation rather than any single node's view: the
transition is derived from the cluster-converged streak (probes) or cron health — which already fold
in every observer's reports and the recovery settling window — and the embedded snapshot carries the
observations from *every* observer. There is no per-node field on the event.

Every delivery is an HTTP `POST` with a JSON body like this:

```json
{
  "version": "v1",
  "id": "0d6f1a3e-8b3b-4f9e-9b3a-2f0b8a6d1c44",
  "event": "probe.state_changed",
  "timestamp": "2026-06-19T12:00:00Z",
  "entity": {
    "type": "probe",
    "name": "example.web",
    "tags": { "service": "Example", "team": "Platform" }
  },
  "state": {
    "current": "failing",
    "previous": "passing",
    "healthy": false,
    "was_healthy": true,
    "since": "2026-06-19T11:59:30Z",
    "availability": 98.7
  },
  "probe": { "...": "the full probe snapshot: streak, history, observations, tags" }
}
```

| Field | Description |
| ----- | ----------- |
| `version` | The payload schema version (`"v1"` today). Branch on it to handle future schema changes. |
| `id` | A unique identifier for the event, also sent in the `Grey-Webhook-Delivery` header. Use it to de-duplicate. |
| `event` | `probe.state_changed` or `cron.state_changed`. |
| `timestamp` | When the event was generated (and the value signed in the `t=` of the signature). |
| `entity.type` | `probe` or `cron`. |
| `entity.name` | The probe/cron name. |
| `entity.tags` | The entity's configured tags. |
| `state.current` / `state.previous` | The status tokens before and after the transition (`passing`/`failing` for a probe; a cron health token for a cron). |
| `state.healthy` / `state.was_healthy` | The same transition collapsed onto the pass/fail axis, so you can branch on health regardless of the specific failure mode. |
| `state.since` | When the current state was entered, when known. |
| `state.availability` | The probe's availability over its retained history, as a percentage. Omitted for crons. |
| `probe` | For a probe event: the full probe snapshot, including its `streak`, `history`, per-observer `observations`, and `tags`. |
| `cron` | For a cron event: the full cron snapshot, including its `runs` and `last_checkin`. |

## Signing and verification
When a `secret` is configured, every delivery carries these headers:

```
Content-Type: application/json
Grey-Webhook-Event: probe.state_changed
Grey-Webhook-Delivery: 0d6f1a3e-8b3b-4f9e-9b3a-2f0b8a6d1c44
Grey-Webhook-Signature: t=1750334400,v1=<hex HMAC-SHA256>
traceparent: 00-<trace-id>-<span-id>-01
```

The signed timestamp is carried in the `t=` field of the signature header, so there is no separate
timestamp header.

When Grey has an OpenTelemetry pipeline configured it also propagates its trace context on each
delivery as W3C `traceparent` (and `tracestate`) headers, so a receiver that records traces can
stitch its handling onto Grey's delivery span.

The signature scheme is the one [Tailscale documents for its
webhooks](https://tailscale.com/docs/features/webhooks#verifying-an-event-signature): the `v1` value
is the hex-encoded **HMAC-SHA256** of the string `"<timestamp>.<raw-json-body>"`, keyed by the shared
`secret`, where `<timestamp>` is the `t=` value in the header.

To verify a delivery:

1. Read the `t` and `v1` values from the `Grey-Webhook-Signature` header.
2. Concatenate the timestamp, a literal `.`, and the **raw, unparsed** request body.
3. Compute `HMAC-SHA256(secret, "<t>.<body>")` and hex-encode it.
4. Compare it to `v1` using a constant-time comparison. Optionally, reject deliveries whose `t` is
   too far from the current time to mitigate replay.

```python
import hashlib, hmac

def verify(secret: str, signature_header: str, body: bytes) -> bool:
    parts = dict(p.split("=", 1) for p in signature_header.split(","))
    signed = parts["t"].encode() + b"." + body
    expected = hmac.new(secret.encode(), signed, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, parts["v1"])
```

When no `secret` is set, deliveries are unsigned (no `Grey-Webhook-Signature` header). Configure a
secret unless the endpoint is only reachable over a trusted network.

## Filtering which events are delivered
The `filter` is an expression in the same [`filt-rs`](../checks/README.md) language used by probe
`checks`. An event is delivered to the endpoint only when its filter evaluates to true. A webhook
with no `filter` receives every event.

The following fields are available to a filter:

| Field | Type | Example |
| ----- | ---- | ------- |
| `event` | string | `event == "cron.state_changed"` |
| `entity.type` (alias `entity.kind`) | string | `entity.type == "probe"` |
| `entity.name` | string | `entity.name matches r"^prod\."` |
| `entity.tags.<key>` (alias `tags.<key>`) | string | `entity.tags.team == "Platform"` |
| `state.current` | string | `state.current == "missing"` |
| `state.previous` | string | `state.previous == "passing"` |
| `state.healthy` | bool | `state.healthy == false` |
| `state.was_healthy` | bool | `state.was_healthy == true && state.healthy == false` |
| `state.availability` | number | `state.availability < 99.0` |

Some useful patterns:

```yaml
# Only page when something becomes unhealthy (a probe fails, or a cron fails/goes missing/overruns).
filter: 'state.healthy == false'

# Only the moment health is lost (ignore recoveries), for one team.
filter: 'state.was_healthy == true && state.healthy == false && entity.tags.team == "Payments"'

# Only cron problems.
filter: 'entity.type == "cron" && state.healthy == false'
```

## Additional headers
The `headers` map attaches extra headers to every delivery — for example an `Authorization` token
the receiving platform expects. They are sent alongside Grey's own signature and metadata headers.

These headers are **not** covered by the signature, which authenticates only the timestamp and the
request body. Treat them as transport-level conveniences (such as routing or auth tokens the receiver
checks itself), not as authenticated data — a receiver should not assume they arrived unmodified.

## Behaviour in a cluster
A webhook event represents the cluster's converged view of an entity, not a single node's
observation: transitions are read from the gossiped streak / cron health (which every node converges
on identically), and the snapshot carries every observer's data. You therefore don't need to run
webhooks on every node — configuring them on a single node is sufficient and authoritative.

If you do configure the same webhook on several nodes, the endpoint will receive one delivery per
node on each transition; de-duplicate downstream using the `Grey-Webhook-Delivery` header or the
entity name and `state.current`.

## Reliability
Each delivery is attempted once, bounded by the per-webhook `timeout` (default 10s). Failures and
non-success responses are logged (and traced) but not retried, so the endpoint should be tolerant of
the occasional missed delivery; the status page and API remain the source of truth for current
state.
