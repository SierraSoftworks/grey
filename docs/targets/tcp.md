# TCP
The `!Tcp` target type is designed to test that a TCP connection can be established to a
given host and port. It is primarily used in situations where you wish to validate that
a network exposed service is accessible where the protocol is not supported by Grey.

## Example
An example of this would be checking that your SMTP server is accepting connections.

```yaml{7-8}
probes:
  - name: smtp.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Tcp
      host: smtp.example.com:25
    validators:
      net.ip: !Equals "127.0.0.1"
```

## Inputs

### host <Badge text="required" type="danger" />
The `host` property is used to specify the host and port which you would like to connect to.
The host should be specified in the format `host:port`.

## Outputs

### net.ip
The `net.ip` property will contain the IP address of the host that was connected to. This
will be a string containing the IP address in either its standard IPv4 or IPv6 representation.
