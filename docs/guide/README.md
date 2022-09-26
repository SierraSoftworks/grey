# Getting Started
Welcome to Grey, we hope you'll find it to be a simple, easy to use, and effective
tool in your arsenal for monitoring the health of your services. Grey is designed
to be extremely lightweight, and that includes the process of defining and running
probes. This guide is intended to walk you through how to get started with Grey
with a minimum of fuss.

::: tip
This guide assumes that you have an OpenTelemetry collection endpoint available and
ready to accept tracing data. If you're looking to test Grey locally you'll probably
find that Jaeger's [all-in-one](https://www.jaegertracing.io/docs/1.38/getting-started/)
setup is a great place to start.
:::

#### Step #1: Installation
You can download the latest version of Grey from our [GitHub releases][release] page.
We include pre-compiled binaries for many different platforms, including Windows, Linux,
and MacOS in both `amd64` and `arm64` variants.

#### Step #2: Configuration
Grey is configured using a YAML file that you can specify using the `-c` command line
flag. The configuration file contains a list of probes which will be scheduled and
executed by Grey. You can read more about configuring Grey in the [configuration guide](./configuration.md).

To get started, let's define a simple probe that will check the status of a website.

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
      http.header.content-type: !Contains "text/html"
```

#### Step #3: Telemetry
Grey is designed to record the health of your services through the use of OpenTelemetry tracing
data. This allows you to use the tools you're already familiar with to understand the behaviour
of your services and to leverage distributed tracing to understand failures more rapidly.

Grey uses the standard OpenTelemetry environment variables to configure the telemetry endpoint
and headers (often used for authentication). You can read more about configuring telemetry in
the [telemetry guide](./telemetry.md).

To get started, let's configure Grey to send telemetry to a Jaeger instance running on the local
machine.

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
```

#### Step #4: Execution
Once you're done configuring your environment, you can start Grey.

```bash
grey -c config.yaml
```

[release]: https://github.com/SierraSoftworks/grey/releases
[new-issue]: https://github.com/SierraSoftworks/grey/issues/new