# TLS Certificate
The `!TlsCert` target type establishes a TLS connection with a remote service and reports
the certificate it presents, so that you can alert on impending expiry, unexpected issuers,
or a loss of trust.

It performs no application-level handshake of its own, which means it works against anything
that speaks TLS over TCP — HTTPS, gRPC, SMTPS, message brokers, or a raw socket.

::: tip
The probe deliberately **does not fail** when a certificate is untrusted, expired, or issued
for the wrong hostname. Those conditions are reported as fields so that you can decide, in
your `checks`, which of them should raise an incident.
:::

## Example
An example of this would be alerting a month before a certificate is due to expire.

```yaml{7-9}
probes:
  - name: tls.example
    policy:
      interval: 1h
      timeout: 10s
      retries: 3
    target: !TlsCert
      host: example.com:443
      alpn: [h2, http/1.1]
    checks:
      - tls.trusted == true
      - tls.hostname_valid == true
      - tls.expires_in > 30d
      - tls.issuer_cn contains "Let's Encrypt"
```

The same target can be pointed at a privately-signed internal service by supplying the
certificate authority which should be trusted for it.

```yaml{9-15}
probes:
  - name: tls.internal
    policy:
      interval: 1h
      timeout: 10s
      retries: 3
    target: !TlsCert
      host: broker.internal:9093
      ca_cert: |
        -----BEGIN CERTIFICATE-----
        MIIBkTCB+wIJAJ...
        -----END CERTIFICATE-----
    checks:
      - tls.trusted == true
      - tls.not_after > now() + 90d
```

## Inputs

### host <Badge text="required" type="danger" />
The `host` property is used to specify the `host:port` pair that you wish to connect to.
Unlike the [`!Http`](./http.md) target, no scheme is used and no default port is assumed,
so the port must always be provided.

### server_name
The `server_name` property overrides the hostname sent in the TLS Server Name Indication
extension and used to validate the certificate. By default the host portion of `host` is
used, which is usually what you want — set this when you are connecting to a specific
node by IP address, but expect a certificate issued for a shared service name.

### ca_cert
The `ca_cert` property is used to provide a PEM-encoded certificate authority which should
be used to validate the server's certificate. When this is provided it **replaces** the
system's `ca-certificates` bundle rather than adding to it, so a certificate signed by a
public authority will be reported as untrusted.

### alpn
The `alpn` property is used to specify a list of protocols to offer during ALPN
negotiation (for example `[h2, http/1.1]`). Some servers require ALPN to be offered before
they will complete a handshake, and the protocol that was agreed upon is reported as
`tls.alpn`.

## Outputs

### tls.trusted
The `tls.trusted` field is `true` when the certificate chain was successfully validated
against the trusted authorities (either the system's `ca-certificates` bundle or the
`ca_cert` you supplied), including its validity period and the hostname it was issued for.

### tls.error
The `tls.error` field contains a description of why validation failed, or `null` when the
certificate was trusted. It is useful for including the underlying cause in an alert.

### tls.hostname_valid
The `tls.hostname_valid` field is `true` when the certificate was issued for the hostname
being connected to, honouring wildcard names such as `*.example.com`.

### tls.expired
The `tls.expired` field is `true` when the current time falls outside the certificate's
validity period, whether because it has expired or because it is not yet valid.

### tls.not_before / tls.not_after
The `tls.not_before` and `tls.not_after` fields contain the bounds of the certificate's
validity period as timestamps, which can be compared against the current time using the
`now()` function.

```yaml
checks:
  - tls.not_after > now() + 30d
```

### tls.expires_in
The `tls.expires_in` field contains the time remaining until the certificate expires, as a
duration which can be compared against duration literals directly. It is negative once the
certificate has expired.

```yaml
checks:
  - tls.expires_in > 30d
```

### tls.subject / tls.issuer
The `tls.subject` and `tls.issuer` fields contain the full distinguished names of the
certificate's subject and of the authority which issued it.

### tls.subject_cn / tls.issuer_cn
The `tls.subject_cn` and `tls.issuer_cn` fields contain just the common name portion of the
subject and issuer, which is usually the more convenient thing to assert against.

### tls.sans
The `tls.sans` field contains the list of subject alternative names (DNS names and IP
addresses) which the certificate is valid for.

```yaml
checks:
  - '"api.example.com" in tls.sans'
```

### tls.serial
The `tls.serial` field contains the certificate's serial number in its lowercase
hex-encoded form.

### tls.thumbprint
The `tls.thumbprint` field contains the lowercase hex-encoded SHA-256 digest of the
certificate, which can be used to pin a specific certificate.

### tls.signature_algorithm
The `tls.signature_algorithm` field contains the name of the algorithm used to sign the
certificate, such as `ecdsa-with-SHA384`.

### tls.chain.length
The `tls.chain.length` field contains the number of certificates the server presented,
including its own. A server which does not send its intermediates will report `1`, which
often works in browsers (which cache intermediates) but fails elsewhere.

### tls.chain.subjects / tls.chain.issuers
The `tls.chain.subjects` and `tls.chain.issuers` fields contain the distinguished names of
every certificate in the presented chain, in the order the server sent them.

### tls.version
The `tls.version` field contains the protocol version that was negotiated, such as
`TLSv1.3`.

### tls.cipher_suite
The `tls.cipher_suite` field contains the name of the cipher suite that was negotiated,
such as `TLS13_AES_256_GCM_SHA384`.

### tls.alpn
The `tls.alpn` field contains the protocol agreed upon during ALPN negotiation, or `null`
when no ALPN protocols were offered or none were agreed.

### net.ip
The `net.ip` field contains the IP address that the hostname resolved to and which the
connection was made to.
