# Keel: notes for AI coding agents

Keel is a local-first point-of-sale platform. The design lives in `docs/` (start with
`README.md`, then `docs/architecture/README.md`); the Rust kernel lives in `core/crates/`.

## Before changing kernel code

Read `docs/engineering/conventions.md`. In short:

- The lints in `Cargo.toml` are the law: no panics, no unchecked arithmetic, no `as` casts, no
  floating point, no `unsafe`. Don't add `#[allow]` without a `reason`, and prefer fixing the code.
- Money is `i64` minor units plus a currency; quantities are `i64` millionths plus a unit. Never
  mix currencies or units, never round implicitly, and never multiply money by a `Decimal`
  directly (use `Money::mul_decimal`).
- Never read the system clock or random number generator in kernel code: take a `Timestamp`,
  `Clock` or `Entropy`.
- New dependencies need an entry in the conventions' dependency policy.
- Event payloads follow ADR-0013: any payload change, even a new optional field, is a new schema
  version with a pinned example payload. Never change what an existing version decodes.
- Golden stores (`core/crates/keel-store/tests/golden/`) pin what devices hold on disk: never
  regenerate one. A new store schema version adds a golden store of its own (ADR-0018).
- Update the architecture docs in the same change when code makes them outdated.
- Keep [`docs/progress.md`](docs/progress.md) current: update it as each piece of work lands, and
  record each finished slice there with how it was verified.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo test --workspace --all-features                   # quick
cargo test --workspace --all-features -- --include-ignored  # with the exhaustive sweeps, as CI runs
PROPTEST_CASES=100000 cargo test --workspace            # soak before significant changes
cargo build -p keel-types -p keel-events -p keel-domain -p keel-pricing --no-default-features --target wasm32-unknown-unknown
python3 tools/planted-bugs/run.py <crate> [--props]    # plant the crate's known bugs, one at a time
KEEL_SIM_SEEDS=2000 cargo test --release -p keel-sim --test seeds every_seed  # soak the simulator
KEEL_SIM_FIRST_SEED=<seed> KEEL_SIM_SEEDS=1 KEEL_SIM_LOG=1 cargo test -p keel-sim --test seeds every_seed  # replay a seed
python3 core/crates/keel-pricing/tests/golden/generate.py  # after changing how pricing works
python3 core/crates/keel-types/tools/gen_currency_table.py  # after changing the ISO 4217 snapshot
KEEL_STORE_MAKE_GOLDEN=1 cargo test -p keel-store --test golden_store -- --ignored  # once, for a new store schema version
```

## Testing expectations

Test new behavior with unit tests for known answers and with property tests against an exact
`num-bigint` oracle, such as those in `core/crates/keel-types/tests/support/` and
`core/crates/keel-pricing/tests/support/`. Add generators that aim at boundaries (ties, range
ends, time zone transitions). Confirm a new test fails when you plant the bug it targets, and add
the bug to the crate's list in `tools/planted-bugs/`. Turn every failure a property test finds
into a named regression test.
