# ADR-0023: The register shell: a device runtime the shell drives through generated bindings, a location profile to ring from, and the step in five slices

- **Status:** Proposed
- **Date:** 2026-10-02

## Context

Step 7 of Phase 0 is the Android register shell: "ring an order with modifiers, pay cash, print a
receipt (ESC/POS over TCP), running on a reference all-in-one against a local hub"
([roadmap §8](../roadmap.md#8-first-engineering-milestones-the-next-build-steps)). Phase 0's
exit criteria ask more of it: every tap answered in under 50 ms at the 99th percentile on a
2 GB Android device with a 100,000-SKU catalog, and the golden baskets matching byte for byte on
Android ([roadmap §2](../roadmap.md#2-phase-0--foundations-internal)).

The design fixes a great deal already:

- **One kernel, thin shells** ([ADR-0003](./0003-rust-kernel-everywhere.md),
  [ADR-0004](./0004-client-ui-stack.md)). Business logic lives in the Rust kernel. Shells
  render what it derives and send it commands, through generated UniFFI bindings: Kotlin and
  Jetpack Compose on Android, structured as Kotlin Multiplatform modules so that desktop lanes
  can reuse them. No panic crosses the boundary, and every entry point returns typed errors.
- **The command pipeline** is the same on device, hub and cloud: authorize, load, validate,
  decide events, then append them with their projections and effects in one SQLite transaction
  ([architecture §5](../architecture/README.md#5-the-kernel)).
- **Printing** goes through the Peripheral Service: drivers in the kernel, logical routes with
  fallbacks, and KeelDoc, one device-independent document model that kernel renderers compile
  to ESC/POS and other languages ([ADR-0010](./0010-hardware-abstraction-and-document-model.md),
  [hardware.md §4, §5](../architecture/hardware.md#4-peripheral-service)). A missing printer
  never blocks a sale.
- **The LAN**: WebSocket over TLS 1.3 with CBOR frames, mutual TLS with device certificates
  from the Keel Device CA, discovery by mDNS (`_keel-hub._tcp`), a cloud-signed roster or a
  manual address, and one TCP port
  ([offline-and-sync §3.3](../architecture/offline-and-sync.md#33-transport-discovery-and-security)).
  The hub role runs on Android as a foreground service.
- **Keys**: the store's key is wrapped by the platform keystore
  ([ADR-0018](./0018-encryption-at-rest-and-integrity-checks.md)); a device's signing key lives
  in secure hardware where it can, or is a software Ed25519 key
  ([ADR-0012](./0012-event-wire-format.md), [security §2.1](../architecture/security.md#21-devices)).
- **The catalog** is reference data the cloud publishes as immutable, content-addressed
  versions, and every line records the catalog version it was priced against
  ([domain-model §5](../architecture/domain-model.md#5-catalog),
  [offline-and-sync §6.5](../architecture/offline-and-sync.md#65-class-e--reference-data-cloud-authored)).

What steps 2 to 6 built shapes the rest:

- The kernel decides: orders, checks and payments, with checkout across them, cash captured with
  what was tendered and its rounding ([ADR-0015](./0015-checks-and-payments.md)); pricing with
  its trace ([ADR-0014](./0014-pricing-engine-v0.md)); the store, which appends, projects and
  enqueues effects in one crash-tested write, encrypted ([ADR-0016](./0016-device-store.md) to
  [ADR-0018](./0018-encryption-at-rest-and-integrity-checks.md)); and replication, the hub's
  role and its election, sans I/O ([ADR-0019](./0019-replication-and-deterministic-simulation.md)
  to [ADR-0022](./0022-hub-election-and-failover.md)).
- Nothing yet turns what a cashier does into those commands. The simulator's workload does it
  for its own sales, a few lines at a time, with names and prices it makes up.
- Lines carry a snapshot of what they sell (the variant, its name, tax category and price, and
  the chosen modifiers with theirs) and a `CatalogVersion`, a content hash; a closed check
  records a `RulesVersion`. No catalog or rules exist to take them from.
- The kernel reads no clock and no random number generator: it takes a `Clock` and an
  `Entropy`. `keel-types` has the system's, behind its `os` feature.
- The workspace forbids `unsafe` code, and every amount the kernel shows is a `Money`, which it
  has never formatted for a person to read.

And the step has constraints of its own:

- **No cloud yet.** The cell (step 8) brings the Device CA, enrollment and publishing. Until
  then, something else must give devices their location's catalog and each other's keys.
- **The build environment.** The environment this project is built in reaches Maven Central
  and the Gradle plugin portal, but not Google's repositories (`dl.google.com`, where
  `maven.google.com` sends every download): no Android SDK, NDK, Android Gradle Plugin, AndroidX
  or Compose. GitHub's CI runners have them.

## Decision

1. **Four layers.** A shell renders and asks; it computes nothing that touches money, and formats
   no amount:

   ```text
   Shell (Kotlin, Compose)          screens, navigation, the platform's keystore, discovery,
     │                                permissions and lifecycle
     └─ Bindings (keel-ffi)         generated by UniFFI; the one crate with unsafe code
          └─ Device runtime         intents and views over the store; drivers for printers and
             (keel-runtime)           the network beside it
               └─ Kernel crates     types, events, domain, pricing, store, sync
   ```

   The runtime is where a cashier's taps become the kernel's commands. It is ordinary Rust, under
   the workspace's lints, and every platform's shell, the hub daemon and the simulator use the
   same one.

2. **The device runtime, `keel-runtime`.** A platform crate (it holds a `keel-store` store) that
   turns **intents** into commands, and the store into **views**:
   - `Runtime::open` takes the store's path and key, the device's identity (its identifier, its
     location's and its signer), the location profile (decision 3), a `Clock`, and the `Entropy`
     the store and the runtime's own identifiers draw on. The platform supplies them: `keel-ffi`
     passes `keel-types`' system clock and the operating system's randomness; tests and the
     simulator pass their own.
   - **A signed-in team member** acts: every event records one. `sign_in` takes a member the
     profile lists; until one signs in, intents are refused. v0 asks for no PIN: staff sign-in
     with PINs ([security §2.2](../architecture/security.md#22-staff-on-devices)) comes later.
   - **Intents**, each one write, so that each happens entirely or not at all:
     - `start_order(mode)`: an order at the register (`Pos`), in the profile's currency, owned by
       the device and its team member;
     - `add_item(order, variant, choices, quantity)`: a line, its item and modifiers rung from
       the catalog (decision 4);
     - `change_line`, `remove_line` and `abandon`;
     - `pay_cash(order, tendered)`: the payment initiated and captured with what was tendered and
       any cash rounding, then, when it covers the check, the check closed with its snapshot and
       the order closed. One write, so a crash never leaves cash taken and the order open. It
       answers with the change due.
   - **Views**: an order's ticket (its lines with their modifiers, quantities and amounts, its
     taxes and totals, what it owes, and the cash due after rounding), the open orders, the
     menu's pages and an item's modifier groups, every amount with its display text
     (decision 5). A view is whole: the shell replaces what it shows with it.
   - Each intent loads what it decides on from the store, decides with `keel-domain`'s commands
     and checkout, and appends the events it decides, correlated as one action, under the
     business date the profile's policy gives the clock's time. Refusals are typed: the
     domain's, the catalog's, a tender short of what is due, an unknown order.
   - It does no I/O but the store's. Printing (slice 2) and the network (slice 5) are drivers
     beside it, which take jobs and frames from it and give it their outcomes, as `keel-sync`'s
     replicator does, so that the simulator can drive them too.
   - One writer: a runtime is used from one thread at a time, which the bindings ensure.

3. **Reference data v0: the location profile.** Everything a register needs to ring at a
   location, in one document:
   - the location: its identifier, name and the address lines a receipt shows; its currency;
     its business-day policy (time zone and cutoff); and its cash rounding rule, if any;
   - its pricing rules, `keel-pricing`'s taxes and rounding modes;
   - its catalog (decision 4) and menu: pages of buttons, each a variant;
   - its team members, each an identifier and a name.

   It is canonical CBOR, as events are ([ADR-0012](./0012-event-wire-format.md)), decoded
   strictly, with a format version. The catalog's encoding hashes, with SHA-256, to the
   `CatalogVersion` its lines record, and the pricing rules' to the `RulesVersion` its checks'
   snapshots record. A profile is checked whole before it is used: identifiers unique, every
   reference resolved, modifier groups nested at most four deep and never within themselves,
   their bounds coherent, every price in the profile's currency and none negative, and every
   count within a limit (200,000 items, for one).

   Until the cloud publishes reference data (step 8, class E), a profile is a file provisioned
   with the device, and a new one takes a restart. Slice 1 builds the codec, the checks and a
   demo profile, a café's, which the tests and the shell use.

4. **Ringing an item.** The catalog v0 has items, each with a tax category, one or more variants
   (the sellable units, each with its price and, when an item has several, a name such as
   "Large"), and the modifier groups offered for it. A group has bounds on its applications
   (`min` and `max`), a number of free ones (`free`), whether a modifier may be applied more than
   once (`repeat`), and its modifiers. A modifier has a name, a prefix (`keel-domain`'s
   `Prefix`), a price of zero or more, and groups of its own, chosen under it, such as a side's
   dressing.

   A cashier's choices for a line are modifiers, each with a quantity and choices of its own.
   They ring when:
   - each modifier is in a group offered where it is chosen, for the item or under the modifier
     it is chosen under, and is chosen once at its level;
   - quantities above one are in groups that repeat;
   - each group offered has between `min` and `max` applications, its modifiers' quantities
     added up, so a group with a `min` of 1 must be chosen.

   The line's snapshot is the variant's price and name ("Latte (Large)"), the item's tax
   category and the catalog's version; its modifiers are those chosen, with their prefixes and
   prices, in the catalog's order. A group's free applications go to its lowest-priced ones,
   ties to the modifier listed first, so the same choices always cost the same, whatever order
   they were tapped in; a modifier whose applications are part charged and part free appears
   twice, charged first. Placement (halves of a pizza), per-variant modifier prices and defaults
   that ring themselves come later.

5. **Display text: `Locale` v0.** Screens and receipts show amounts the same way, so the kernel
   formats them, not each platform's libraries, whose symbols and spaces differ from each other
   and from a printer's. `keel-types` gains `Locale`, by BCP 47 tag, with v0 for `en-US` and
   `es-US`, following CLDR 48's data for them, which a snapshot holds as the ISO 4217 table is
   held: the currency's symbol in the locale or else its code, a no-break space after a symbol
   ending in a letter (`EUR 12.34` in `es-US`), grouping by thousands, a minus sign before the
   symbol, and every minor unit the amount has. Every amount in a view carries its text. CLDR's
   data for other locales comes with the languages the product adds.

6. **Receipts and printing (slice 2)** follow ADR-0010 from the start, small:
   - **KeelDoc v0**, in a portable crate, `keel-doc`: lines of text with alignment, emphasis and
     double width or height; rows of a left and a right column ("Latte ... $4.50"), the left
     wrapping; separators; feeds; a cut; a drawer kick. A receipt v0 is built from a closed
     check's snapshot, its payments and the profile, and a reprint says "REPRINT".
   - **An ESC/POS renderer** in the subset Epson, Star (in its ESC/POS emulation, which is not
     its default), Bixolon, Citizen and generic printers share: Windows-1252 (`ESC t 16`), which
     has the euro sign and every Spanish letter, with the international set fixed to the US and
     Kanji mode cancelled; text outside the code page transliterated, then `?`, and raster later;
     control characters stripped from every text, so that no name can kick a drawer. Columns per
     line come from a printer profile: 48 at 80 mm and 203 dpi, 42 at 180 dpi, 32 at 58 mm.
   - **Effects**: paying cash enqueues the receipt and a drawer kick, in the write that closes the
     check (ADR-0017's outbox), so neither is lost by a crash.
   - **The Peripheral Service's first driver**: raw TCP to port 9100, one connection per job.
     It asks the printer's status (`DLE EOT`) before a job and doesn't send it to a printer that
     is offline, open or out of paper; after it, it waits for the printer's word that the job
     printed where the printer gives one (Epson's `GS ( H`) or was processed (`GS r`). A job
     retries by itself only if none of it was sent. Once any of it was, a job that fails, or is
     found running after a restart, is **unconfirmed**: it is never sent again by itself, the
     cashier is asked, and a reprint says so. This state is new to the Peripheral Service's job
     states ([hardware.md §4](../architecture/hardware.md#4-peripheral-service)).
   - Tested against bytes pinned per command, an ESC/POS interpreter in the tests that turns the
     bytes back into lines of text, and a scripted fake printer: a TCP server that records what
     it is sent and answers status as each test needs, refusing, resetting mid-job or going
     silent.

7. **Bindings (slice 3): `keel-ffi` with UniFFI.** UniFFI 0.32, pinned exactly, with its
   procedural macros; Kotlin bindings generated from the compiled library ("library mode") by a
   small binary in the workspace on the same version, and not committed:
   - one call per thing a cashier does, answered with the whole view, rather than a call per
     field: a call through JNA costs microseconds, tens of them on a till, so a tap makes few;
   - the runtime as an object, its views as records and enumerations, its refusals as typed
     errors, which reach Kotlin as exceptions; signing that secure hardware does, later, as a
     foreign trait the shell implements;
   - Kotlin calls it through JNA (5.19.1 or later, for 16 KB pages and older Android), from one
     background thread, so the runtime has one writer and the UI thread never waits on a write;
     the library is loaded at start, off the UI thread, since the first call costs tens of
     milliseconds;
   - `keel-ffi` keeps every workspace lint, `unsafe_code` forbidden included: the code UniFFI's
     macros generate isn't linted as the crate's own (so a test with 0.32.2 found), and the crate
     writes none of its own; the kernel crates stay free of UniFFI, and portable;
   - the platform edge, the one place that reads the system clock and the operating system's
     randomness, for the runtime;
   - the generated Kotlin compiles for Android and the desktop JVM alike; a Kotlin/JVM library
     and its tests run against the kernel built for the host, here and in CI, with no Android SDK;
   - UniFFI is licensed MPL-2.0, a copyleft that applies file by file, which the dependency
     policy's permissive list doesn't name. Used unmodified, as a library, it obliges nothing of
     Keel's own files; the policy gains that exception with it.

8. **The Android app (slice 4)** in `apps/android`, built with the Android Gradle Plugin 9.3,
   Gradle 9.7 and Kotlin 2.4, the newest versions tested together, and Compose:
   - Modules: the kernel's native libraries and generated bindings, in an Android library; the
     register's screens and view models, which know the kernel only through an interface, so
     their tests run on the JVM against a fake; and the app. Modules without Android code are
     Kotlin Multiplatform, for desktop lanes later.
   - `minSdk` 28 (Android 9), which every current all-in-one we found meets, and which brings
     StrongBox's APIs; `targetSdk` 36, as Google Play asks since August 2026.
   - The kernel built with NDK r30 through `cargo-ndk`, for `arm64-v8a`, and `x86_64` for
     emulators; its libraries aligned to 16 KB pages, which CI checks.
   - Keys: the store's key, 32 random bytes, wrapped by an AES-256-GCM key in the Android
     Keystore (StrongBox where the device has one, else its TEE), never bound to a lock screen,
     which kiosks don't have, and unwrapped once after wrapping, to catch hardware that wraps
     wrongly; the wrapped key and the store kept out of backups. The device's signing key in v0
     is a software key, wrapped the same way. Keys in secure hardware come with enrollment
     (step 8), once their signing time is measured: a StrongBox signature is reported to take
     about 150 ms, too slow for one per event.
   - The register: sign in, the menu's pages, the modifier sheet for an item's groups (the
     required first), the ticket, cash tender with change, the receipt printed and the drawer
     opened, today's orders and reprints.
   - A performance harness, tap to frame at the 99th percentile, on the reference device with a
     100,000-SKU catalog; and the kernel's golden baskets run on Android.
   - CI: a job that builds the kernel and the app, runs the JVM tests, and runs instrumented
     tests on an emulator (API 30, 2 GB, like the reference device): ring an order with
     modifiers, pay cash, and receive the receipt at a fake printer on the host.
   - Printers built into all-in-ones (Sunmi, iMin, PAX) are reached through their makers'
     services later, each an adapter behind the Peripheral Service; the reference device's
     receipts go to a network printer in this step.

9. **The local hub (slice 5).** The transport is Rust, written once for Android, Apple, desktops
   and the hub daemon, and tested with the protocol it carries; Kotlin keeps what only the
   platform can do:
   - `keel-net`: WebSocket over TLS 1.3 with mutual TLS, carrying `keel-sync`'s frames on one
     port; the hub listens and devices connect. On `tokio`, with `rustls` (one crypto provider
     for everything) and `tokio-tungstenite`. Certificates are pinned to the location's CA, and
     the hub's carries a stable name, so a new address doesn't break it. A key the Keystore holds
     signs the handshake through the shell.
   - The hub role, hosted by the same runtime: in the app as a foreground service of type
     `connectedDevice`, and in a Linux daemon for development and, later, the appliance.
   - Discovery: the provisioned address first; then mDNS, through Android's `NsdManager`, whose
     service picker is the one way to find the hub without the local-network permission that
     Android 17 enforces for apps targeting it.
   - Until the Device CA (step 8), a provisioning tool stands in for it: it makes a location's
     profile, enrolls devices (their keys into the location's registry) and issues their
     certificates from a development CA. Enrollment replaces it, with the same artifacts.

10. **Step 7 comes in five slices, each ending with a review**, each designed in detail in an ADR
    of its own when it begins, as step 6's were:
    1. the device runtime and the location profile: decisions 2 to 5;
    2. receipts and printing: decision 6;
    3. the bindings: decision 7;
    4. the Android app: decision 8;
    5. the local hub: decision 9.

    The step is done when a reference all-in-one rings an order with modifiers, takes cash and
    prints its receipt against a local hub, and the performance harness has run on it. The
    reference device is the user's to choose and buy; a 2 GB all-in-one with Android 11 and a
    built-in printer, such as iMin's Swan 1, fits.

11. **Verification**, for slice 1:
    - known answers: the demo café's items rung with modifiers, free applications among them,
      and every refusal; every rule a profile is checked against; the demo profile pinned byte
      for byte, with its versions, against the same profile built independently with Python's
      `cbor2`; amounts in both locales, and every currency's symbol against CLDR's snapshot;
      intents' views, totals and tax; cash paid exactly, with change and with cash rounding;
    - property tests: ringing, against a model written separately, over random catalogs and
      choices aimed at each group's bounds and at ties in price; profiles round-tripping their
      encoding; amounts and quantities against a model of the formatting; and the runtime,
      against a model of a register, over random intents, its views agreeing with the model's
      lines and with pricing of the model's basket, unchanged by reopening the store, and each
      intent whole or absent when a write is interrupted at any fault point;
    - formatting cross-checked against ICU (CLDR 48) for every currency in both locales;
    - planted bugs in each new list, caught by the property tests where they can reach them;
    - the intents' latency on the host, as a baseline for slice 4's harness.

12. **The build environment.** Slices 1 to 3 need nothing from Google's repositories. Slice 4
    does: the Android SDK and NDK, the Android Gradle Plugin, AndroidX and Compose. Unless this
    environment's network policy lets it reach `dl.google.com`, the Android build is verified in
    CI alone, each round a CI run.

## Consequences

**Positive**
- Shells stay thin. What ringing, paying and showing an amount mean is written once, in the
  runtime, for Android now and Apple, desktop lanes and the hub daemon later.
- Each thing a cashier does is one write: a crash leaves it done or not done, never half.
- The runtime is tested where the rest of the kernel is: on the host, with property tests, and
  later in the simulator, whose devices can run it in place of their own workload.
- Every line and check records the catalog and rules it was priced with, as the design asks.
- The kernel's part of the step is built and verified before any Android code, in an
  environment that can't build Android.

**Negative**
- A new layer, whose intents and views every shell depends on, and whose changes the bindings
  carry to each.
- Stand-ins until their real pieces land, each recorded as a gap: the profile file for
  published reference data, sign-in without a PIN, a software signing key, and a development CA.
- A catalog held in memory may cost too much at 100,000 SKUs; slice 4 measures it, and the store
  with full-text search is the fallback.
- `Locale` v0 knows two locales.
- Bindings from a tool before its 1.0, whose minor versions break things, licensed MPL-2.0, and
  whose calls through JNA cost microseconds each; its JNI backend, once released, is the faster
  path.
- Without `dl.google.com`, every Android build is checked in CI only.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Bindings straight to `keel-store` and `keel-domain`, with no runtime | The bindings would grow with every kernel type, and each shell would resolve choices, group a payment's events into one write and build views itself. |
| The catalog's rules in the shell | Written again for Apple and the web storefront, which must price carts the same way. |
| Each platform's formatting (`java.text`, Foundation, `Intl`) | Their symbols, spaces and rounding differ from each other and from the printed receipt. |
| The catalog in SQLite with full-text search from the start | A café's menu doesn't need it; 100,000 SKUs might, and slice 4 measures before choosing. |
| Hand-written JNI | Unsafe code written by hand, at the edge of the money path. |
| Faster bindings: UniFFI's JNI backend, BoltFFI | Calls hundreds of times faster, but the first is unreleased ("use at your own risk") and the second first released in February 2026. UniFFI's backend takes the same annotations, so moving to it later costs little. |
| Drivers in the shell: Kotlin sockets for printers and the hub | Each platform would write its own, untested by the simulator, while the renderers and the protocol are in Rust anyway. |
| The hub on a Linux daemon only | Most stores, and every single-device merchant, run the hub on an Android device ([ADR-0004](./0004-client-ui-stack.md)). |

## References
- [roadmap §2, §8](../roadmap.md#8-first-engineering-milestones-the-next-build-steps),
  [architecture README §4, §5](../architecture/README.md#4-containers),
  [hardware.md](../architecture/hardware.md),
  [offline-and-sync §3.3, §6.5](../architecture/offline-and-sync.md#33-transport-discovery-and-security),
  [security §2](../architecture/security.md#2-identity),
  [domain-model §5](../architecture/domain-model.md#5-catalog)
- [ADR-0003](./0003-rust-kernel-everywhere.md), [ADR-0004](./0004-client-ui-stack.md),
  [ADR-0010](./0010-hardware-abstraction-and-document-model.md),
  [ADR-0012](./0012-event-wire-format.md), [ADR-0015](./0015-checks-and-payments.md),
  [ADR-0017](./0017-projections-and-outbox.md),
  [ADR-0018](./0018-encryption-at-rest-and-integrity-checks.md),
  [ADR-0019](./0019-replication-and-deterministic-simulation.md)
- Research, as of 2026-10-02 (the versions will have moved by the slices that use them):
  - CLDR 48: [cldr-json 48.2.3](https://github.com/unicode-org/cldr-json), and the snapshot's
    [README](../../core/crates/keel-types/data/cldr/README.md).
  - Android: [AGP 9.3 and 9.4 release notes](https://developer.android.com/build/releases/agp-9-4-0-release-notes),
    [Kotlin's compatibility](https://kotlinlang.org/docs/gradle-configure-project.html),
    [Kotlin Multiplatform with AGP 9](https://developer.android.com/kotlin/multiplatform/plugin),
    [NDK downloads](https://developer.android.com/ndk/downloads),
    [16 KB pages](https://developer.android.com/guide/practices/page-sizes),
    [target API requirements](https://developer.android.com/google/play/requirements/target-sdk),
    [Keystore](https://developer.android.com/privacy-and-security/keystore),
    [foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types),
    [local network permission](https://developer.android.com/privacy-and-security/local-network-permission),
    [NsdManager](https://developer.android.com/reference/android/net/nsd/NsdManager);
    StrongBox's signing time from the KeyDroid measurements
    ([arXiv 2507.07927](https://arxiv.org/abs/2507.07927)), unconfirmed on the reference device.
  - Bindings and building: [UniFFI](https://github.com/mozilla/uniffi-rs) and its
    [changelog](https://github.com/mozilla/uniffi-rs/blob/v0.32.2/CHANGELOG.md),
    [foreign traits](https://github.com/mozilla/uniffi-rs/blob/v0.32.2/docs/manual/src/foreign_traits.md),
    Firefox's [JNA rules for R8](https://github.com/mozilla/application-services/blob/main/proguard-rules-consumer-jna.pro),
    [JNA's changes](https://github.com/java-native-access/jna/blob/master/CHANGES.md),
    [cargo-ndk](https://github.com/bbqsrc/cargo-ndk).
  - The LAN: [rustls](https://github.com/rustls/rustls),
    [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite),
    [rcgen](https://github.com/rustls/rcgen),
    [matrix-sdk-ffi](https://github.com/matrix-org/matrix-rust-sdk/tree/main/bindings/matrix-sdk-ffi),
    UniFFI and `rustls` with `aws-lc-rs` on Android in production.
  - Printing: [Epson's ESC/POS reference](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/commands.html)
    (status, `GS ( H`, code tables, real-time commands),
    [Star's ESC/POS emulation](https://www.starasia.com/Download/Manual/com-emu_escpos_cm_en.pdf),
    [Epson's network interface](https://files.support.epson.com/pdf/ube04_/ube04_trg.pdf) and
    [Star's](https://www.starasia.com.hk/Download/Manual/UsersManual_IFBD_HE0708BE07_EN.pdf)
    on port 9100, and printer profiles from
    [escpos-printer-db](https://github.com/receipt-print-hq/escpos-printer-db).
