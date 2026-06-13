# Equals
The `!Equals` validator asserts that the value of the field is equal to the provided value.
it accepts a single argument and requires that both the type and value of the field matches
that of the value provided.

::: warning Deprecated
Validators are deprecated in favour of [checks](../checks/README.md). Replace `!Equals` with
the `==` operator — see [Migrating to a check](#migrating-to-a-check) below.
:::

## Example

```yaml{10-11}
probes:
  - name: http.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Http
      url: https://example.com
    validators:
      # This validates that the status code of the response is exactly 200
      http.status: !Equals 200
```

## Migrating to a check

Replace the validator with the `==` operator referencing the same field:

```yaml
# Before:
    validators:
      http.status: !Equals 200

# After:
    checks:
      - http.status == 200
```

::: tip
String equality with `==` is **case-insensitive**. For an exact, case-sensitive match use an
anchored regular expression instead, e.g. `http.header.content-type matches r"^text/html$"`.
:::
