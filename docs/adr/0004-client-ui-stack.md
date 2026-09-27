# ADR-0004: Client UI stack — native shells over the Rust kernel

- **Status:** Accepted (supersedes an earlier draft that proposed React Native for device apps)
- **Date:** 2026-09-27

## Context

Device apps must:
- respond in under 50 ms on a 2 GB Android all-in-one;
- integrate **native-first** payment, SoftPOS, printer, scanner and MDM SDKs (Stripe Terminal, Adyen,
  Tap to Pay, StarIO10, Epson ePOS, Sunmi, DataWedge, Android lock-task, iOS Autonomous Single App
  Mode);
- run the **Store Hub role**: a background service that listens on the LAN;
- run for ten or more years.

Because all business logic lives in the Rust kernel (ADR-0003), the UI layer is thin: it renders
projections and sends commands.

Evidence from research ([R04 §1, §6](../research/04-technical-architecture.md)):
- **Shopify**, the flagship React Native POS, reported regressions of up to 20% on complex screens
  after moving to RN's New Architecture. In September 2026 it reportedly announced a rebuild of its
  apps, **including POS, in native Swift and Kotlin**. It cited AI-assisted development making dual
  native codebases affordable, with large startup, size and crash-rate improvements in its first
  rebuilt app. *(Reported via Shopify Engineering and InfoQ; not independently verified in this
  session.)*
- **PWAs** can't be LAN peers or hubs (browsers can't listen on sockets or do mDNS) and have **no
  device APIs on iOS**.

## Decision

| Surface | Technology | Priority |
|---|---|---|
| **Android** devices: registers, handhelds, KDS, kiosk, customer display, Android payment terminals, hub role (foreground service) | **Kotlin + Jetpack Compose**, structured as **Kotlin Multiplatform** modules | **First** (MVP) |
| **Apple** devices: iPad registers, KDS and kiosk; iPhone handhelds with Tap to Pay | **Swift + SwiftUI** | Second (v1) |
| **Windows / Linux** lanes and kiosks (grocery, enterprise retail) | **Compose Multiplatform desktop**, reusing the Android UI modules; Tauri 2 + web UI is the fallback option | v3 |
| **Back office, storefront, QR order and pay, booking pages** | **React + TypeScript** (web) | MVP |
| **Hub-served display surfaces** (order-status boards, menu boards, secondary customer displays on any screen) | React web apps served by the Store Hub over its trusted local hostname | v1–v2 |
| **Store Hub appliance** | Headless Rust daemon; admin UI is a hub-served web app | v1 |

All shells call the kernel through **generated UniFFI bindings**, so view models consume identical
projection types on every platform.

**Keeping two native UIs consistent:**
- Shared **design tokens** and a component specification. Each platform implements the same component
  inventory.
- **Screen contracts** (view-model interfaces) generated from the kernel.
- Cross-platform **scenario tests** (the same scripted service flows run on Android and iOS against
  the simulator).
- UI extensions rendered from one remote-component schema by both hosts.
- A feature is "done" only when it has shipped on every platform in its tier.

## Consequences

**Positive**
- Best performance on cheap hardware.
- Direct access to every native SDK.
- The hub role runs natively on Android.
- Native accessibility (TalkBack and VoiceOver).
- No dependency on a cross-platform framework's roadmap.

**Negative**
- Two native UI codebases, plus web.
- No over-the-air JS updates. Menus, rule packs, configuration and extensions are data and do update
  over the air. App binaries ship through MDM or app stores in staged rings, which POS fleets need
  anyway.

**Mitigations**
- A thin UI over the kernel.
- The Android-first sequence.
- AI-assisted parity work.
- KMP structure enables desktop reuse, and even an iOS Compose fallback if SwiftUI parity costs prove
  too high.

## Alternatives considered

| Alternative | Why not chosen as primary |
|---|---|
| React Native (+ React web, Tauri) | One TypeScript UI codebase and OTA updates. Rejected because of weaker performance headroom on low-end devices, a bridge to every native SDK, a less natural hub role, and a declining flagship precedent in POS. |
| Flutter + Rust core | Strong single-codebase option; platform channels for every SDK; smaller talent pool; framework roadmap risk. Kept as a credible fallback. |
| Compose Multiplatform everywhere, including iOS | Attractive single codebase. iOS rendering and accessibility are less mature than SwiftUI for the iPad-heavy SMB market. Revisit in 2027. |
| PWA-first | No LAN role and no iOS device APIs. Used only for web surfaces. |

## References
- [architecture README §4, §10, §11](../architecture/README.md), [hardware.md](../architecture/hardware.md), [R04 §5.3, §6](../research/04-technical-architecture.md)
