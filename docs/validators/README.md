# Introduction
Grey exposes a standard set of validators which can be used to assert that a service's response
matches your expectations. These validators are designed to be straightforward to use and broadly
applicable, keeping the configuration simple and easy to understand.

::: tip
For a quick introduction to using Grey to probe a service, take a look at the
[Usage Guide](../guide/README.md).
:::

When defining a probe, you can specify the validators using the `http.status: !OneOf [200]` syntax.
Each validator targets a specific field and accepts a distinct set of configuration options which
are documented on their respective pages.

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
```
