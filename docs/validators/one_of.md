# OneOf
The `!OneOf` validator asserts that the value of the field is one of the provided values.
It is particularly useful in situations where multiple values are valid, such as when
validating the status code of an HTTP response.

The `!OneOf` validator accepts a list of values as its argument. The values contained within
its list must match the type of of the field being validated. For example, if the field is a string,
the values in the list must also be strings.

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
      # This validates that the status code of the response is either 200 or 204
      http.status: !OneOf [200, 204]
```
