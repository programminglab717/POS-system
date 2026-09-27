# ADR-0002: Event-sourced core with signed, hash-chained per-device logs

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Keel needs:
- offline multi-device operation with deterministic merging;
- tamper-evident records for fiscal regimes and loss prevention (France's inalterability conditions,
  hash chains in Portugal, Spain and Saudi Arabia, and the anti-"zapper" requirements behind them);
- exact audit trails for disputes;
- rebuildable projections;
- idempotent integration with processors and authorities.

Update-in-place databases make each of these hard, and D365's warning about duplicate transaction
IDs after offline periods illustrates the problem.

## Decision

1. All commercial facts are **immutable events** in one universal envelope ([domain model §17](../architecture/domain-model.md#17-events)):
   - a UUIDv7 ID and a per-device gapless sequence;
   - a hybrid logical clock;
   - a canonical CBOR payload with a versioned schema;
   - the previous-event hash and a device signature (a hardware-backed ECDSA P-256 key where
     available).
2. **Each device appends only to its own log.** Logs replicate by version vector (ADR-0006).
3. State is a **projection** computed by **total, deterministic fold functions**. No event is ever
   rejected or discarded at merge time; conflicts become visible states.
4. Commands go through one pipeline (authorize → validate → extensions → emit), identical on
   device, hub and cloud.
5. Side effects go through a **transactional outbox**, with idempotency keys derived from event IDs.
6. **Local storage is SQLite.** Events, projections and outbox are written in one transaction.
7. **Schema evolution is additive.** Events are never rewritten, and upcasters in the kernel handle
   old versions. CI checks compatibility against the full schema registry.
8. The cloud stores daily **Merkle roots** per location in write-once storage.

## Consequences

**Positive**
- Offline merging reduces to set union plus deterministic folding.
- Tamper evidence and fiscal inalterability come built in.
- Complete audit and "why" traces.
- Deterministic replay for support and debugging.
- Easy CDC to analytics and merchant warehouses.

**Negative**
- Storage growth, handled by retention windows, snapshots and cloud tiering to Iceberg.
- Schema governance discipline is mandatory.
- Erasure of personal data needs crypto-shredding (ADR-0011).
- Developers must learn event-sourcing patterns. Mitigated by the kernel owning the pattern:
  product code sends commands and reads projections.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| CRUD + change-data-capture | Doesn't merge offline edits, no tamper evidence, lossy audit |
| Generic JSON CRDT documents (Automerge/Yjs-style) for orders | Merges structure, not business semantics (money, invariants, escrow). Tombstone and metadata overhead. Harder to sign and fiscalize. |
| Blockchain / distributed ledger | Unnecessary consensus cost. Keel needs tamper *evidence*, not trustless consensus. |

## References
- [domain-model.md](../architecture/domain-model.md), [offline-and-sync.md](../architecture/offline-and-sync.md), [compliance.md](../architecture/compliance.md)
