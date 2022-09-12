# Grey
**Lightweight health probing with support for OpenTelemetry**

Grey is a health probing system which is designed to simplify the regular
checking of service health from external locations. It is built in Rust
and integrates native support for OpenTelemetry, with the goal of providing
operators with a detailed view into the behaviour of their systems and how
they fail.

## Features
- Extremely lightweight, can be run on every server in your fleet if you wish.
- Native OpenTelemetry integration, including trace propagation.
- Supports multiple health probes, including HTTP, TCP, and DNS.

## Usage
Grey is a self contained binary which is expected to be executed with a YAML
configuration file as its only input. The configuration file looks like this:

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
      - !HttpStatus [200]
      - !HttpHeader { name: "content-type", value: "text/html; charset=ISO-8859-1" }
```

### OpenTelemetry
Grey uses OpenTelemetry to export trace information about each of the probes that
it has executed. This can be emitted to any gRPC compatible OpenTelemetry endpoint
by configuring the `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable. For example,
to emit traces to a local Jaeger instance, you would run:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317" \
    grey --config config.yaml
```

If you need to provide headers for the OTLP exporter, you can do so by setting the
`OTEL_EXPORTER_OTLP_HEADERS` environment variable. For example, to provide a Honeycomb
API key, you would run:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT="https://api.honeycomb.io" \
OTEL_EXPORTER_OTLP_HEADERS="x-honeycomb-team=YOUR_API_KEY" \
    grey --config config.yaml
```
