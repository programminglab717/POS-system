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
| [0014](./0014-pricing-engine-v0.md) | Pricing engine v0: snapshot prices, five rounding points, allocated discounts, US sales tax | Proposed |

Template: *Status · Date · Context · Decision · Consequences · Alternatives considered · References*.
