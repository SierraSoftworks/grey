# Notices

Notices are prominently displayed messages on your status page that allow you to communicate important information to your users, such as scheduled maintenance, ongoing incidents, or general announcements.

## Configuration

Notices are configured as an array under the `ui.notices` section of your configuration file:

```yaml
ui:
  enabled: true
  notices:
    - title: Scheduled Maintenance
      description: We will be performing scheduled maintenance on our systems from 2:00 AM to 4:00 AM UTC on August 15th. Some services may be temporarily unavailable.
      timestamp: 2025-08-15T02:00:00Z
      level: warning
    - title: Service Restored
      description: All services have been restored to normal operation following the maintenance window.
      timestamp: 2025-08-15T04:30:00Z
      level: ok
```

::: tip
Grey will attempt to reload any new notices from your configuration file automatically
once every 10 seconds. You do not need to restart Grey for new notices to appear.
:::

## Notice Properties

### title <Badge text="required" type="danger" />

A concise, descriptive title for the notice. This will be displayed prominently.

### description <Badge text="required" type="danger" />

The main content of the notice. Provide clear, detailed information about the situation.

### timestamp

When the notice was created or when the event occurred. Must be in ISO 8601 format (e.g., `2025-08-15T02:00:00Z`). If not provided, the notice will be displayed without a date in
the order it appears in the config file.

### level

The severity level of the notice, which affects its visual styling and priority. Valid values: `ok`, `warning`, `error`. If not provided, a generic notice will be displayed without any special styling.

## Examples

::: code-tabs

@tab Maintenance Notice

```yaml
ui:
  notices:
    - title: Scheduled Database Maintenance
      description: We will be upgrading our database systems on August 20th from 1:00 AM to 3:00 AM UTC. During this time, all services will be temporarily unavailable. We apologize for any inconvenience.
      timestamp: 2025-08-20T01:00:00Z
      level: warning
```

@tab Incident Update

```yaml
ui:
  notices:
    - title: Service Degradation - API Response Times
      description: We are currently experiencing increased response times on our API endpoints. Our engineering team is actively investigating the issue. We will provide updates as more information becomes available.
      timestamp: 2025-08-12T14:30:00Z
      level: error
```

@tab Positive Update

```yaml
ui:
  notices:
    - title: Performance Improvements Deployed
      description: We have successfully deployed performance improvements to our API infrastructure. Users should experience faster response times and improved reliability.
      timestamp: 2025-08-12T10:00:00Z
      level: ok
```

@tab Multiple Notices

```yaml
ui:
  notices:
    - title: Current Incident - Email Service
      description: Our email notification service is currently experiencing issues. We are working to resolve this as quickly as possible.
      timestamp: 2025-08-12T16:00:00Z
      level: error
    - title: Scheduled Maintenance Reminder
      description: Reminder that scheduled maintenance is planned for this weekend. See our previous notice for details.
      timestamp: 2025-08-12T12:00:00Z
      level: warning
    - title: New Feature Launch
      description: We're excited to announce the launch of our new dashboard features, now available to all users.
      timestamp: 2025-08-10T09:00:00Z
      level: ok
```

:::

