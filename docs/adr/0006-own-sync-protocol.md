# ADR-0006: Build Keel's own domain-specific sync protocol

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

The sync layer is on the sell path and carries money. Requirements:
- multi-device operation in the store with **no internet**;
- a store-level authority for ordering and invariants: check ownership, no double payment, gapless
  per-register fiscal numbering;
- islands that keep selling;
- signed, hash-chained events;
- bounded escrow for scarce resources;
- mixed-version fleets;
- browser clients served by the hub;
- open licensing.

Evaluation of existing technology ([R04 §3](../research/04-technical-architecture.md)):

| Option | Blocking issue for POS |
|---|---|
| Zero (Rocicorp) | No offline writes; writes are rejected while disconnected |
| ElectricSQL | Read-path sync from Postgres; no LAN mode |
| PowerSync | Excellent device↔cloud; no hub or peer mode in the store; service under FSL licence |
| Ditto | Best off-the-shelf LAN mesh; **proprietary**, per-device pricing, CRDT semantics can't enforce POS invariants |
| Couchbase Lite / Sync Gateway | BSL licence and enterprise pricing for peer-to-peer |
| Automerge / Yjs / Loro | Document CRDTs, not transaction ledgers |
| cr-sqlite, Turso | Pre-1.0 maturity for money data |
| MongoDB Realm / Atlas Device Sync | **Deprecated; end-of-life 30 Sep 2025.** The cautionary tale. |

The broader lesson: the sync engine's semantics leak into application code, and vendors deprecate or
relicense (Realm, CockroachDB, Redpanda; NATS narrowly stayed Apache-2.0).

## Decision

Keel builds its own **hub-sequenced, event-log replication protocol** in the Rust kernel
([offline-and-sync.md](../architecture/offline-and-sync.md)):

1. **Per-device signed, hash-chained logs**, replicated by version vectors over WebSocket/TLS with
   mTLS. The protocol is topology-agnostic: device↔hub, hub↔standby, hub↔cloud, device↔cloud.
2. **The Store Hub sequences** events into a canonical store order (`store_seq` per epoch). Confirmed
   and provisional states are explicit.
3. **Events are facts.** They are never rejected at merge. Invalid-in-context events are flagged and
   compensated. This adapts Replicache-style server reconciliation for signed fiscal records, because
   an authority can't "rebase away" a card that was charged.
4. **Check ownership leases**, following the Oracle Simphony CAPS pattern: structural and money
   operations need ownership; commutative additions don't. **Island mode** allows owned-order
   operations only, with manager override.
5. **Hot-standby hub with epoch fencing**; failover in under 5 s.
6. **Consistency classes** A–E per aggregate, with escrow for class C and D resources.
7. **Deterministic simulation testing** is a release gate for any change to the protocol.
8. **Vendor-risk policy**: anything bought for the sell path must sit behind Keel's own interface,
   use self-hostable licence terms, and have an exit plan.
   - If time-to-market ever demands it, a commercial mesh could be used only behind the sync
     interface, with escrow.
   - Device↔cloud read models for back-office web apps may use off-the-shelf engines, because they
     aren't on the sell path.

## Consequences

**Positive**
- The POS invariants are enforced by design.
- No vendor can deprecate Keel's most critical layer.
- The protocol is exactly as complex as the domain needs, and no more.

**Negative**
- Keel owns a hard distributed-systems component. Mitigated by:
  - the narrow semantics (append-only logs, version vectors, a single sequencer per store);
  - deterministic simulation with fault injection;
  - staged rollouts with N-2 wire compatibility.

## Alternatives considered

- **Pure peer-to-peer mesh with no sequencer.** Rejected for the core: no natural authority for
  ownership and sequencing. A future BLE / Wi-Fi Aware device mesh is kept as a v3 research item for
  venues.
- **NATS JetStream leaf nodes as the hub↔cloud transport.** Attractive for appliance hubs, but hubs
  also run on Android and iPad, so the universal protocol is Keel's own. JetStream remains the
  in-cell bus (ADR-0007) and could be an optimization for appliance hubs later.

## References
- [offline-and-sync.md](../architecture/offline-and-sync.md), [R04 §3, §10](../research/04-technical-architecture.md), [R01 §2.1](../research/01-restaurant-pos.md)
