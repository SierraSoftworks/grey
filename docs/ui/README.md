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

## State Persistence

Grey supports persisting probe execution state across application restarts by configuring a state directory. This ensures that probe history, availability metrics, and state transitions are maintained when the application is restarted.

```yaml
state: ./state/
```

When a state directory is configured:

- **Probe History Preservation**: All probe execution history, including state transitions, availability metrics, and timing data, is automatically saved to disk
- **Automatic Snapshots**: Probe state is written to disk asynchronously after each probe execution (throttled to once every 60 seconds per probe)
- **Seamless Recovery**: On startup, Grey automatically loads the previous state from disk, allowing for uninterrupted monitoring

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

The directory will be created automatically if it doesn't exist. Ensure the Grey process has read/write permissions to the specified directory.

## Configuration Options

### state <Badge text="optional"/>

Directory path where Grey should store probe execution state for persistence across restarts. When configured, probe history, availability metrics, and state transitions are automatically saved to disk and restored on startup.

```yaml
state: ./state/  # Relative path
state: /var/lib/grey/state/  # Absolute path (Linux)
state: C:\ProgramData\Grey\state\  # Windows path
```

If not specified, probe state will only be kept in memory and will be lost when the application restarts.

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
