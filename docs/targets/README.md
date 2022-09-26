# Introduction
Grey includes support for a number of different synthetic probing target types, which should
allow you to cover many of the most common service types.

::: tip
For a quick introduction to using Grey to probe a service, take a look at the
[Usage Guide](../guide/README.md).
:::

When defining a probe, you can specify the target type using the `!Http` syntax. These
target types each accept a distinct set of configuration options which are documented
on their respective pages.

## Example

```yaml
probes:
  - name: http.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Http
      url: https://example.com
    validators:
      http.status: !OneOf [200]
      http.header.content-type: !Contains "text/html"

  - name: tcp.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Tcp
      host: example.com:6379
    validators:
      net.ip: !Equals "127.0.0.1"

  - name: dns.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Dns
      domain: example.com
      record_type: MX
    validators:
      dns.answers: !Contains "10 smtp.example.com"
```
