# Introduction

Grey includes a built-in web interface that provides a status page for monitoring your probes and sharing health information with your customers. The UI is designed to be simple, lightweight, and customizable to match your brand.

## Configuration

The UI is disabled by default and can be enabled through the configuration file. All UI-related settings are nested under the `ui` section.

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
    notices:
        - title: Example Notice
            description: This is an example notice message showcasing how you can alert users to something happening on your platform.
            timestamp: 2025-08-10T19:00:00Z
            level: ok # ok, warning, error
```

:::

## Configuration Options

### enabled <Badge text="required" type="danger"/>

Whether to enable the web interface. When disabled, no web server will be started.

### listen <Badge text="default: 127.0.0.1:3002"/>

The address and port on which the web interface should listen. Use `0.0.0.0:3002` to listen on all interfaces.

### title <Badge text="default: Grey Status Page"/>

The title displayed at the top of the status page and in the browser tab.

### logo

URL to a logo image to display on the status page. Should be accessible via HTTP(S). If not provided, the Grey default logo will be used.

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

### notices

A list of notices to display prominently on the status page. See [Notices](./notices.md) for more details.

```yaml
notices:
  - title: Scheduled Maintenance
    description: We will be performing scheduled maintenance on our systems from 2:00 AM to 4:00 AM UTC on August 15th.
    timestamp: 2025-08-15T02:00:00Z
    level: warning
  - title: Service Restored
    description: All services have been restored to normal operation.
    timestamp: 2025-08-15T04:30:00Z
    level: ok
```

## Security Considerations

- By default, the UI listens only on `127.0.0.1` (localhost), making it accessible only from the same machine.
- To make the status page publicly accessible, set the listen address to `0.0.0.0:3002`.
- Consider placing the UI behind a reverse proxy with proper SSL/TLS termination for production deployments.
- The UI does not include authentication mechanisms - it's designed to be a public status page.
