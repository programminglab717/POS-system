# ADR-0001: Local-first, three-tier topology (device, Store Hub, cloud)

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Availability at the point of sale is Keel's top quality attribute. The research shows the dominant
failure pattern in cloud POS:
- "Offline mode" keeps card payments but stops operations: login, loyalty, gift cards, device-to-device
  sync, close-out and reports.
- Outages that are online-but-broken (DNS), regional (cloud provider) or data-center-specific
  (ransomware) stop thousands of merchants.

Systems with an on-premises coordinator (Simphony CAPS, Aloha's server, TouchBistro's Mac server)
degrade better, but impose a mandatory server, IT burden and a single point of failure. Toast's
local-sync hub requires a specific wired device model on the same subnet.

## Decision

1. **Every device is an autonomous replica.** Each runs the full kernel with a local event store and
   can sell alone indefinitely.
2. **The Store Hub is a role, not a box.** It is hosted on a Keel Hub appliance or on any eligible
   device, and elected automatically with epoch fencing. It is the store's authority:
   - it **sequences** events into a canonical order and grants **check-ownership leases**;
   - it holds number blocks, escrow and the stored-value snapshot;
   - it runs peripherals, the local API and the cloud gateway.

   A **hot standby** takes over in under 5 s. A device that reaches neither hub nor cloud keeps
   selling in **island mode** (owned orders only; manager override otherwise).
3. **The cloud is authoritative for history, reference-data authoring, cross-store state and
   integrations.** It is never on the in-store critical path.
4. **Card authorization goes directly device → terminal → processor.** It never goes through Keel
   Cloud.
5. Each aggregate type declares a **consistency class** (A–E) that determines its coordination
   mechanism (see the domain model §18 and offline-and-sync §6).

## Consequences

**Positive**
- Loss of the internet, Keel Cloud, the hub or a device degrades bounded capabilities only.
- Local latency is under 50 ms regardless of the network.
- Stores survive vendor-cloud ransomware and regional outages.
- The same model scales from a single-device food truck (hub embedded) to stadiums (hub appliances,
  revenue-center partitioning).

**Negative**
- Harder engineering: distributed state, merge semantics, escrow, mixed versions. Mitigated by
  deterministic simulation testing (offline-and-sync §12).
- Devices need more storage and CPU than thin clients. Acceptable on modern hardware; retention
  windows bound storage.
- Some cross-store operations (gift card balances, cross-location purchase limits) can only be
  *bounded*, not guaranteed, while offline. Made explicit through risk envelopes.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Cloud-first with an offline fallback (Toast, Square, Clover model) | The exact failure mode we are designing out |
| Mandatory on-prem server (Simphony, Aloha model) | IT burden, cost, SPOF, hostile to SMB self-serve |
| Pure peer-to-peer mesh, no coordinator | No natural authority for check ownership, sequencing and escrow. A BLE / Wi-Fi Aware device mesh stays a v3 research item for venues. |
| Third-party mesh sync product | Vendor dependency in the most critical layer (see ADR-0006) |

## References
- [offline-and-sync.md](../architecture/offline-and-sync.md), [R01 §2.1](../research/01-restaurant-pos.md), [R02 §2.12](../research/02-retail-pos.md), [R04](../research/04-technical-architecture.md)
