# Configuration
Grey uses a YAML configuration file to define probes and control how they are executed.
This guide is intended to walk you through the various configuration options available
and how to use them to configure your probes.

## Top-Level Configuration

### State Database
The `state` option configures a database file where Grey will store probe execution state
for persistence across application restarts. When configured, probe history, availability
metrics, and state transitions are automatically saved to disk using a high-performance
embedded database.

```yaml
state: ./state.redb
```

If not specified, probe state will only be kept in memory and will be lost when the
application restarts. The database file uses the `.redb` extension and will be created
automatically if it doesn't exist.

## Probes
Probes are the core of Grey's configuration. Each probe defines a single target and a set
of checks that will be used to assert that the target is healthy. In addition to these
properties, a probe has a `name`, and a policy governing how frequently it is executed and
how timeouts and retries should be handled.

```yaml
probes:
    - name: example
      policy:
        interval: 5s
        timeout: 2s
        retries: 3
      target: !Http
        url: https://example.com
      checks:
        - http.status == 200
        - http.header.content-type contains "text/html"
```

### Name
The `name` property is a unique identifier for the probe. It is used to identify the probe
in the traces that are emitted by Grey and should be a short, descriptive name. By convention
we recommend using the format `<service>.<environment>[.<subcomponent>]`, for example:
`vault.production` or `nomad.staging.leader`. In practice, however, Grey doesn't enforce
any constraints on this value and you're welcome to use it as you see fit.

### Policy
The `policy` property defines how Grey will execute your probe, including how frequently,
how long to wait for a response, and how many times to retry if the probe fails. In the
future, additional policy options may be introduced to control exponential back-off,
circuit breaking, and other behaviours.

When configuring your policy, keep in mind that both `interval` and `timeout` are specified
in milliseconds. The `retries` property is an integer value that specifies the number of times
that the probe will be executed (also known as "attempts") before considering it failed if
an issue is encountered.

::: warning
The `timeout` property applies to the entire probe's execution, including the time taken
to perform any retries, and should be configured to allow time for retries to occur if you
expect them to be needed.

*The decision to apply the timeout to the entire probe execution is intentional and designed
to avoid retry storms in the event that the target service is degrading in the face of increased
load. By not retrying on timeouts, Grey avoids introducing non-linear degradation scenarios.*
:::

### Target
The `target` property defines the target that will be probed. This is where you specify
the type of target (e.g. `!Http`) and any configuration options that are specific to that
target type. For example, the `!Http` target type accepts a `url` property that specifies
the URL that will be probed.

You can read more about the various target types in the [Targets](../targets/README.md) section
of the documentation.

### Checks
The `checks` property defines a list of expressions, written in the
[`filt-rs`](https://docs.rs/filt-rs/latest/filt_rs/) filter language, that are evaluated
against the probe's result to assert that the target is healthy. Each expression references
the sample's fields (such as `http.status`) and can combine conditions, compare ranges,
match patterns, and relate one field to another. A probe fails as soon as any of its checks
does not match.

You can read more about the expression language and the available operators in the
[Checks](../checks/README.md) section of the documentation.

::: tip
You can read more about the fields available for each target in the [Targets](../targets/README.md)
section of the documentation.
:::

### Visibility
The optional `visible` property controls who can see a probe on the status dashboard and in the API.
It is a [`filt-rs`](../checks/README.md) expression — the same language used by `checks` — but instead
of being evaluated against a probe sample, it is evaluated against the **auth context** of whoever is
making the request:

| Field | Type | Meaning |
| --- | --- | --- |
| `auth` | boolean | `true` when the viewer presented a valid sign-in (a shortcut for "any authenticated user"). |
| `auth.admin` | boolean | `true` when the viewer passed the [admin ACL](../ui/incidents.md#access-control). |
| `claims.<name>` | claim value | A validated token claim, mirroring the admin ACL (e.g. `claims.groups`). |

`visible` defaults to `true`, so a probe with no `visible` expression is shown to everyone, exactly as
before. Set it to restrict a probe to a subset of viewers — entries a viewer may not see are never sent
to their browser at all (they are filtered out of both the server-rendered page and the API response):

```yaml
probes:
  - name: internal.dashboard
    policy:
      interval: 30s
      timeout: 5s
    target: !Http
      url: https://internal.example.com/health
    # Only signed-in administrators can see this probe.
    visible: auth.admin

  - name: partner.api
    policy:
      interval: 30s
      timeout: 5s
    target: !Http
      url: https://partner.example.com/health
    # Anyone signed in (not necessarily an admin) can see this probe.
    visible: auth
```

Because `auth.admin` reuses the admin ACL, visibility requires an [`admin`](../ui/incidents.md) block to
be configured; with no admin configured, nobody passes `auth.admin` and such an entry is hidden from
everyone. The same `visible` property is available on [crons](crons.md).

::: warning
Visibility is a presentation control, not a security boundary for the probed services themselves — it
governs what the status page and API expose, so use it to tidy a public status page rather than as the
only protection for sensitive endpoints.
:::

## Status Dashboard
Grey includes an optional web-based user interface that provides real-time visibility
into probe status and execution history. The UI can be enabled on any node and integrates
seamlessly with clustering to provide a unified view of your service health. It's a great
way to provide a status page for your users.

```yaml
ui:
  enabled: true
  listen: 0.0.0.0:3000
  title: "Grey Health Monitor"
  logo: "https://example.com/logo.svg"
```

You can read more about UI configuration options in the [User Interface](../ui/README.md)
section of the documentation.

## Clustering
Grey supports distributed probing through its clustering feature, which enables multiple
Grey instances to coordinate probe execution and share results. This is particularly useful
for scaling probe execution across multiple nodes, providing redundancy, and enabling probes
to be executed from different network locations while maintaining a centralized view through
the web UI.

```yaml
cluster:
  enabled: true
  listen: 0.0.0.0:8888
  peers:
    - 10.0.0.2:8888
    - 10.0.0.3:8888
  secret: /pL7XKDj1UrAGjNMv3t9jmb9leDOZT+64KkYE8k7UH8=
```

You can read more about clustering in the [Clustering](../clustering/README.md) section of the documentation.

## Webhooks
Grey can notify external systems whenever a probe or cron changes state, which is handy for wiring
Grey into incident-management platforms, chat tools, or your own automation. Each webhook declares a
destination `endpoint`, an optional shared `secret` used to sign deliveries, an optional set of extra
`headers`, and a `filter` (in the same [`filt-rs`](../checks/README.md) language as `checks`) that
selects which events it receives.

```yaml
webhooks:
  - name: pagerduty
    endpoint: https://events.pagerduty.com/integration/abc123/enqueue
    secret: 'a-long-random-shared-secret'
    filter: 'state.healthy == false'
    headers:
      Authorization: 'Token token=xxxxxxxxxxxxxxxxxxxx'
```

You can read more about the event payload, signature verification, and the available filter fields in
the [Webhooks](./webhooks.md) guide.