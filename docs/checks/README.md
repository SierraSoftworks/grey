# Introduction
**Checks** are the recommended way to validate a probe's result. A check is an expression
written in the [`filt-rs`](https://docs.rs/filt-rs/latest/filt_rs/) filter language and
evaluated against the *whole* sample, so a single check can compare fields, combine
conditions, match patterns, and more.

::: warning
Checks supersede the older per-field [validators](../validators/README.md), which are now
**deprecated**. Validators continue to work, so you can migrate at your own pace, but new
probes should use checks and existing probes should move across when convenient. See
[Migrating from validators](#migrating-from-validators) for a field-by-field guide.
:::

::: tip
Validators and checks can be used together on the same probe, so you can adopt checks
gradually — expression by expression — while you migrate.
:::

Each entry under `checks:` is parsed when the configuration is loaded (so a malformed
expression is reported immediately, not at run time), and each is reported as its own
pass/fail validation in the dashboard and telemetry, labelled with the expression itself.
A probe fails as soon as any one of its checks does not match.

## Example

```yaml{12-15}
probes:
  - name: http.example
    policy:
      interval: 5s
      timeout: 2s
      retries: 3
    target: !Http
      url: https://example.com
    validators:
      # Classic validators still work exactly as before.
      http.status: !OneOf [200]
    checks:
      # A check is a single expression evaluated against the whole sample.
      - http.status >= 200 && http.status < 300
      - http.header.content-type contains "html"
```

## Expression language

A check is any single `filt-rs` expression that evaluates to a truthy value when the probe
should be considered healthy. Sample fields are referenced by the same dotted names used by
validators (for example `http.status` or `http.header.content-type`); names containing `.`
and `-` are supported directly, and an unknown field resolves to `null`.

The most useful operators are summarised below — see the
[`filt-rs` documentation](https://docs.rs/filt-rs/latest/filt_rs/) for the full language.

| Operator                       | Meaning                                                      |
|--------------------------------|-------------------------------------------------------------|
| `\|\|`, `&&`, `!`              | Logical OR, AND, NOT.                                        |
| `==`, `!=`                     | Equality (strings compare case-insensitively).              |
| `>`, `>=`, `<`, `<=`           | Ordering comparisons.                                        |
| `contains`, `in`               | Substring / tuple membership (`a in b` ≡ `b contains a`).   |
| `startswith`, `endswith`       | String prefix / suffix tests.                               |
| `like`                         | Case-insensitive glob match (`*` and `?` wildcards).        |
| `matches`                      | Regular-expression match.                                   |
| `+`, `-`                       | Arithmetic on numbers, datetimes, and durations.            |
| `now()`                        | The current UTC time, for relative-time comparisons.        |

The string operators are case-insensitive by default; each has a case-sensitive `_cs`
variant (`contains_cs`, `startswith_cs`, …). String literals use double quotes (`"text"`),
and raw strings (`r"^v\d+$"`) are handy for regular expressions.

## Migrating from validators

Every validator has a direct check equivalent. A validator keyed by a field path becomes an
expression that references that same field, so migrating is largely mechanical:

| Validator                                 | Equivalent check                          |
|-------------------------------------------|-------------------------------------------|
| `http.status: !Equals 200`                | `http.status == 200`                      |
| `http.status: !NotEquals 500`             | `http.status != 500`                      |
| `http.status: !OneOf [200, 204]`          | `http.status in [200, 204]`               |
| `http.header.content-type: !Contains "html"` | `http.header.content-type contains "html"` |
| `dns.answers: !Contains "10 mx.example.com"` | `"10 mx.example.com" in dns.answers`   |

A probe that previously listed several validators becomes one check per condition (a probe
fails as soon as any check does not match, exactly as it did with validators):

```yaml
# Before — per-field validators:
    validators:
      http.status: !OneOf [200, 204]
      http.header.content-type: !Contains "html"

# After — checks:
    checks:
      - http.status in [200, 204]
      - http.header.content-type contains "html"
```

Once you have moved everything across, you can drop the probe's `validators:` block
entirely. You can also fold related conditions into a single expression and reach for
operators that validators never offered — ranges, regular expressions, and relationships
between fields:

```yaml
    checks:
      # A range, a regex, and a relationship between fields.
      - http.status >= 200 && http.status < 400
      - http.header.content-type matches r"^application/json"
      - net.ip in dns.answers
```

::: tip Behavioural note
`filt-rs` compares strings **case-insensitively** by default, so `==`, `contains`,
`startswith`, and `endswith` ignore case where the equivalent validators compared exactly.
When you need an exact, case-sensitive comparison, use the `_cs` variants
(`contains_cs`, `startswith_cs`, `endswith_cs`) or an anchored regular expression with
`matches`.
:::
