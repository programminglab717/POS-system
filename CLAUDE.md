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
- Update the architecture docs in the same change when code makes them outdated.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo test --workspace --all-features                   # quick
cargo test --workspace --all-features -- --include-ignored  # with the exhaustive sweeps, as CI runs
PROPTEST_CASES=100000 cargo test --workspace            # soak before significant changes
cargo build -p keel-types --no-default-features --target wasm32-unknown-unknown
python3 core/crates/keel-types/tools/gen_currency_table.py  # after changing the ISO 4217 snapshot
```

## Testing expectations

Test new behavior with unit tests for known answers and with property tests against the exact
`num-bigint` oracle in `core/crates/keel-types/tests/support/`. Add generators that aim at
boundaries (ties, range ends, time zone transitions). Confirm a new test fails when you plant the
bug it targets. Turn every failure a property test finds into a named regression test.
