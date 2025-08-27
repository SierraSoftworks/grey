# gRPC
The `!Grpc` target type is designed to perform health checks on gRPC services using the standard
[gRPC Health Checking Protocol](https://github.com/grpc/grpc/blob/master/doc/health-checking.md).
This enables you to monitor the health of your gRPC microservices and ensure they are responding
correctly to health check requests.

## Example
An example of this would be checking that a gRPC service is healthy and responding to requests.

```yaml{7-9}
probes:
  - name: grpc.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Grpc
      url: https://api.example.com:443
      service: "myservice.v1.MyService"
    validators:
      grpc.status: !Equals "SERVING"
      grpc.status_code: !Equals 1
```

You can also check the overall server health by omitting the `service` field:

```yaml{7-8}
probes:
  - name: grpc.server-health
    policy:
      interval: 10000
      timeout: 3000
      retries: 2
    target: !Grpc
      url: https://api.example.com:443
    validators:
      grpc.status: !Equals "SERVING"
```

## Inputs

### url <Badge text="required" type="danger" />
The `url` property is used to specify the gRPC endpoint that you wish to perform a health check against.
It should be a fully-qualified URI containing the scheme (`http://` for plaintext or `https://` for TLS),
host, and port.

::: tip
Most gRPC services use TLS, so you'll typically want to use `https://` as the scheme. The implementation
automatically configures TLS with native root certificates for secure connections.
:::

### service <Badge text="default: empty string"/>
The `service` property is used to specify the name of the specific service you want to check the health of.
If left empty (or omitted), the health check will query the overall server health rather than a specific service.

The service name should match the fully-qualified service name as defined in your gRPC service's protobuf
definition (e.g., `mypackage.v1.MyService`).

### ca_cert
The `ca_cert` property allows you to specify a custom Certificate Authority (CA) certificate in PEM format
to use when validating the server's TLS certificate. This is useful when connecting to gRPC services that
use self-signed certificates or certificates issued by a private CA.

```yaml{9-15}
target: !Grpc
  url: https://internal-api.company.com:443
  service: "myservice.v1.MyService"  
  ca_cert: |
    -----BEGIN CERTIFICATE-----
    MIIDXTCCAkWgAwIBAgIJAKoK/heBjcOuMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV
    BAYTAkFVMRMwEQYDVQQIDApTb21lLVN0YXRlMSEwHwYDVQQKDBhJbnRlcm5ldCBX
    ...
    -----END CERTIFICATE-----
```

If not specified, the target will use the system's native root certificates for TLS validation.

## Outputs

### grpc.status
The `grpc.status` field contains the health status returned by the gRPC health service. The possible
values are:
- `UNKNOWN` - The health status is unknown (default state)
- `SERVING` - The service is healthy and serving requests
- `NOT_SERVING` - The service is not healthy or not serving requests
- `SERVICE_UNKNOWN` - The specified service is not known to the server (only returned by the Watch method)

### grpc.status_code
The `grpc.status_code` field contains the numeric representation of the health status:
- `0` - UNKNOWN
- `1` - SERVING  
- `2` - NOT_SERVING
- `3` - SERVICE_UNKNOWN

This can be useful for numeric comparisons in validators.

## Protocol Details

This target implements the standard [gRPC Health Checking Protocol](https://github.com/grpc/grpc/blob/master/doc/health-checking.md)
by making a `Check` RPC call to the `/grpc.health.v1.Health/Check` endpoint. The implementation uses
the `tonic-health` crate which provides pre-compiled protobuf types for the health service.

## TLS Configuration

The gRPC target provides flexible TLS configuration options:

- **Native Root Certificates**: By default, the target uses the system's native root certificate store for validating server certificates
- **Custom CA Certificates**: Use the `ca_cert` property to specify custom Certificate Authority certificates for private or self-signed certificates
- **User Agent**: The client automatically sets a user agent header identifying itself as `SierraSoftworks/grey@{version}`

::: tip Custom CA Certificates
When using custom CA certificates, make sure the certificate is in PEM format and includes the complete certificate chain if necessary. The certificate should be the CA that signed the server's certificate, not the server certificate itself.
:::

## Common Use Cases

### Microservice Health Monitoring
Monitor the health of individual microservices in a distributed system:

```yaml
probes:
  - name: user-service
    target: !Grpc
      url: https://user-service:443
      service: "users.v1.UserService"
    validators:
      grpc.status: !Equals "SERVING"
      
  - name: order-service  
    target: !Grpc
      url: https://order-service:443
      service: "orders.v1.OrderService"
    validators:
      grpc.status: !Equals "SERVING"
```

### Load Balancer Backend Health
Check the health of gRPC backends behind a load balancer:

```yaml
probes:
  - name: backend-1
    target: !Grpc
      url: https://backend-1.internal:443
    validators:
      grpc.status: !OneOf ["SERVING", "UNKNOWN"]
      
  - name: backend-2
    target: !Grpc
      url: https://backend-2.internal:443  
    validators:
      grpc.status: !OneOf ["SERVING", "UNKNOWN"]
```

### Development Environment Checks
Verify that development services are running and healthy:

```yaml
probes:
  - name: local-api
    target: !Grpc
      url: http://localhost:50051
      service: "api.v1.ApiService"
    policy:
      interval: 2000
    validators:
      grpc.status: !Equals "SERVING"
```

### Private CA or Self-Signed Certificates
Connect to gRPC services using custom Certificate Authority certificates:

```yaml
probes:
  - name: internal-service
    target: !Grpc
      url: https://internal.company.com:443
      service: "internal.v1.InternalService"
      ca_cert: |
        -----BEGIN CERTIFICATE-----
        MIIDXTCCAkWgAwIBAgIJAKoK/heBjcOuMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV
        BAYTAkFVMRMwEQYDVQQIDApTb21lLVN0YXRlMSEwHwYDVQQKDBhJbnRlcm5ldCBX
        aWRnaXRzIFB0eSBMdGQwHhcNMTcwODI4MTUxNjU5WhcNMTgwODI4MTUxNjU5WjBF
        MQswCQYDVQQGEwJBVTETMBEGA1UECAwKU29tZS1TdGF0ZTEhMB8GA1UECgwYSW50
        ZXJuZXQgV2lkZ2l0cyBQdHkgTHRkMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIB
        CgKCAQEAwvKCS138dhybOhWHKlXLO8kB+2pQYOF4zWXJX7SiE0EWCKmNbLrKKZk7
        -----END CERTIFICATE-----
    validators:
      grpc.status: !Equals "SERVING"
```
