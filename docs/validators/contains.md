# Contains
The `!Contains` validator allows you to assert that a field contains a specific value. It can be used to perform both
substring comparisons, as well as list membership checks. The specific behaviour will be determined by the type of the
field being tested.

The `!Contains` validator accepts a single argument, which is the value to check for. If performing string comparisons
this value **MUST** be a string, while if performing list membership it **MUST** be the same type as the list's elements.

::: warning Deprecated
Validators are deprecated in favour of [checks](../checks/README.md). Replace `!Contains`
with the `contains` (or `in`) operator — see [Migrating to a check](#migrating-to-a-check)
below.
:::

## Example

```yaml{10-11,22-24}
probes:
  - name: http.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Http
      url: https://example.com
    validators:
      # This validates that the Content-Type header contains the substring "text/html"
      http.header.content-type: !Contains "text/html"

  - name: dns.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Dns
      domain: example.com
      record_type: MX
    validators:
      # This validates that the MX record for example.com contains at least one record
      # with the value "10 smtp.example.com".
      dns.answers: !Contains "10 smtp.example.com"
```

## Migrating to a check

Use the `contains` operator for substring checks, and either `contains` or the more natural
`in` operator for list membership:

```yaml
# Before:
    validators:
      http.header.content-type: !Contains "text/html"
      dns.answers: !Contains "10 smtp.example.com"

# After:
    checks:
      - http.header.content-type contains "text/html"
      - "10 smtp.example.com" in dns.answers
```

::: tip
String `contains` is **case-insensitive**; use `contains_cs` when you need an exact,
case-sensitive substring match.
:::
