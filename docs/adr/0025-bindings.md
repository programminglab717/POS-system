# ADR-0025: The bindings: `keel-ffi` with UniFFI 0.32, a register object that answers with whole views, and a Kotlin/JVM library tested on the host

- **Status:** Accepted (2026-11-09) after review, with the details settled in building and
  reviewing it, under "As built"
- **Date:** 2026-11-09

## Context

Slice 3 of step 7 is the bindings ([ADR-0023](./0023-register-shell.md), decision 7): "`keel-ffi`
with UniFFI, and a Kotlin/JVM library tested on the host". ADR-0023 fixes the outline:

- UniFFI 0.32, pinned exactly, with its procedural macros; Kotlin generated from the compiled
  library by a binary in the workspace on the same version, and not committed.
- One call per thing a cashier does, answered with the whole view; the runtime as an object, its
  views as records, its refusals as typed errors that reach Kotlin as exceptions.
- Kotlin calls through JNA, 5.19.1 or later, from one background thread.
- `keel-ffi` keeps every workspace lint, `unsafe_code` forbidden included: the code UniFFI's
  macros generate isn't linted as the crate's own. The kernel crates stay free of UniFFI.
- The platform edge: the one place that reads the system clock and the operating system's
  randomness.
- The generated Kotlin compiles for Android and the desktop JVM alike; a Kotlin/JVM library and
  its tests run against the kernel built for the host, here and in CI, with no Android SDK.
- UniFFI is MPL-2.0; the dependency policy gains that exception.

What slices 1 and 2 built shapes the rest:

- The runtime (`keel-runtime`) is generic over its signer, entropy, clock and identifiers'
  entropy, used from one thread at a time, and answers each intent with the order's ticket.
- Print jobs are handed to a driver by `next_print_job` and their outcomes recorded by
  `print_outcome`; the driver, `keel-print`, does blocking I/O and must not run on the runtime's
  thread, nor while the runtime is locked.
- Every amount in a view carries its text; identifiers are typed `Id<T>`, UUIDv7s.

A spike in this environment, outside the repository, settled the toolchain: UniFFI 0.32.2's
macros compile in a crate with `unsafe_code = "forbid"`; its bindgen, run on the compiled
library, writes one Kotlin file; Gradle 8.14 with Kotlin 2.4.21 and JNA 5.19.1 from Maven
Central compiles it and runs a JUnit test that calls the Rust library. Maven Central rate-limits
this environment's proxy now and then: Gradle runs with one worker, and retries.

## Decision

1. **`keel-ffi`**, a platform crate in the workspace, built as a `cdylib` (for JNA) and a `lib`
   (for its own Rust tests): the only crate that depends on UniFFI, pinned `=0.32.2`. Its
   bindgen is a binary in the crate, `uniffi-bindgen`, behind a `bindgen` feature, so that the
   library never carries UniFFI's command-line dependencies.

2. **The register object**, `Register`, wraps one runtime behind a mutex, which makes it safe to
   share and serializes its calls: the shell calls it from one background thread anyway, and a
   second thread waits rather than racing.
   - `Register.open(config)`: the store's path and key (32 bytes, which the shell keeps wrapped in
     the platform's keystore), the device's identifier, its signing key (a software Ed25519 key's
     32 secret bytes in v0), the location profile's encoding, and a locale tag. It uses the system
     clock and the operating system's randomness: the platform edge.
   - One method per intent and view of the runtime, with the same names in Kotlin's case:
     `signIn`, `signOut`, `startOrder`, `addItem`, `changeQuantity`, `removeLine`, `abandon`,
     `payCash`, `ticket`, `openOrders`, `menu`, `item`; and for printing `setPrinter`,
     `nextPrintJob`, `printOutcome`, `resolve`, `reprint`, `printJobs`.
   - `printTcp(host, port, job, waits)`, a free function, runs the driver on the caller's thread,
     outside the register's lock: the shell calls it on its printing thread, then records the
     outcome on the register.
   - `demoProfile()` gives the demo café's encoding, for the shell before a location has one, and
     for tests.

3. **What crosses the boundary**: records and enumerations, never the kernel's types.
   - Identifiers as their canonical text, parsed strictly on the way in (a malformed one is
     `KeelException.InvalidArgument`).
   - Amounts as `{minor, currency, text}`; quantities as their millionths and their text; tenders
     as minor units in the location's currency.
   - Views as records of the runtime's views, field for field.
   - Print jobs as `{job, kind, bytes, confirm, processId}`; outcomes as an enumeration of the
     driver's.

4. **Errors**: one enumeration, `KeelError`, which UniFFI makes `KeelException` with a subclass
   per variant. The refusals a cashier can meet are variants of their own, with what the shell
   needs to say why (`TenderShort` carries what is due); the domain's refusals carry their message;
   the device's failures are `Device`, for someone to look at the store. No panic crosses: the
   crate keeps the workspace's lints, and UniFFI turns any it meets into an exception anyway.

5. **The Kotlin library**, `bindings/kotlin`, a Gradle project (Kotlin/JVM 2.4.21, JNA 5.19.1,
   JUnit 5 through `kotlin-test`):
   - its sources are the Kotlin UniFFI generates, into `build/`, never committed, by a script,
     `bindings/kotlin/generate.sh`, which builds `keel-ffi` for the host and runs the bindgen on it;
   - its tests run against that library: open a register on the demo profile, ring an order with
     modifiers, see refusals as exceptions, pay cash, and send the drawer kick and the receipt with
     `printTcp` to a fake printer written in Kotlin, which answers as a ready Epson;
   - it is Kotlin/JVM now; slice 4 moves it into the Android library, where the same generated
     Kotlin compiles.

