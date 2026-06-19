# Cron Monitoring

Most of Grey actively *probes* your services — it reaches out on a schedule and checks that they
respond. Some work can't be probed from the outside, though: backups, batch jobs, and scheduled
tasks only tell you they're healthy by *running*. Grey's cron monitors are a passive "deadman's
switch" for exactly this case. Instead of Grey calling your job, your job checks in with Grey on its
expected cadence, and Grey raises the alarm when an expected run goes missing.

## Overview

A cron monitor is declared in your configuration and then driven entirely by HTTP check-ins from the
job itself. Each check-in reports a status — `running`, `succeeded`, or `failed` — and Grey applies
two independent detectors to the stream of check-ins:

- A **schedule** detector that flags a **missed run** when no run starts by the next scheduled time
  (plus a grace period).
- A **completion** detector that flags a **hung run** when a started run doesn't report completion
  within its `max_duration`.

Crons surface on the status page alongside the probes that share their `service` tag, rendered much
like an active probe: a current status, how long it has held that status, a strip of recent runs, and
the last reported message. In a [cluster](./clustering.md) a check-in received by any node is
replicated to the rest, so the whole cluster converges on the same view regardless of which node was
called.

## Configuration

Crons live under a top-level `crons` key, alongside `probes`:

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

  - name: internal.report
    schedule: '0 6 * * *'
    visible: auth.admin    # only signed-in administrators can see this cron
```

| Option | Default | Description |
| --- | --- | --- |
| `name` | _(required)_ | Unique name; also the identifier in the check-in URL. |
| `interval` | — | A fixed cadence such as `1h` or `24h`. **Exactly one of `interval` / `schedule` is required.** |
| `schedule` | — | A standard 5-field crontab expression such as `0 2 * * *`, evaluated in UTC. **Exactly one of `interval` / `schedule` is required.** |
| `max_duration` | _none_ | How long a run may stay in flight before it reads as overrunning. When unset, hung-run detection is disabled. |
| `grace` | `interval / 10`, or `5m` for a crontab | Slack after the due time before a late run reads as missing. |
| `token` | _none_ | When set, callers must present this secret on every check-in. |
| `tags` | _none_ | Free-form labels; the `service` tag groups the cron with the probes of the same service. |
| `visible` | `true` | A [`filt-rs`](../checks/README.md) expression over the viewer's auth context (`auth`, `auth.admin`, `claims.<name>`) deciding who can see this cron. See [Visibility](configuration.md#visibility). |

## Checking In

A job reports a check-in to `/api/v1/cron/{name}/check-in` — a dedicated ingest endpoint, separate
from the UI's `/api/v1/crons` read API. It accepts a `POST` with a JSON body or a `GET` with query
parameters, so it works from almost any environment.

```bash
# Report that a run has started, then that it succeeded (POST with a JSON body).
curl -X POST https://status.example.com/api/v1/cron/backup.nightly/check-in \
  -d '{"status":"running"}'
curl -X POST https://status.example.com/api/v1/cron/backup.nightly/check-in \
  -d '{"status":"succeeded","message":"42 GB written"}'

# The same as a GET with query parameters, handy for restricted cron environments.
curl 'https://status.example.com/api/v1/cron/backup.nightly/check-in?status=succeeded'
```

The `status` is required and must be one of `running`, `succeeded`, or `failed`; an optional
`message` is shown alongside the cron on the status page. A successful check-in returns `202
Accepted`.

A job that only reports on completion (just `succeeded`/`failed`, with no `running`) is fully
supported: the schedule detector still applies, and the completion detector simply stays dormant.

### Statuses

- **`running`** — a run has started. This is a heartbeat: it satisfies the schedule detector and
  starts the completion clock, but it does not change the displayed pass/fail result, which stays at
  the last terminal outcome.
- **`succeeded`** — the run finished successfully.
- **`failed`** — the run finished unsuccessfully; the cron reads as failing until a later run
  succeeds.

### Authentication

Crons are config-declared, so a check-in for an unknown name is rejected with `404`. The endpoint is
otherwise public so that jobs can check in without an interactive login. To protect a cron from
spurious check-ins, set a `token`; callers must then present it via the `X-Cron-Token` header or a
`token` query parameter, and a missing or wrong token is rejected with `401`.

```bash
curl -X POST https://status.example.com/api/v1/cron/sync.hourly/check-in \
  -H 'X-Cron-Token: s3cr3t' -d '{"status":"succeeded"}'

curl 'https://status.example.com/api/v1/cron/sync.hourly/check-in?status=succeeded&token=s3cr3t'
```

::: tip
The token is a guard against accidental or casual check-ins, not a substitute for network controls.
Treat it like any other secret: deliver it to your jobs through your secret manager rather than
committing it to source control.
:::

## Detection

Detection is **deterministic** — measured against the schedule you declared rather than a learned
cadence, so a job that quietly slows down is flagged rather than silently accepted. A cron reads as
failing once either:

- **Missed run** — no run has started by the next scheduled time plus `grace`. For an `interval` the
  next due time is `last run + interval`; for a `schedule` it is the next matching crontab time (in
  UTC).
- **Overrunning** — an in-flight run (one that reported `running` but no terminal status yet) has been
  going for longer than `max_duration`. With no `max_duration` set, this check is disabled.

Health is computed on read from the replicated run history, so every node in a cluster reaches the
same verdict without extra coordination, and a cron recovers on its own as soon as a fresh, on-time
run checks in.
