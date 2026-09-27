# ADR-0008: WebAssembly Functions that run offline inside the kernel

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Merchants and partners need to customize pricing, promotions, validation, routing, loyalty and
returns. Webhook-based customization fails offline and adds latency to the sales path. Cloud-executed
functions (as in Shopify Functions) solve safety and performance but still don't run in a store that
has lost its internet. Custom logic must also produce **identical results** on every replica, or
order totals diverge between devices and the cloud.

## Decision

1. Logic extensions ("Functions") are **WebAssembly modules** executed inside the kernel at defined
   hook points ([platform §6](../architecture/platform-and-extensibility.md#6-functions--logic-extensions-wasm-offline-deterministic)).
2. **Runtime**: `wasmi` (an interpreter) on platforms that forbid JIT (iOS); `wasmtime` elsewhere.
   Behavior is identical across both, enforced by conformance tests.
3. **Determinism**: pure functions over declared inputs (CBOR/JSON with published schemas). No clock,
   randomness, network or filesystem; the kernel supplies everything as input.
4. **Limits**: fuel metering (an instruction budget), a memory cap and a module-size cap. Each hook
   declares a fail-safe policy (skip, or block with manager override).
5. **Distribution**: modules are signed and versioned, reviewed for marketplace apps, rolled out in
   rings per location, and can be disabled with a kill switch.
6. **UI extensions** are separate: a sandboxed JS runtime emitting a host-rendered component tree
   (platform §7). Functions stay logic-only.

## Consequences

**Positive**
- Custom logic works offline and converges across replicas.
- A sandbox with strong isolation, so an extension can't crash or slow the register beyond its budget.
- Any language that compiles to WASM can be used.

**Negative**
- Interpreted WASM on iOS is slower. Budgets are sized for the interpreter, and hot paths such as the
  promotions optimizer stay native in the kernel.
- Pure-function constraints limit some use cases (e.g. real-time external lookups). These use online-
  only UI extensions or automations instead, with declared offline behavior.
- Keel must maintain stable hook schemas as public API with versioning.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Webhooks in the sales path | Fail offline; add latency; make totals depend on external availability |
| Embedded JavaScript (QuickJS/Hermes) for logic | Weaker isolation and determinism guarantees; JIT restrictions on iOS; still an option for UI extensions |
| Cloud-only functions | Don't run offline |
| A merchant-side scripting DSL only | Too limited for partners; kept for rule packs (ADR-0009) where declarative logic suffices |

## References
- [platform-and-extensibility.md](../architecture/platform-and-extensibility.md), [R04 §7](../research/04-technical-architecture.md)
