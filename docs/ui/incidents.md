# Incidents

Incidents let an administrator record and communicate events affecting your
services — outages, degradations, and their resolution — directly from the
status page. Unlike [notices](./notices.md), which are static entries in your
configuration file, incidents are created and managed live through the UI and
stored in Grey's state database.

An incident is just a title and a series of **updates**, each with an **impact**,
a timestamp, and a markdown message. Its start, current status and resolution are
all inferred from those updates — there are no separate description, time or
affected-service fields to keep in sync.

Each update's impact is one of `offline`, `degraded`, `none` (no impact) or
`hidden`. An incident's current impact is that of its most recent update, and an
incident with no updates is treated as `hidden` — so a freshly created incident
is a hidden draft until you publish it by posting a visible update. An incident
whose current impact is `offline` or `degraded` is *active*; the overall status
shown on the page is the worst impact among the active incidents. Posting a
`none` update resolves an incident, returning its current impact to operational.

Incidents appear beneath the probes on the status page — the page's top-line
status turns amber or red while any incident is active — and in full on the
dedicated **Incidents** page. Each incident is shown as a vertical timeline of
its updates, every update a card coloured by its impact, with the connecting line
keeping each update's colour until the next one.

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

Once signed in, the **Incidents** page shows every incident (including hidden
drafts), and each incident's own page becomes editable:

- **Declare Incident** — the button on the Incidents page opens a dedicated page
  for a title and the incident's opening update (an impact and a markdown
  message). Saving creates the incident with that first update and takes you to
  its page.
- **Edit the title or a message** — on an incident's page, click the title to
  change it, and click an update's edit (pencil) icon to switch its message from
  the rendered markdown to a textarea. A **Save** icon appears at the top-right as
  soon as there are unsaved changes. Saving is an atomic check-and-set, so an
  edit made against a stale version is rejected rather than overwriting a
  concurrent change.
- **An update's impact is fixed once posted** — you choose an update's impact
  when you add it; after it has been saved, only its message can be changed (the
  impact records what the status was at that point on the timeline). The latest
  update sets the incident's current impact, so adding a visible update publishes
  a draft and adding a `none` update resolves the incident.
- **Add / remove updates** — add a new update (pick its impact and write its
  message), or remove an unsaved one, then save.
- **Delete** — remove the incident permanently.

Signing in is via the **Sign in** button in the header; once signed in, hover the
user chip to reveal **Sign out**.

::: tip
Each update is timestamped automatically when you add it, and all times are
displayed in UTC.
:::

## Pages and links

Each incident has a short id shown as dash-grouped base36 (e.g. `1up-3mt-g`) and
its own page at `/incidents/{id}`. The landing page shows recent and active
incidents as compact summaries — a title and a horizontal timeline whose markers
reveal each update on hover; the **Incidents** page lists them in full, and every
incident title links through to its page.
