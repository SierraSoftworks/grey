---
home: true

actions:
    - text: Get Started
      link: /guide/
    - text: Download
      link: https://github.com/SierraSoftworks/grey/releases
      type: secondary

features:
    - title: Lightweight
      details: |
        With an average memory footprint of just 4MB, Grey is designed to be effectively free to run across your infrastructure
        while providing exceptional visibility.

    - title: Trivial Configuration
      details: |
        Grey's intuitive YAML configuration makes defining and deploying new probes a breeze.

    - title: Best in Class Observability
      details: |
        With native OpenTelemetry integration and trace propagation, it has never been easier to dive into the cause of a
        probe failure.
---


Grey is a synthetic monitoring tool designed to help you measure and understand customer impacting outages within
minutes. It is designed to be trivially lightweight, letting you run it anywhere in your infrastructure or on the
cheapest public cloud instances money can buy, giving you exceptional breadth of visibility and high signal to noise
ratios.

## Features
 - **Extremely low memory footprint** allows you to run Grey in resource constrained environments at low cost.
 - **Native ARM binaries** allow you to run Grey on embedded Linux devices like Raspberry Pis for cheap distributed probing.

## Example

```yaml
probes:
  - name: google.search
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Http
      url: https://google.com?q=grey+healthcheck+system
    validators:
      http.status: !OneOf [200]
      http.header.content-type: !Equals "text/html; charset=ISO-8859-1"
    tags:
      service: Google

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


<ClientOnly>
    <Contributors repo="SierraSoftworks/grey" />
    <Releases repo="SierraSoftworks/grey" />
</ClientOnly>