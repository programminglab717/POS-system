# ADR-0003: One Rust kernel on every device, hub and cloud node

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Money logic must behave identically everywhere. The code that computes prices, promotions, tax,
rounding, allocations, state folds, fiscal chains and sync has to run on:
- Android, iOS/iPadOS, Windows, macOS and Linux devices;
- the Store Hub appliance;
- cloud services;
- the browser (storefront cart totals).

A classic POS defect is divergent tax, rounding or discount logic across platforms, including the
online and offline price divergence noted in [R02](../research/02-retail-pos.md). A cloud total that
differs from the register total by a cent is a bug that can't be allowed.

Low-end Android all-in-ones (about 2 GB RAM, often without Google services) need a small, fast core.
The extension sandbox (ADR-0008) must run on iOS, where JIT is prohibited.

High-reliability local-first products have converged on a **Rust core with platform shells**: Ditto,
Automerge, Loro, Turso, and PowerSync's Rust SDK ([R04 §6](../research/04-technical-architecture.md)).

## Decision

1. The **Keel kernel** is a Rust workspace (`keel-*` crates; see
   [architecture README §5](../architecture/README.md#5-the-kernel)) containing:
   - value types, events, domain folds, pricing, promotions and tax, policy and rule packs;
   - the event store (SQLite), sync, payments orchestration, fiscal, peripherals and encoders;
   - the WASM extension host, a sandboxed JS runtime for UI extensions, and the deterministic
     simulator.
2. **Bindings**:
   - UniFFI to Kotlin (Android, JVM desktop) and Swift (Apple);
   - a C ABI where needed;
   - `wasm-bindgen` for the browser;
   - native linking in cloud services (also Rust).
3. **Coding rules**:
   - No floating point in money paths.
   - Canonical CBOR encoding.
   - No panics across the FFI boundary; every entry point returns typed errors.
   - No `unwrap()` in production paths, enforced by lints (see the Cloudflare November 2025 incident in
     [R04 §2](../research/04-technical-architecture.md)).
   - Deterministic simulation hooks for all I/O.
4. **Golden tests** (≥ 1,000 baskets and all jurisdiction vectors) run on every target triple in CI.
   Outputs must match byte for byte.

## Consequences

**Positive**
- A single implementation of everything that touches money.
- Memory safety and performance on cheap hardware.
- One codebase for device, hub and cloud.
- WASM for the browser and for sandboxed extensions.

**Negative**
- Rust expertise is required for the kernel team, and FFI adds complexity. Mitigated by keeping the
  FFI surface small and generated (UniFFI): product engineers write Kotlin, Swift or TypeScript
  against typed bindings.
- Compile times and cross-compilation CI cost. Mitigated with caching and a remote build service.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| TypeScript core running in JS engines everywhere | Weaker determinism guarantees (numeric pitfalls), heavier on low-end devices, harder to host a WASM sandbox on iOS; ties the core to JS runtimes |
| Kotlin Multiplatform core | Strong on Android and JVM, but weaker for the cloud (non-JVM), WASM and embedded hubs; less proven for sync engines |
| C++ core | Memory-safety risk in the money path |
| Logic duplicated per platform | The classic source of POS rounding, tax and discount bugs |

## References
- [architecture README](../architecture/README.md), [R04 §6, §10](../research/04-technical-architecture.md)
