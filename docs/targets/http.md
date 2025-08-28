# HTTP
The `!Http` target type is designed to make requests to services over HTTP(S) to enable
external health checking and basic functional validation. It allows you to make a wide
range of requests and will expose the response for validation.

## Example
An example of this would be checking that an authenticated service is operating correctly.

```yaml{7-12}
probes:
  - name: http.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Http
      url: https://example.com
      method: POST
      headers:
        Authorization: Bearer <token>
      body: '{"foo": "bar"}'
      no_verify: false
    validators:
      http.status: !OneOf [200, 201]
      http.header.content-type: !Equals "application/json"
      http.body: !Contains "foo"
```

## Inputs

### url <Badge text="required" type="danger" />
The `url` property is used to specify the endpoint that you wish to make a request to.
It should be a fully-qualified URI containing the scheme, host, and optionally the path.

::: tip
If you wish to provide authentication credentials, you should do so using the `headers`
property rather than specifying them in the `url`.
:::

### method <Badge text="default: GET"/>
The `method` property is used to specify the HTTP method that you wish to use for the
request. It should be a valid HTTP method, such as `GET`, `POST`, `PUT`, `DELETE`, etc.

### headers
The `headers` property is used to specify a map of headers that you wish to include in
the request. The keys should be the header names and the values should be the header
values.

### body
The `body` property is used to specify the body that you wish to include in the request.
It should be a string containing the body that you wish to send, in its UTF-8 encoded
format.

### no_verify <Badge text="default: false"/>
The `no_verify` property is used to disable TLS verification for the request.
This is useful in scenarios where the remote service is running
with a self-signed certificate and/or you wish to ignore potentially
expired certificates.

## Outputs

### http.status
The `http.status` field will contain the HTTP status code returned by the remote service.

### http.header.`<header>`
The `http.header.<header>` field contains any response headers returned by the remote service,
with the `<header>` portion of the field name being the lowercase name of the header. For example,
the `Content-Type` header would be available as `http.header.content-type`.

### http.version
The `http.version` field contains the HTTP version of the response and can be used to ensure that
the service is responding with a specific version of the protocol.

### http.body
The `http.body` field contains the body of the response in its UTF-8 decoded string format. It
can be used to validate the response body against a set of expectations.
