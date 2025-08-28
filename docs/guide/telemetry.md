# Telemetry
Grey emits probe execution telemetry using OpenTelemetry tracing. This allows you to easily
visualize the probes being executed against your services, the time taken to conduct these
probes, and the way in which your systems are processing probe requests.

To configure telemetry emission for Grey, you should use the following environment variables.

```bash
# Jaeger running on your local machine
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317

# Honeycomb
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.honeycomb.io:443
export OTEL_EXPORTER_OTLP_HEADERS=x-honeycomb-team=YOUR_API_KEY

# New Relic
export OTEL_EXPORTER_OTLP_ENDPOINT=https://otlp.eu01.nr-data.net:443
export OTEL_EXPORTER_OTLP_HEADERS=api-key=YOUR_API_KEY

# Lightstep
export OTEL_EXPORTER_OTLP_ENDPOINT=https://ingest.lightstep.com:443
export OTEL_EXPORTER_OTLP_HEADERS=lightstep-access-token=YOUR_API_KEY
```

## `OTEL_EXPORTER_OTLP_ENDPOINT`
This environment variable should be set to the URL of the OpenTelemetry collector endpoint
that you wish to emit telemetry to. This should be a gRPC compatible endpoint and should
use a trusted TLS certificate (e.g. a certificate signed by a well known CA).

## `OTEL_EXPORTER_OTLP_HEADERS`
This environment variable should be set to a comma separated list of headers that should be
sent with each telemetry emission. This is useful for providing authentication credentials
to the OpenTelemetry collector endpoint and is required by some providers (e.g. Honeycomb).

You can provide multiple headers by separating them with a comma. For example, to provide
a legacy Honeycomb team and dataset, you would specify:
`x-honeycomb-team=YOUR_TEAM,x-honeycomb-dataset=YOUR_DATASET`.
