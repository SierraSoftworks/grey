# DNS
The `!Dns` target type is designed to allow you to perform external validation of your
DNS records and infrastructure. It does so by making a DNS query to the system's configured
DNS resolver and exposing the `dns.answers` for validation.

## Example
An example of this would be checking that your DNS mail records are configured correctly.

```yaml{7-9}
probes:
  - name: dns.example
    policy:
      interval: 5000
      timeout: 2000
      retries: 3
    target: !Dns
      domain: example.com.
      record_type: MX
    validators:
      dns.answers: !Contains "10 smtp.example.com"
```

## Inputs

### domain <Badge text="required" type="danger" />
The `domain` property is used to specify the DNS domain record which you would like to
query.

::: tip
You should include a trailing `.` on your domain name to improve lookup performance by
avoiding lookups against alternate DNS parent domains.
:::

### record_type <Badge text="default: A"/>
The `record_type` property is used to specify the DNS record type which you would like to
query. The default record type, if none is provided, is `A` - corresponding to the IPv4
address associated with the domain.

## Outputs

### dns.answers
The `dns.answers` property will contain the list of answers returned by the DNS query. This
will be a list of strings, each containing the answer for the record type that was queried.

```yaml
dns.answers:
  - "10 smtp.example.com"
  - "20 smtp2.example.com"
```

::: tip
The easiest way to validate the contents of the `dns.answers` property is to use the
`!Contains` validator.
:::