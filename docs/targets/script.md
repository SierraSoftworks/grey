# Script
The `!Script` target type is built on top of [Deno](https://deno.land) and
allows you to write custom JavaScript probes to conduct complex health
evaluations against your service. This includes executing complex workflows
like signing into a website, or implementing more powerful validation than
is possible with the standard [validators](../validators/README.md).

## Example
A good example here would be an interactive authentication flow which requires
multiple web requests.

```yaml{7-39}
probes:
  - name: script.example
    policy:
      interval: 60000
      timeout: 5000
      retries: 3
    target: !Script
      code: |
        const auth = await fetch("https://example.com/api/v1/login", {
            method: "POST",
            headers: {
                "Accept": "application/json",
                "Content-Type": "application/json"
            },
            body: JSON.stringify({
                "username": "test-user",
                "password": "test-user-password"
            })
        })

        // Store the authentication request status code in the output
        setOutput("auth.status", auth.status)

        if (auth.ok) {
            const authPayload = await auth.json()

            const profile = await fetch("https://example.com/api/v1/profile", {
                headers: {
                    "Accept": "application/json",
                    "Authorization": "Bearer ${authPayload.token}"
                }
            })

            setOutput("profile.status", profile.status)

            if (profile.ok) {
                const profilePayload = await profile.json()
                setOutput("profile.username", profilePayload.username)
            }
        }
    validators:
      auth.status: !Equals 200
      profile.status: !Equals 200
      profile.username: !Equals "test-user"
```

## Inputs

### code <Badge text="required" type="danger" />
The `code` property is used to specify the JavaScript code which should be
executed as part of your probe.

::: warning
Your code may `await` asynchronous operations and will stop executing once
all synchronous and `await`ed operations have finished running. *Orphaned
promises will not be run to completion, so make sure you `await` them.*
:::

### args
The `args` property can be used to provide customizable arguments to your
script. These arguments should appear as a list of strings in your probe
definition and may be accessed through the
[`Deno.args`](https://examples.deno.land/command-line-arguments)
array in your code. Conceptually, these are the same as command line arguments
and can be paired with YAML's ability to leverage references to re-use scripts
across multiple probes.

```yaml
probes:
  - name: script.example
    target: !Script
      code: &myScript |
        // Your script code here
      args:
        - "--foo"
        - "bar"
    # ...

  - name: script.example2
    target: !Script
      code: *myScript
      args:
        - "--baz"
        - "qux"
    # ...
```

The `script.exit_code` value will be set to the process exit code associated
with your script execution, this will usually be `0` if the script ran
successfully and non-zero if it failed.

### Custom Outputs
If you wish to expose additional outputs from your script, you can do so using
the `setOutput(key, value)` function in the script environment. This function
will set an output which may then be checked by one or more of the
[validators](../validators/README.md).

```js
setOutput("my.value", 42)
```

::: warning
Only primitive values (`null`, `boolean`, `number`, `string`) are supported
by the output system. More complex types will automatically be converted to
their JSON string representation during the output process.
:::

## Runtime Environment
The script runtime environment is built on top of [Deno](https://deno.land)
and includes most Web APIs, allowing you to use features like
[fetch()](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API),
[WebSocket](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket),
and more. It also includes Deno-specific APIs which allow you to connect
directly to TCP, resolve DNS names, and more.

On top of these APIs, we also provide a couple of helpers to improve the
integration with Grey.

::: warning
The runtime environment currently does **NOT** support the use of the `import`
directive to import additional scripts either from the local filesystem or
from remote endpoints.
:::

### `setOutput(key: string, value: any): never`
This method allows you to emit a new output value from your probe which
can then be interrogated by the [validators](../validators/README.md)
that you have defined in your Grey configuration.

```js
const resp = await fetch("https://example.com")

setOutput('http.status_code', resp.status)
setOutput('http.body', await resp.text())
```

### `getTraceId(): string`
This method retrieves the current OpenTelemetry Trace ID for your probe
execution, allowing you to pass this information along in requests to
downstream systems.

### `getTraceHeaders(): { traceparent: string, tracestate: string }`
This method retrieves the W3C Trace Context headers used to propagate
trace information across systems. These may be used directly in calls
to `fetch()` and other similar APIs to propagate trace information.

```js
await fetch("https://example.com/api/v1/ping", {
    headers: {
        "Accept": "application/json",

        // Pass trace information to the remote service
        ...getTraceHeaders()
    }
})
```
