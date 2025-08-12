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
      http.status: !OneOf [200]
      http.header.content-type: !Equals "text/html; charset=ISO-8859-1"
    tags:
      service: Google

ui:
  enabled: true
  listen: 127.0.0.1:3002
```

## Targets

### `http`
The `http` target makes requests to HTTP(S) endpoints and allows you to validate
the response. It accepts the following options:

- `url` (required): The URL to make the request to.
- `method` (optional, default: `GET`): The HTTP method to use for the request.
- `headers` (optional): A map of headers to include in the request.
- `body` (optional): The body to include in the request.
- `no_verify` (optional, default: `false`): Whether to disable TLS verification.

It exposes the following fields for validation:

- `http.status`: The HTTP status code of the response.
- `http.header.<header>`: The value of the specified header in the response, header names are lowercased.
- `http.version`: The HTTP version of the response.
- `http.body`: The body of the response.

### `tcp`
The `tcp` target makes a TCP connection to a host and port to confirm that it
is available and accepting connections. It accepts the following options:

- `host` (required): The host and port to connect to (e.g. `example.com:80`).

It exposes the following fields for validation:

- `net.ip`: The IP address of the host that was connected to.

### `dns`
The `dns` target makes a DNS query and allows you to assert that the response
matches your provided expectations. It accepts the following options:

- `domain` (required): The domain name to query.
- `record_type` (optional, default: `A`): The type of DNS record to query for.

It exposes the following fields for validation:

- `dns.answers`: The list of answers to the DNS query.

## Validators
Validation is performed using one of the provided validation functions.

### `!Equals`
The `!Equals` validator asserts that the value of the field is equal to the
provided value.

### `!OneOf`
The `!OneOf` validator asserts that the value of the field is one of the
provided values.

### `!Contains`
The `!Contains` validator asserts that the value of the field contains the
provided value. If both the field and the provided value are a string, it will
perform a string contains check, while if the field is a list, it will perform
a list contains check.

## OpenTelemetry
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

## Status Page
Grey also includes support for sharing a status page with your customers. This can be
useful for providing updates on the health of your infrastructure to customers. The UI
is disabled by default, and can be configured with your own company logo and title, as
well as links and customer notices.

To enable the user interface, you'll need to set `ui.enabled` to `true` in your
configuration file. But you can easily configure additional options as you see fit.

```yaml
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
