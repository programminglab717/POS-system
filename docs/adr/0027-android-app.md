# ADR-0027: The Android app: AGP 9.5 and Gradle 9.8, the kernel built with cargo-ndk, a register module in Kotlin alone, keys wrapped in the Keystore, built in parts

- **Status:** Proposed
- **Date:** 2026-11-09

## Context

Slice 4 of step 7 is the Android app ([ADR-0023](./0023-register-shell.md), decision 8): the
register's screens, keys in the Android Keystore, the build for Android, CI on an emulator, and the
performance harness on a reference device. It comes after slice 5, the local hub
([ADR-0026](./0026-local-hub.md)), which the app will host.

ADR-0023 planned for an environment that couldn't reach Google's repositories, so that the Android
build was verified in CI alone. This environment can reach them now: the Android SDK, NDK r30, the
Android Gradle Plugin, AndroidX and Compose are installed and resolved here, and the app is built
here. It has no KVM, so the emulator runs in CI alone.

What is built shapes the rest:

- `keel-ffi` ([ADR-0025](./0025-bindings.md)) answers each intent with a whole view, as UniFFI
  records, and its refusals as `KeelException`'s subclasses; Kotlin calls it through JNA, from one
  thread.
- `keel-ffi` builds for Android with NDK r30 as it is: SQLCipher with its vendored OpenSSL
  cross-compiles, and `libkeel_ffi.so` for `arm64-v8a` is 7.4 MB stripped, its segments aligned to
  16 KB, and linked to Android's system libraries alone.
- The runtime writes without replication until `start_sync` (ADR-0026), so a register works on its
  own before the app hosts the hub.
- The kernel draws a device's identifier as a UUIDv7, which a shell can't be trusted to mint.

## Decision

1. **Versions**, the newest released and tested together: the Android Gradle Plugin 9.5.1 on
   Gradle 9.8.2 (AGP 9.5 needs 9.6 or later), through the wrapper; Kotlin 2.4.21, the bindings';
   Compose's BOM 2026.09.00; `compileSdk` and `targetSdk` 36, `minSdk` 28 (ADR-0023); build tools
   36.1.0; NDK r30 (30.0.16248370); JDK 21 for the build, Java 17 bytecode for the app. Every
   version sits in one catalog, `gradle/libs.versions.toml`. This amends ADR-0023's AGP 9.3 and
   Gradle 9.7, which newer releases have passed.

2. **Three modules** in `apps/android`:
   - `:register`, Kotlin Multiplatform with a JVM target alone for now: `Till`, what the register
     asks of the kernel in its own words, and its records; and `RegisterModel`, the register's
     state, a `StateFlow` that each intent changes when the kernel answers, each intent run on
     one worker so the kernel has one caller. Its tests run on the JVM against a fake `Till`.
   - `:kernel`, an Android library: `keel-ffi` built by `cargo ndk` for `arm64-v8a` and `x86_64`
     (`-PkeelAbis` narrows them, for an emulator), always optimized, since a debug kernel is many
     times larger and slower, and its own tests run on the host; the Kotlin bindings, generated
     from the kernel built for the host by `bindings/kotlin/generate.sh --out`, the same library
     and so the same Kotlin; JNA's Android archive; and `KernelTill`, the `Till` the kernel
     answers, which turns its refusals into the cashier's words. Both are generated sources of
     the build, never committed.
   - `:app`: the activity, its Compose screens, `RegisterViewModel`, which opens the kernel on a
     thread of its own at its first call and makes every call on it, and the device's keys.

3. **Keys** (ADR-0023): `KeyVault` wraps each secret with an AES-256-GCM key the Keystore holds,
   in StrongBox where the device has one, else in its TEE, never bound to a lock screen; each
   secret is unwrapped again at once after wrapping. `DeviceIdentity` keeps the device's
   identifier and its wrapped signing and store keys in the app's no-backup directory, written
   the first time from `keel-ffi`'s new `new_device()`, which draws the identifier as the kernel
   does and each key on its own from the operating system's randomness; the keys first and the
   identifier last, so a device is made only once all three are. The store lies beside them.
   Backups and device transfers are off, and the data extraction rules exclude everything.

4. **The register, built in parts**, each verified as it lands, then reviewed:
   1. *The skeleton* (this change): the build, the three modules, the keys, and a screen with
      the menu's pages and the order being rung, paid with exact cash, signed in as the demo
      café's first team member; the register's JVM tests; an instrumented test that rings an
      espresso and pays it; and CI's jobs, below.
   2. *The register's screens*: sign in from the team, the modifier sheet for an item's groups
      (the required first), quantities and removing lines, cash tender with change, today's
      orders. `keel-ffi` gains the team and the day's orders.
   3. *Printing*: the printer's address, a print loop on its own thread that runs `printTcp`
      outside the register's lock, unconfirmed jobs and reprints; the instrumented test's receipt
      received by a fake printer.
   4. *The hub on Android*: replication through `keel-ffi`, `keel-net` in the app's process, a
      kit provisioned onto the device, and the hub's role as a foreground service.
   5. *The performance harness*: tap to frame at the 99th percentile with a 100,000-SKU catalog,
      and the kernel's golden baskets on Android, on the reference device.

