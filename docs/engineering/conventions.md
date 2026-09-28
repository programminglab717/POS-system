# Engineering conventions

How Keel's code is written, tested and changed. The kernel handles real money on every device, so
these rules are strict, and most are enforced by the compiler, the lints or CI rather than by
review.

## 1. Workspace and toolchain

- **One Cargo workspace** at the repository root, with one `Cargo.lock` for every Rust component:
  kernel crates now, the Store Hub and cloud services later. Every tier links the exact same
  dependency versions, which the kernel's determinism depends on.
- **Kernel crates** live in `core/crates/keel-*`.
- **The toolchain is pinned** in `rust-toolchain.toml` (Rust 1.94.1, with rustfmt, clippy and the
  `wasm32-unknown-unknown` target). The minimum supported version is the workspace `rust-version`.
  Toolchain upgrades are deliberate and get their own commit.
- **Edition 2024**, resolver 3. `rustfmt.toml` sets the style: 100 columns, `use_small_heuristics
  = "Max"`.

## 2. Rules for kernel code

### 2.1 Enforced by lints

`[workspace.lints]` in `Cargo.toml` turns these into compile errors:

| Rule | Lints | Instead |
|---|---|---|
| No panics | `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `unreachable`, `indexing_slicing` | Return `Result` or `Option`; use `get`, `?`, `ok_or` |
| No silent overflow | `arithmetic_side_effects` | `checked_*` methods; `wrapping_*` only where wrapping *is* the algorithm, with a comment |
| No lossy casts | `as_conversions`, `cast_*` | `From` and `TryFrom` |
| No floating point | `float_arithmetic`, `float_cmp`, `lossy_float_literal` | Integers and `Decimal` |
| No unsafe code | `unsafe_code = "forbid"` | — |
| Everything public is documented | `missing_docs` | — |

Clippy's `pedantic` group is on. Each exception is listed in `Cargo.toml`, and every `#[allow]`
needs a `reason`. Assertions in `const` contexts are allowed: they fail the *build*, never the
program. The currency and unit tables use them to guarantee that `Decimal` construction can't panic.

Release builds keep `overflow-checks` on as a second line of defense for code outside these lints
(dependencies and tests).

### 2.2 Domain rules

- **Money** is an `i64` count of a currency's minor units plus a `Currency`. **Quantities** are an
  `i64` count of millionths of a `Unit`. Neither has arithmetic operators or `PartialOrd`: amounts
  are combined with checked methods, and currencies and units never mix.
- **Rounding** happens only where the domain requires it (tax calculation, allocation and cash
  rounding), always with an explicit `RoundingMode` or `RoundingRule`, and always from the exact
  value, exactly once. `rust_decimal` rounds silently when a product needs more than 96 bits, so
  never multiply money by a `Decimal` directly: use `Money::mul_decimal` or
  `Quantity::mul_decimal`, which compute the product exactly (up to 192 bits) before rounding.
- **Determinism.** Kernel code never reads the operating system's clock or random number
  generator. It takes a `Timestamp`, or a `Clock` or `Entropy` it was given. Time zone rules come
  from the IANA database bundled into the kernel, never from the device. Code that touches the
  operating system sits behind the `os` feature, which the `wasm32` build and simulations leave off.
- **Identifiers** are UUIDv7s from an `IdGenerator`, typed as `Id<T>`.
- **Stable codes.** Currency codes, unit codes and every serialized form end up in signed events, so
  they never change. Tests pin them.
- **Event schemas** follow [ADR-0013](../adr/0013-event-payloads-and-schema-evolution.md).
  Decoding is strict, so any change to a payload, even a new optional field, is a new schema
  version: list it in the aggregate's `SCHEMAS`, pin an example payload, and upcast the older
  versions. Never change what an existing version decodes. Every event a command produces must
  decode under its own schema before it is recorded.
- **Errors.** Each module has one `thiserror` error enum, marked `#[non_exhaustive]`, with lowercase
  messages and no trailing period.
- **Text.** Parsing is strict (APIs, imports and tests). `Display` is canonical and
  locale-independent. Formatting for people is the UI's job.

## 3. Testing

- **Unit tests** sit next to the code and cover known answers and edge cases. Every bug gets a
  named regression test. Proptest also saves the seed of every failure it finds in a
  `*.proptest-regressions` file beside the test and replays it on each run: commit these files.
