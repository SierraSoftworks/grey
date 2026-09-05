# Introduction

Grey includes a built-in web interface that provides a status page for monitoring your probes and sharing health
information with your customers. The UI is designed to be simple, lightweight, and customizable to match your brand.

## Configuration

The UI is disabled by default and can be enabled through the configuration file.
All UI-related settings are nested under the `ui` section.

::: code-tabs

@tab Basic Configuration

```yaml
ui:
    enabled: true
    listen: 127.0.0.1:3002
```

@tab Full Configuration

```yaml
state: ./state/

ui:
    enabled: true
    listen: 127.0.0.1:3002
    title: My Status Page
    logo: https://example.com/logo.png
    links:
        - title: GitHub
            url: https://github.com/SierraSoftworks/grey
    inject:
      - '<script src="https://example.com/custom.js"></script>'
      - '<link rel="stylesheet" href="https://example.com/custom.css">'
```

:::

## State Persistence

Grey supports persisting probe execution state across application restarts by configuring a state directory.
This ensures that probe history, availability metrics, and state transitions are maintained when the application is restarted.

```yaml
state: ./state/
```

When a state directory is configured:

 - **Probe History Preservation**
   All probe execution history, including state transitions, availability metrics,
   and timing data, is automatically saved to disk
 - **Automatic Snapshots**
   Probe state is written to disk asynchronously after each probe execution (throttled to once every 60 seconds per probe)
 - **Seamless Recovery**
   On startup, Grey automatically loads the previous state from disk, allowing for uninterrupted monitoring

This is primarily used in conjunction with the `ui` configuration options to allow you to restart Grey without
losing your historical status page data.

### Usage

Simply specify a directory path where Grey should store state files:

```yaml
# Relative path (recommended for development)
state: ./state/

# Absolute path (recommended for production)
state: /var/lib/grey/state/

# Windows path
state: C:\ProgramData\Grey\state\
```

The directory will be created automatically if it doesn't exist.
Ensure the Grey process has read/write permissions to the specified directory.

## Configuration Options

### state <Badge text="optional"/>

Directory path where Grey should store probe execution state for persistence across restarts.
When configured, probe history, availability metrics, and state transitions are automatically saved to disk
and restored on startup.

```yaml
state: ./state/  # Relative path
state: /var/lib/grey/state/  # Absolute path (Linux)
state: C:\ProgramData\Grey\state\  # Windows path
```

If not specified, probe state will only be kept in memory and will be lost when the application restarts.

### state_flush_interval <Badge text="default: 30s"/>

How often probe results and gossip updates are flushed durably to disk. These frequent writes are committed
without waiting for the disk (so a slow volume never stalls probing) and are persisted by a periodic flush,
on shutdown, and by any other durable write. Up to one interval of probe history may be lost if the process
is killed or the host loses power; peers in a cluster retain a copy of the lost samples through gossip.
Rare writes such as incidents and cron check-ins are always persisted immediately.

```yaml
state: ./state/
state_flush_interval: 30s
```

### enabled <Badge text="required" type="danger"/>

Whether to enable the web interface. When disabled, no web server will be started.

### listen <Badge text="default: 127.0.0.1:3002"/>

The address and port on which the web interface should listen. Use `0.0.0.0:3002` to listen on all interfaces.

### title <Badge text="default: Grey Status Page"/>

The title displayed at the top of the status page and in the browser tab.

### logo

URL to a logo image to display on the status page. Should be accessible via HTTP(S).
If not provided, the Grey default logo will be used.

### links

A list of links to display in the status page navigation. See [Links](./links.md) for more details.

```yaml
links:
  - title: GitHub
    url: https://github.com/SierraSoftworks/grey
  - title: Documentation
    url: https://grey.sierrasoftworks.com
  - title: Support
    url: https://support.example.com
```

### inject

A list of raw HTML snippets to append to the page's `<head>` block. Each string is
inserted verbatim, immediately before the closing `</head>` tag, on every server-rendered
page. This lets you add custom styling, scripts, analytics, favicons, or `<meta>` tags
without rebuilding Grey.

```yaml
ui:
  enabled: true
  inject: |
    <link rel="stylesheet" href="https://example.com/custom.css">
    <script src="https://example.com/custom.js"></script>
    <link rel="icon" href="https://example.com/favicon.ico">
```

::: warning
Snippets are injected exactly as written, with no escaping or sanitisation. Only inject
markup you trust and control — a malicious or broken snippet runs in the browser of
everyone who views your status page.
:::

### admin

Optional administrative access configuration. When present, it enables the
[incident management](./incidents.md) tooling and other admin APIs, gating them behind
OIDC authentication and an access-control list. When omitted, the admin API is closed
entirely and the status page remains fully public and read-only.

See [Authentication](#authentication) below for the full configuration.

## Authentication

The status page itself is public and read-only by default. Administrative features —
such as [declaring and managing incidents](./incidents.md) — are gated behind OIDC
authentication, configured under `ui.admin`.

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

Once configured, sign in via the **Sign in** button in the header. The browser runs the
OIDC Authorization Code flow and hands the resulting authorization code to the agent,
which exchanges it for a token using its configured `client_secret` and returns the token
to the browser. The token is sent as an `Authorization: Bearer` header on admin requests —
no cookies are used, and the client secret never reaches the browser.

The same signed-in identity can also drive per-probe and per-cron
[visibility](../guide/configuration.md#visibility), letting you hide selected entries from
anonymous viewers.

For the OIDC provider requirements, access-control (`acl`) expression syntax, and the full
administration workflow, see [Incidents](./incidents.md).

## Security Considerations

 - By default, the UI listens only on `127.0.0.1` (localhost), making it accessible only from the same machine.
 - To make the status page publicly accessible, set the listen address to `0.0.0.0:3002`.
 - Consider placing the UI behind a reverse proxy with proper SSL/TLS termination for production deployments.
 - The public status page is read-only; administrative actions require signing in, enabled by configuring [authentication](#authentication) under `ui.admin`.
 - Content added through `inject` is served verbatim to every viewer, so only inject markup you trust.
