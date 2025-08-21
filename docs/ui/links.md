# Links

Links allow you to add navigation items to your status page, providing
quick access to relevant resources such as support documentation,
contact information, or related services.

::: tip
Links open in a new tab by default, and are marked with `rel="noopener noreferrer"` to prevent
leaking of information about your status page to the destinations.
:::

## Configuration

Links are configured as an array under the `ui.links` section of your configuration file:

```yaml
ui:
  enabled: true
  links:
    - title: GitHub
      url: https://github.com/SierraSoftworks/grey
    - title: Documentation
      url: https://grey.sierrasoftworks.com
    - title: Support
      url: https://support.example.com
    - title: Status Updates
      url: https://status.example.com
```

## Link Properties

### title <Badge text="required" type="danger" />

The text to display for the link. Keep it concise and descriptive.

### url <Badge text="required" type="danger" />

The URL to navigate to when the link is clicked. Must be a valid HTTP or HTTPS URL.

## Examples

::: code-tabs

@tab Basic Links

```yaml
ui:
  links:
    - title: Website
      url: https://example.com
    - title: Contact Us
      url: https://example.com/contact
```

@tab Support and Documentation

```yaml
ui:
  links:
    - title: Documentation
      url: https://docs.example.com
    - title: API Reference
      url: https://api.example.com/docs
    - title: Support Portal
      url: https://support.example.com
    - title: System Status
      url: https://status.example.com
```

@tab Developer Resources

```yaml
ui:
  links:
    - title: GitHub Repository
      url: https://github.com/company/project
    - title: Issue Tracker
      url: https://github.com/company/project/issues
    - title: Changelog
      url: https://github.com/company/project/releases
```

:::