6. **CI**: a job of its own builds `keel-ffi`, generates the Kotlin, and runs the Kotlin tests with
   Java 21 and Gradle 8.14; the Rust job lints and tests `keel-ffi` with the rest.

7. **The dependency policy** gains UniFFI (MPL-2.0, a file-level copyleft that obliges nothing of
   Keel's own files when it is used unmodified as a library) and the crates it brings, each
   permissively licensed, listed with their licenses.

8. **Verification**:
   - Rust tests of `keel-ffi`: each conversion of identifiers, amounts and views, every error's
     variant, and a register driven through a sale and its print jobs, as Kotlin would drive it;
   - the Kotlin tests above, here and in CI;
   - planted bugs in the conversions, caught by the tests.

## As built

Details settled in building it:

- **Nothing crosses that nests itself**: an item's groups are one list, each group with its depth
  and the modifier it is offered under, and a cashier's choices one list, each naming the index of
  the choice it is chosen under, which must come before it. The Rust side builds the tree and
  refuses a parent that doesn't come first.
- **No error field is named `message`**, which Kotlin's exceptions already have: the refusals
  that explain themselves carry a `reason`.
- **Kotlin's types**: Rust's unsigned integers become Kotlin's (`UByte`, `UInt`, `UShort`,
  `ULong`); bytes are `ByteArray`; enumerations' cases are upper case (`Mode.TAKEOUT`).
- **The package is `dev.keel.kernel`**, from `core/crates/keel-ffi/uniffi.toml`, which
  `generate.sh` passes to the bindgen as its configuration (`[crates.keel_ffi.bindings.kotlin]`):
  with UniFFI's default features off, the bindgen finds no crate's configuration by itself.
- **What is due** when a tender is short crosses with its text, in the register's locale.
- **The Gradle build** runs `generate.sh` before it compiles and before it tests, points JNA at
  the library it built, where `CARGO_TARGET_DIR` puts it, reruns the tests whenever the library
  changes (the review found them skipped after a change to Rust that left the Kotlin as it was),
  and downloads one thing at a time.

The review, an independent one, found and these fix, each with a test or a planted bug:

- **A store opened twice had two writers**, whatever the register's lock: a second `open` of the
  same path, as Android recreating an activity does, succeeded, and every write of the second
  register failed once the first had written. The store now takes an exclusive lock on a file
  beside it, `<path>.lock`, which the operating system releases when the store closes or its
  process dies; a second open is refused, `KeelError.StoreInUse` (`StoreError::Busy` in the
  store). A shell must close a register before it opens its store again: `Register` is
  `AutoCloseable`, and the Kotlin tests close theirs with `use`.
- **Store failures** have variants of their own, where they were all `Device`: `StoreInUse`,
  `WrongKey` (the keystore's key isn't this store's), `StoreNewer` (the app was downgraded),
  `NotThisDevice` and `Damaged`.
- **A panic** under the register's lock poisoned it, and the register refused everything after.
  The lock is recovered: whatever the call was writing was rolled back, as every write is a
  transaction. The panic itself reaches Kotlin as UniFFI's `InternalException`.
- **Identifiers** must be in their canonical, lowercase form, as every identifier the register
  gives is, so a shell compares like with like: an uppercase one is `InvalidArgument`.
- **`printTcp`** refuses a process ID that isn't four printable bytes, `InvalidArgument`, where it
  said the printer was unreachable and the job was tried for ever; and tries each address its
  host resolves to while none is reached. Resolving a name takes as long as the system's resolver
  does: an address is quicker.
- **The tests** caught 3 of 25 bugs planted in the conversions; they check every field of every
  record and every case of every enumeration now, and the list holds all 28, each caught.
- **A gap**: Kotlin's `message` of a `KeelException` is UniFFI's rendering of its fields
  (`reason=…`), not the Rust error's text, and empty for the variants without fields. Shells
  show their own words for each variant, and `reason` where there is one.
- `generate.sh` runs under macOS's bash 3.2, which can't expand an empty array under `set -u`.

## Consequences

**Positive**
- The shell sees records and exceptions, never the kernel's types; the kernel crates stay
  portable and free of UniFFI.
- The bindings are tested on the host, here and in CI, before any Android code exists.
- Printing stays off the register's lock: a slow printer never holds up a tap.

**Negative**
- A copyleft-licensed tool before its 1.0, in the shipped library.
- Every view is copied into records at the boundary: microseconds per call, measured in slice 4.
- The signing key is a software key handed in by the shell until keys in secure hardware.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Bindings generated from a UDL file | The procedural macros keep the interface beside the code it exposes, and UniFFI's own direction is towards them. |
| Committing the generated Kotlin | It would drift from the library it is generated from; generating it in the build can't. |
| `Register` without a lock, one per thread | The store has one writer; a lock keeps that true whatever the shell does. |
| The driver called by the register | Blocking network I/O under the lock would hold up every tap while a printer times out. |
| The Gradle wrapper committed | A binary in the repository; CI and this environment have Gradle. |

## References
- [ADR-0023](./0023-register-shell.md), decision 7; [ADR-0024](./0024-receipts-and-printing.md)
- [UniFFI](https://github.com/mozilla/uniffi-rs) 0.32.2 and its
  [manual](https://mozilla.github.io/uniffi-rs/); [JNA](https://github.com/java-native-access/jna)
  5.19.1; [Kotlin](https://kotlinlang.org/) 2.4.21.
