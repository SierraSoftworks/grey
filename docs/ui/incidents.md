# Incidents

Incidents let an administrator record and communicate events affecting your
services — outages, degradations, and their resolution — directly from the
status page. Unlike [notices](./notices.md), which are static entries in your
configuration file, incidents are created and managed live through the UI and
stored in Grey's state database.

An incident has a title, a description (markdown), a start time, optional
detection / mitigation / resolution times, an optional list of affected
services, and a series of **status updates** (each with a status of `healthy`,
`degraded`, `offline` or `unknown`, a timestamp, and a markdown message).

Incidents appear as a timeline beneath the probes on the status page, and in
full on the dedicated **Incidents** page. Incidents can be hidden from
unauthenticated visitors while you prepare them.

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
      # The public OAuth2 client id registered for the status page.
      client_id: grey-status-page
      # Optional extra scopes (openid is always requested).
      scopes: [profile, email]
```

Sign in via the **Sign in** button in the header. Grey runs the OIDC
Authorization Code flow with PKCE entirely in the browser, then sends the
resulting ID token as an `Authorization: Bearer` header on admin requests — no
cookies are used, so there is nothing to protect against CSRF. The agent only
*validates* tokens; it never sees a client secret.

## OIDC provider requirements

Because the browser drives the login directly, the client you register with your
provider must be configured as a **public / SPA client**:

- **PKCE enabled**, with **no client secret**.
- The **redirect URI** registered as your status page's origin with a trailing
  slash, e.g. `https://status.example.com/`.
- **CORS** permitted on the provider's token endpoint for your status page's
  origin (the browser calls it directly to exchange the authorization code).

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
ones) with management controls:

- **New incident** — create an incident with its title, description, times,
  affected services, and visibility.
- **Edit** — change any of an incident's details.
- **Add update** — post a status update (status + markdown message); the most
  recent update drives the incident's current status on the timeline.
- **Hide / Show** — toggle whether unauthenticated visitors can see the incident.
- **Delete** — remove the incident permanently.

::: tip
Times are entered and displayed in UTC.
:::

## Schema migrations

Incidents are stored as JSON behind a global schema version recorded in the
state database. On startup Grey applies any pending migrations as part of
initializing the database; it only refuses to start if a migration cannot be
applied, so upgrades are safe and automatic.
