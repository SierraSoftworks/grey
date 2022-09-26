# Equals
The `!Equals` validator asserts that the value of the field is equal to the provided value.
it accepts a single argument and requires that both the type and value of the field matches
that of the value provided.

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