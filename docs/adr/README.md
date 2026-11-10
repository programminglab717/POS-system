# Architecture Decision Records

Each ADR records one significant decision: its context, the decision, the consequences, and the
alternatives rejected. ADRs are immutable once accepted. A changed decision gets a new ADR that
supersedes the old one.

| ADR | Title | Status |
|---|---|---|
| [0001](./0001-local-first-three-tier-topology.md) | Local-first, three-tier topology (device, Store Hub, cloud) | Accepted |
| [0002](./0002-event-sourced-signed-event-log.md) | Event-sourced core with signed, hash-chained per-device logs | Accepted |
| [0003](./0003-rust-kernel-everywhere.md) | One Rust kernel on every device, hub and cloud node | Accepted |
| [0004](./0004-client-ui-stack.md) | Client UI stack: native shells (Kotlin/Compose, SwiftUI) + React web over the Rust kernel | Accepted |
| [0005](./0005-processor-agnostic-payments.md) | Processor-agnostic payments; card auth never transits Keel Cloud | Accepted |
| [0006](./0006-own-sync-protocol.md) | Build Keel's own domain-specific sync protocol | Accepted |
| [0007](./0007-cell-based-cloud-modular-monolith.md) | Cell-based multi-tenant cloud running a modular monolith | Accepted |
| [0008](./0008-wasm-extensions-run-offline.md) | WebAssembly Functions that run offline inside the kernel | Accepted |
| [0009](./0009-compliance-as-data-and-fiscal-adapters.md) | Compliance as signed rule packs, plus pluggable fiscal adapters | Accepted |
| [0010](./0010-hardware-abstraction-and-document-model.md) | Peripheral Service and KeelDoc document model | Accepted |
| [0011](./0011-pii-vault-and-crypto-shredding.md) | PII vault with per-person keys and crypto-shredding | Accepted |
| [0012](./0012-event-wire-format.md) | Event wire format: canonical CBOR, COSE_Sign1, and revocation by log position | Accepted |
| [0013](./0013-event-payloads-and-schema-evolution.md) | Event payloads: integer-keyed canonical maps, strict versions, and total folds | Accepted |
| [0014](./0014-pricing-engine-v0.md) | Pricing engine v0: snapshot prices, five rounding points, allocated discounts, US sales tax | Accepted |
| [0015](./0015-checks-and-payments.md) | Checks and payments: line shares, per-check pricing, closing snapshots, and the payment aggregate | Accepted |
| [0016](./0016-device-store.md) | The device store: SQLite through rusqlite, one transaction per write, crash-tested | Accepted |
| [0017](./0017-projections-and-outbox.md) | Projections and the outbox: per-stream projections recomputed in each write, and a transactional effect outbox | Accepted |
| [0018](./0018-encryption-at-rest-and-integrity-checks.md) | Encryption at rest and integrity checks: SQLCipher with a vendored OpenSSL, a raw key the platform protects, and checks on open and on request | Accepted |
| [0019](./0019-replication-and-deterministic-simulation.md) | Replication and deterministic simulation: anti-entropy over version vectors, folds in HLC order, and a seeded single-process simulator | Accepted |
| [0020](./0020-hub-sequencing.md) | Hub sequencing: signed sequencing records in the hub's log, confirmation by hash, and durability watermarks | Accepted |
| [0021](./0021-ownership-leases.md) | Ownership leases: an order's owning device in its own events, requests answered by the hub, and a manager's override when the hub can't be reached | Accepted |
| [0022](./0022-hub-election-and-failover.md) | Hub election and failover: hub terms claimed in the log, heartbeats and priorities, and a deposed hub's records fenced by its successor's claim | Accepted |
| [0023](./0023-register-shell.md) | The register shell: a device runtime the shell drives through generated bindings, a location profile to ring from, and the step in five slices | Accepted |
| [0024](./0024-receipts-and-printing.md) | Receipts and printing: KeelDoc v0, an ESC/POS renderer, a raw TCP driver, and print jobs in the outbox that are never sent twice by themselves | Accepted |
| [0025](./0025-bindings.md) | The bindings: `keel-ffi` with UniFFI 0.32, a register object that answers with whole views, and a Kotlin/JVM library tested on the host | Accepted |
| [0026](./0026-local-hub.md) | The local hub: a provisioned roster, replication in the runtime, WebSocket over mutual TLS in `keel-net`, a hub daemon and a provisioning tool | Accepted |
| [0027](./0027-android-app.md) | The Android app: AGP 9.5 and Gradle 9.8, the kernel built with cargo-ndk, a register module in Kotlin alone, keys wrapped in the Keystore, built in parts | Accepted |
| [0028](./0028-cloud-cell-v0.md) | The cloud cell v0: the cloud as each location's durable replica, catalogs published as chunked events, and a back office over the cell's API | Accepted |

Template: *Status · Date · Context · Decision · Consequences · Alternatives considered · References*.
