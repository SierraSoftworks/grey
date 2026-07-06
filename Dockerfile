# NOTE: This Dockerfile depends on you building the grey binary first.
# It will then package that binary into the image, and use that as the entrypoint.
# This means that running `docker build` is not a repeatable way to build the same
# image, but the benefit is much faster cross-platform builds; a net win.
FROM ubuntu:24.04

LABEL org.opencontainers.image.source=https://github.com/SierraSoftworks/grey
LABEL org.opencontainers.image.description="Lightweight OpenTelemetry native health probing system"

RUN apt-get update && apt-get install -y \
  ca-certificates \
  openssl

ADD ./grey /usr/local/bin/grey

ENTRYPOINT ["/usr/local/bin/grey"]