- **Property tests** (`proptest`, in `tests/`) compare the code with an independent oracle, built
  differently from the implementation so the two are unlikely to share a mistake: exact rational
  arithmetic with `num-bigint`, written from the definitions; another implementation of a standard
  (`ciborium` for CBOR); or a model of the rules written from the specification, such as which
  envelope fields are valid, where a received event fits in a log, or what a fold makes of any
  sequence of events. Give a model a different shape from the code: the order fold is a state
  machine, and its model is a set of queries over the events' positions ("the first removal after
  the line was added"). Known-answer tests pin formats with bytes checked by outside tools
  (Python's `cbor2`, `hashlib` and `pycose`).
- **Aim at the boundaries.** Uniformly random inputs almost never land where the bugs are. The
  double-rounding bug in the first version of `mul_decimal` passed 2,000 random cases, and a
  generator that puts products exactly on (and a hair either side of) rounding boundaries caught it
  at once. Write such generators for ties, range ends and time zone transitions.
- **Generate faulty inputs on purpose.** A generator that only makes valid commands can't catch a
  missing check. Give command generators faulty variants, such as an identifier already in use, a
  price in another currency, a zero quantity or another location, and require the code and the
  model to agree on every one. Folds get events no single device would write, such as a change to
  a line that was never added.
- **No `prop_assume!`.** Generate valid inputs directly, or filter inside the strategy. Rejected
  cases count against a global limit that aborts high case-count runs.
- **Check that new tests can fail.** Plant the bug a test is meant to catch, and confirm the test
  fails. Plant bugs a property test should catch on its own too, and run only the property tests:
  unit tests often catch a bug that the property test's generators never reach.
  - Each crate's planted bugs are listed in `tools/planted-bugs/<crate>.py`, and
    `tools/planted-bugs/run.py` plants them one at a time: `run.py keel-domain` against every
    test, `--props` against the property tests alone, `--ignored` with the exhaustive sweeps. Add
    the bugs a new test targets to the list.
  - The runner reports a bug whose text no longer matches the code as stale, and one that doesn't
    compile as proving nothing; a compile error is never counted as a caught bug.
  - It restores each file by rewriting it, so Cargo sees a new modification time, and runs with
    `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`, so a planted bug's failures never reach the
    regression files. Don't edit or build the crate while it runs, or give it a separate worktree
    with `--root`.
- **Golden baskets** check the pricing engine against an independent oracle in Python, with exact
  fractions (`core/crates/keel-pricing/tests/golden/generate.py`). CI regenerates the baskets and
  fails if they change, so the committed expectations always come from the oracle.
- **Exhaustive sweeps**, such as every time zone transition from 1970 to 2037, are `#[ignore]`d
  for quick local runs. CI runs them with `-- --include-ignored`.
- **Case counts.** Locally, property tests run proptest's default of 256 cases. CI runs 4,096
  (`PROPTEST_CASES`). Before a significant change, run 100,000. Don't set a case count in a test:
  it would override these. Dependencies are compiled with optimizations even in test builds (the
  workspace's dev profile), so property tests can afford thousands of signatures.
- **Test code follows the same lints.** Integration test crates may allow `unwrap`, `expect`,
  `panic`, `unreachable`, indexing and arithmetic, with a reason, since a failed assumption there
  should fail loudly.
  `clippy.toml` allows arithmetic on `BigInt`, which can't overflow.

## 4. Dependency policy

Every dependency is a liability in a money kernel. A new dependency needs an entry in the table
below, and must be:

- **pure Rust**, and for kernel crates, buildable for `wasm32-unknown-unknown`;
- **permissively licensed** (MIT, Apache-2.0, BSD, ISC, Zlib or Unicode);
- **deterministic**, with no hidden I/O, clock reads or randomness;
- **well maintained and widely used**, with a stable API;
- **minimal**: default features off unless they are needed.

| Crate | Used by | Why |
|---|---|---|
| `rust_decimal` | keel-types | Exact 96-bit decimals for rates, factors and prices per unit. Default features off. Its multiplication rounds silently beyond 96 bits, so money math uses the exact paths in `keel-types` (§2.2). |
| `uuid` | keel-types | UUID parsing, formatting and layout. Keel generates UUIDv7s itself: monotonic, with injected entropy. |
| `jiff` | keel-types | Calendar and time zone arithmetic, with the IANA database bundled (`tzdb-bundle-always`) so every platform applies the same rules. |
| `thiserror` | all crates | Error types without boilerplate or runtime cost. |
| `getrandom` | keel-types (`os` feature) | The operating system's secure random numbers, for identifiers. |
| `sha2` | keel-events | SHA-256, for event hashes and key identifiers. Default features off. |
| `p256`, `ecdsa` | keel-events | ECDSA on P-256 (ES256), the algorithm secure hardware supports: verification, low-S normalization, DER decoding of hardware signatures, and deterministic signing (RFC 6979) for software keys. Default features off. |
| `ed25519-dalek` | keel-events | Ed25519, for devices without secure hardware, with strict verification. Default features off: keys come from injected entropy, never from the operating system directly. |

Test-only dependencies must be permissively licensed, but need not build for `wasm32`:

| Crate | Why |
|---|---|
| `proptest` | Property testing. |
| `num-bigint`, `num-integer` | The exact arithmetic oracle, and the scalar arithmetic that crafts invalid Ed25519 signatures. |
| `ciborium` | An independent CBOR implementation, which the codec's property tests agree with. |
| `serde_json` | Reads the golden baskets, which the Python oracle writes as JSON. |
| `csv` | Reads the ISO 4217 snapshot in the currency table test. |

`Cargo.lock` is committed. Update dependencies deliberately (`cargo update -p <crate>`), read
their changelogs, and say in the commit message when a `jiff` or `jiff-tzdb` update changes time
zone rules.

To do: check licenses and security advisories in CI (`cargo-deny`), and pin GitHub Actions to
commit hashes.

## 5. Reference data

Reference data compiled into the kernel, such as the ISO 4217 currency list, lives next to its
crate as a snapshot, with a README recording its source, how it was verified and how to update it.
A generator script turns the snapshot into Rust source, and CI checks that the generated file
matches.

## 6. Documentation

- Public API documentation says what an item does, why it exists and when it fails (`# Errors`).
- Architecture documents change in the same commit as code that makes them outdated.
- [`docs/progress.md`](../progress.md) records where the build stands. Update it as work lands,
  and record each finished slice with what it built, how it was verified and its commits.
- Write plain, precise English: short sentences, active voice, no hype.

## 7. Commits

One logical change per commit, with a message that says what changed and why. Everything in §2
and §3 passes before a commit: format, lints, tests and documentation.
