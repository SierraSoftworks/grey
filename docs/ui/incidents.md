# Incidents

Incidents let an administrator record and communicate events affecting your
services — outages, degradations, and their resolution — directly from the
status page. Unlike [notices](./notices.md), which are static entries in your
configuration file, incidents are created and managed live through the UI and
stored in Grey's state database.

An incident has a title, a description (markdown), a start time, an optional end
time, an optional list of affected services, and a series of **updates** (each
with an **impact**, a timestamp, and a markdown message).

Each update's impact is one of `offline`, `degraded`, `none` (no impact) or
`hidden`. An incident's current impact is that of its most recent update, and an
incident with no updates is treated as `hidden` — so a freshly created incident
is a hidden draft until you publish it by posting an update. `offline` and
`degraded` incidents that are still ongoing (no end time) are *active*; the
overall status shown on the page is the worst impact among the active incidents.

Incidents appear as status-coloured blocks beneath the probes on the status
page (under a header that turns amber/red when incidents are active), and in
full on the dedicated **Incidents** page. Each block carries a timeline running
from the incident's start to its end, with its updates shown as cards along the
way; the connecting line keeps each update's colour until the next one.

::: tip
Incidents are stored locally in Grey's state database (`state.redb`) as JSON.
They are **not** replicated across a cluster — manage them against a single
node (or a stable endpoint that routes to one).
:::

## Enabling administration

Incident management is gated behind OIDC authentication. Add an `admin` block
under `ui` with your provider's details and an access-control list (`acl`):

```yaml
ui:
  enabled: true
  admin:
    # A filt-rs expression evaluated against the signed-in user's token claims.
    # It must evaluate to true for a request to be allowed. Defaults to denying
    # everyone, so the admin area is closed until you set this.
    acl: 'claims.email == "you@example.com"'
    oidc:
      # Your OIDC provider's issuer / base URL.
      endpoint: https://auth.example.com
      # The OAuth2 client id registered for the status page.
      client_id: grey-status-page
      # The OAuth2 client secret. Held by the agent only; never sent to the browser.
      client_secret: '00000000000000000000000000000000'
      # Optional extra scopes (openid is always requested).
      scopes: [profile, email]
```

Sign in via the **Sign in** button in the header. The browser runs the OIDC
Authorization Code flow and hands the resulting authorization code to the agent,
which exchanges it for a token using its configured `client_secret` and returns
the token to the browser. The token is then sent as an `Authorization: Bearer`
header on admin requests — no cookies are used, so there is nothing to protect
against CSRF. The client secret stays on the agent and never reaches the browser.

## OIDC provider requirements

Register a **confidential web client** with your provider:

- A **client secret**, configured on the agent as `client_secret` (it is never
  shipped to the browser; the agent exchanges the authorization code itself).
- The **redirect URI** registered as your status page's origin with a trailing
  slash, e.g. `https://status.example.com/`.
- The **Authorization Code** grant enabled. PKCE is *not* required, and the
  provider does **not** need to permit cross-origin (CORS) requests — every call
  the browser makes is to the agent, which talks to the provider server-side.

ID tokens must be signed with an asymmetric algorithm (e.g. `RS256` or `ES256`);
symmetric (`HS*`) tokens are rejected. The token's audience must be the
configured `client_id` and its issuer the configured `endpoint`.

## Access control

The `acl` is a [filt-rs](https://docs.rs/filt-rs) expression — the same language
used by probe [checks](/checks/). The validated token claims are exposed under
the `claims.` prefix, and the request `method` and `path` are also available.

```yaml
# A single permitted user
acl: 'claims.email == "you@example.com"'

# Anyone in an "admins" group claim
acl: 'claims.groups contains "admins"'

# A whole verified domain
acl: 'claims.email_verified == true && claims.email matches r"@example\.com$"'
```

::: warning
The default ACL denies every request. Administration stays closed until you
provide an expression that matches your account.
:::

A request with a valid token whose claims fail the ACL receives `403`; a request
with no (or an invalid) token receives `401`.

## Managing incidents

Once signed in, the **Incidents** page shows every incident (including drafts)
with management controls:

- **New incident** — immediately saves a new draft (with its start time set to
  now) and opens its editor so you can fill in the details.
- **Edit** — change the incident's title, description, start/end times and
  affected services. Affected services offer an autocomplete drawn from your
  configured services and probe names. (Impact is set through updates, not here.)
- **Add update** — post an update with an **impact** (`offline`, `degraded`,
  `none`, or `hidden`) and a markdown message. The latest update sets the
  incident's current impact, so posting a non-`hidden` update publishes a draft,
  and posting a `none` update (plus setting an end time) resolves it.
- **Delete** — remove the incident permanently.

Signing in is via the user chip in the header; hover it to reveal **Sign out**.

::: tip
Times are entered and displayed in UTC.
:::

## Schema migrations

Incidents are stored as JSON behind a global schema version recorded in the
state database. On startup Grey applies any pending migrations as part of
initializing the database; it only refuses to start if a migration cannot be
applied, so upgrades are safe and automatic.
