# Clustering

Grey's clustering feature enables distributed health probing across multiple nodes, providing
scalability, redundancy, and the ability to probe from different network locations while
maintaining centralized monitoring through the web UI. Due to the way we replicate probe
information, it is possible to run probes on headless Grey nodes and consume their results
via the web UI on a single central instance (which may be configured with some, or none, of
the same probes).

## Overview

Grey clustering uses a gossip protocol for peer discovery and coordination, with all
communication encrypted using AES-256-GCM. When clustering is enabled, probe results
are automatically synchronized across the cluster, ensuring that all nodes eventually
converge on a common view of your platform health.

Internally, we keep track of aggregate probe health using [CRDTs], which enables us
to provide highly available health information while delivering eventual consistency.

[CRDTs]: https://en.wikipedia.org/wiki/Conflict-free_replicated_data_type)

## Quick Start

### 1. Generate a Shared Cluster Secret

All cluster members must share the same encryption key. You can generate the key using
one of the following methods, or simply run Grey with clustering enabled and an invalid
key to have one generated for you.

```bash
# Using OpenSSL
openssl rand -base64 32

# Using Python
python3 -c "import secrets, base64; print(base64.b64encode(secrets.token_bytes(32)).decode())"

# Using Node.js
require('crypto').randomBytes(32).toString('base64');

# Example output: /pL7XKDj1UrAGjNMv3t9jmb9leDOZT+64KkYE8k7UH8=
```

### 2. Configure Grey

Cluster configuration is the same for all Grey instances, however by convention we tend
to only enable the `ui` on primary nodes while workers tend to have this disabled. For
reliable operation, we recommend providing at least two initial peers - these will be used
to join the cluster, at which point further peers will be automatically discovered.

```yaml
state: ./state.redb

ui:
  enabled: true
  listen: 0.0.0.0:3000
  title: "Grey Cluster - Primary"

cluster:
  enabled: true
  listen: 0.0.0.0:8888
  peers:
    - 10.0.0.2:8888
    - 10.0.0.3:8888
  secret: /pL7XKDj1UrAGjNMv3t9jmb9leDOZT+64KkYE8k7UH8=

```


## Configuration Reference

### Basic Options

#### `enabled`
Enable or disable clustering for this node.

```yaml
cluster:
  enabled: true
```

#### listen
The local address and port to bind for cluster communication over UDP.

```yaml
cluster:
  listen: 0.0.0.0:8888  # Default
```

#### peers
Initial peer addresses for cluster discovery. These should be IP addresses which
are accessible from the current node (either over a private network, a VPN, or over
the public internet).

```yaml
cluster:
  peers:
    - 10.0.0.2:8888
    - 10.0.0.3:8888
```

#### secret
Base64-encoded 32-byte encryption key. All cluster members must use the same key.

```yaml
cluster:
  secret: /pL7XKDj1UrAGjNMv3t9jmb9leDOZT+64KkYE8k7UH8=
```

### Advanced Tuning

#### gossip_interval
How frequently nodes exchange gossip messages. Lower values improve the time
taken for the cluster to reach consensus on the state of a given probe, but
increase network usage.

```yaml
cluster:
  gossip_interval: 30s  # Default
```

#### gossip_factor
Number of random peers to gossip with per interval. Higher values how quickly
the cluster reaches consensus, but also increase network usage.

```yaml
cluster:
  gossip_factor: 2  # Default
```

For clusters with N nodes, optimal gossip_factor is typically `log₂(N) + 1`:
- 2-4 nodes: gossip_factor = 2
- 5-8 nodes: gossip_factor = 3  
- 9-16 nodes: gossip_factor = 4

#### gc_interval
How frequently to run the garbage collector to remove stale peers and expired probes.

```yaml
cluster:
  gc_interval: 300s  # Default (5 minutes)
```

#### gc_probe_expiry
How long to retain information about a probe before it is considered stale and removed
in the next garbage collection cycle.

Every time you start Grey, it starts reporting probe state under a new node identifier,
so it is normal and expected that application restarts will result in probe state for
old instances not being updated and eventually needing to be removed. Once removed,
the aggregated probe metrics will be adjusted to account for the loss of this data.

```yaml
cluster:
  gc_probe_expiry: 7d  # Default (7 days)
```

#### gc_peer_expiry
How long to attempt to contact a known peer after it was last seen before considering
it to be offline and removing it from the local member list.

We recommend you keep this value relatively low to avoid sending unnecessary broadcasts
to inactive peers, however having it set too short can result negatively impact cluster stability
under load.

```yaml
cluster:
  gc_peer_expiry: 30m  # Default (30 minutes)