5. **CI**: a job that builds the kernel for both ABIs and the app, runs the register's JVM tests,
   builds the instrumented tests, and checks every `libkeel_ffi.so` in the APK is aligned to
   16 KB pages; and a job that runs the instrumented tests on an emulator, API 30 with 2 GB, like
   the reference device, building the kernel for `x86_64` alone. The wrapper's jar is checked
   against Gradle's published checksums by `gradle/actions/setup-gradle`.

## As built

The skeleton's details, settled in building it:

- **The wrapper's distribution checksum** isn't pinned yet: Gradle publishes it at
  `downloads.gradle.org`, which this environment's network refuses. CI can reach it; the next
  part pins it.
- **The skeleton signs in as the demo café's first team member** and rings items with no
  choices, which every item of the demo café allows: the sign-in screen and the modifier sheet
  come with part 2.
- **The store's lock failed on Android**, found by CI's emulator: std's `File::try_lock` has no
  implementation for Android and always fails there, so every store was refused as open
  already. `keel-store` takes the lock with `flock` through `rustix` on every Unix, so the host's
  tests take the same path as Android. Opening the kernel goes through `KernelTill.open`, which
  words its refusals as every other call's, and a failure with an empty message shows its
  exception's name: the first sign of this bug was "Something went wrong: ", UniFFI's message for
  a variant without fields.
- **The register's screens** (part 2): signing in from the team, and out; the modifier sheet,
  `ModifierSheet`, which shows the item's required groups first and a group under a modifier
  once it is chosen, chooses alone in a group of one, counts repeats up to a group's most, and
  clears what was chosen under a modifier with it; quantities and removing lines, and dropping
  an order; the cash tender, with the kernel's offers (what is due, then the smallest amounts
  above it in whole multiples of 1, 5, 10, 20, 50 and 100 of the currency's major unit) and a
  keypad whose amount the kernel formats; and today's orders, an open one taken up again. The
  kernel gains `team`, `orders_today`, `cash_offers` and `amount`.
- **Printing** (part 3): `PrintLoop`, in `:kernel` as it knows the bindings' types, takes each
  job due and records its outcome on the kernel's thread and sends it on a thread of its own,
  outside the register's lock; it sets the default printer profile (48 columns, a cutter,
  confirmed once printed) when it first has an address, as jobs wait for one. The address is kept
  in the app's preferences (`StoredPrinter`); a printer page lists the jobs and asks staff
  whether an unconfirmed one printed. `:kernel`'s unit tests run on the host, against the kernel
  built for it, through JNA's host library; the instrumented test's fake printer runs in the
  test's own process, on the device, rather than on the host.
- **The hub on Android** (part 4): `keel-ffi` joins a register to its location's network
  (`start_sync`, `sync_step`, `sync_status`), the roster's most preferred hub listening at its
  port and every other device connecting; `keel-hub`'s daemon is behind a default feature, so the
  bindings take `step` without the provisioning tool, which never ships. A provisioned kit,
  pushed into the app's external files directory, is imported on first launch by `DeviceFiles`,
  which wraps its secrets and removes it; a device without one is drawn alone, on the demo café's
  profile. Replication runs on the kernel's thread every 50 ms, each turn without waiting, so a
  cashier's intents wait for none of it. The hub runs while the register's screen is open: the
  foreground service that keeps it running in the background comes next.
- **The release build isn't shrunk** yet: JNA finds the bindings' classes by reflection, and the
  rules that keep them (`kernel/consumer-rules.pro`) are checked when the release build is.

## Consequences

**Positive**
- The app builds and its JVM tests run here, so each part is verified as it lands, not a CI run
  at a time; only the emulator waits for CI.
- The screens' state is tested on the JVM against a fake, and shared with desktop lanes later.
- The kernel's bindings are generated by the build, from the kernel it ships.

**Negative**
- Each build of the app builds the kernel too, twice for the two ABIs: cargo rebuilds only what
  changed, but a clean build takes minutes.
- The register's records repeat the bindings' in its own words, and `KernelTill` maps one to the
  other.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| AGP 9.3 and Gradle 9.7, as ADR-0023 named | Passed by 9.5 and 9.8, which are released, and tested together. |
| The register's state over the bindings' records directly | The bindings are an Android library here, so the state couldn't be tested on the JVM, or shared with desktop lanes. |
| The kernel built by Gradle's own NDK support (CMake) | The kernel is Cargo's; `cargo-ndk` sets the NDK's toolchain for each ABI and is what UniFFI's Android projects use. |
| A device identifier drawn in Kotlin | Its identifiers are the kernel's UUIDv7s, drawn as every other is. |

## References
- [ADR-0023](./0023-register-shell.md), decision 8; [ADR-0025](./0025-bindings.md);
  [ADR-0026](./0026-local-hub.md)
- [AGP 9.5 release notes](https://developer.android.com/build/releases/agp-9-5-0-release-notes),
  [Android Keystore](https://developer.android.com/privacy-and-security/keystore),
  [16 KB page sizes](https://developer.android.com/guide/practices/page-sizes),
  [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) 4.1
