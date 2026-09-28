# Build progress

> A living record of the build: where it stands, what each step delivered and how it was
> verified, and what is waiting on a decision. Updated as each piece of work lands.
> Last updated: 2026-09-28.

## Where we are

**Phase 0 (Foundations), step 4 of 8:** `keel-domain`, slice 1 of 3. This slice builds the payload
codecs, the schema registry, and the order aggregate's lines.

## Phase 0 milestones

From the [roadmap](./roadmap.md#8-first-engineering-milestones-the-next-build-steps).

| # | Milestone | Status | Where |
|---|---|---|---|
| 1 | Monorepo scaffold | Rust workspace and CI done. The Android, web and schema directories arrive with their first code. | [`Cargo.toml`](../Cargo.toml), [CI](../.github/workflows/ci.yml) |
| 2 | `keel-types`: value types | Done, 2026-09-27 | [`core/crates/keel-types`](../core/crates/keel-types/) |
| 3 | `keel-events`: the signed, hash-chained event log | Done, 2026-09-27. Its schema registry is being built with the first domain events (step 4). | [`core/crates/keel-events`](../core/crates/keel-events/), [ADR-0012](./adr/0012-event-wire-format.md) |
| 4 | `keel-domain` (order, check, payment) and `keel-pricing` v0 | In progress: slice 1 of 3 | `core/crates/keel-domain` |
| 5 | `keel-store`: SQLite events, projections and outbox | Not started | |
| 6 | `keel-sim` and `keel-sync` v0 | Not started | |
| 7 | Android register shell | Not started | |
| 8 | Cloud cell v0 | Not started | |

Step 4 is split into three slices, each ending with a review:

1. **Foundations and the order's lines** (in progress): payload codecs and schema registry; the
   aggregate framework; order events, fold and commands for creating an order, adding, changing,
   removing, firing, voiding and comping lines, changing attributes, and voiding or abandoning
   the order.
2. **Pricing v0**: modifier pricing, discounts, US sales tax, rounding and allocation, the
   calculation trace, and the golden-basket suite.
3. **Checks and payments**: splits and allocations, and the payment aggregate.

## Current slice: `keel-domain` foundations and the order's lines

| Work | Status |
|---|---|
| Payload codecs: strict field maps; money, quantity, identifier, text, code and set encodings | Done, with unit tests |
| Schema registry and the fold entry point (unknown and malformed payloads skipped and flagged) | Done, with unit tests |
| Order events v1 (ten schemas), order state, and a total fold with the sync design's conflict rules | Done, with a known-answer test for each conflict rule |
| Order commands, validated against the device's current view | Done, with unit tests for each check |
| Property tests: payload round trips and a validity model per schema, golden payloads checked by an outside tool, fold totality, commands against a reference model, concurrent devices against a model of the conflict rules | In progress |
| Planted-bug checks | Not started |
| Docs: ADR-0013 (payload conventions and schema evolution), domain model, progress | Not started |

Design decisions in this slice, to review when it ships:

- **Payloads** are canonical CBOR maps with small integer keys, decoded strictly, like the
  envelope. Money is `[minor units, currency code]`, quantity `[millionths, unit code]`.
- **Schema evolution:** a kernel decodes every version it knows, strictly. Writers only emit a
  version that every kernel at the location supports (negotiated by sync), so a newer version
  never meets an older kernel in normal operation.
- **Unknown or malformed payloads** from a trusted device stay in the log, so the device's chain
  never stalls, but the fold skips them and flags the aggregate ("needs update" or "malformed").
- **Text** in names and notes can't contain control characters, which could otherwise drive
  receipt printers and kitchen displays.
- **Invalid-in-context events aren't applied** when applying them would break the order's
  consistency: a line in another currency, a quantity in another unit, an event before the
  order's creation or from another location. They leave a conflict instead, so every price in an
  order can always be added up.
- **Commands** turn into exactly one event each, and every event a command produces must decode
  under its own schema before it is returned, so a device never writes a payload other kernels
  reject.

## Completed

### `keel-events`: the signed, hash-chained event log (2026-09-27)

Commits `34fda99`, `f79e701`.

- **Built:** a strict canonical CBOR codec; the event envelope (format 1); ES256 and Ed25519
  signatures in COSE_Sign1 messages; event hashes; the device log writer (numbering, chaining,
  timestamping and signing in two steps, so memory never runs ahead of storage); the link check
  for received events (gaps, forks, broken links, clocks going backwards); the device registry
  with revocation by log position.
- **Verified:**
  - 38 unit tests and 26 property tests, which also passed 100,000 cases each.
  - Interoperability both ways with Python's `pycose`; a pinned reference event checked
    independently with `cbor2`, `hashlib` and `pycose`.
  - 59 planted bugs, all caught; the property tests alone catch the 43 in the envelope, event,
    log and registry code.
  - CI green.
- **Decisions:** [ADR-0012](./adr/0012-event-wire-format.md), accepted 2026-09-28: a location
  field in every event, revocation by log position rather than time, one key per device for life,
  and strict decoding everywhere.
- **Found and fixed during the build:**
  - A payload nested deeper than the decoder allows could be signed but never decoded, which
    would have stalled the device's log for good. Such payloads are now refused when created.
  - Planted-bug runs were saving their failure seeds as if they were real regressions. The seeds
    were removed, and the conventions now require disabling proptest's failure persistence for
    planted-bug runs.

### `keel-types`: the kernel's value types (2026-09-27)

Commits `2bb1c68`, `493dd3b`, `de45ce0`.

- **Built:** money in integer minor units, with exact products (up to 192 bits) rounded once;
  seven rounding modes and cash rounding; rates; quantities in millionths with 47 exactly defined
  units; ISO 4217 currencies from a verified snapshot (155 circulating, 6 withdrawn); timestamps
  and clocks; hybrid logical clocks; UUIDv7 identifiers; business dates with daylight-saving-safe
  cutoffs.
- **Verified:** 110 tests and 2 doctests, with property tests against an exact `num-bigint`
  oracle and an exhaustive sweep of every time zone transition from 1970 to 2037. CI green.
- **Found and fixed during the build:** double rounding in `Money::mul_decimal` (caught by a
  generator aimed at rounding boundaries after 2,000 uniform cases missed it); parsing of
  `i64::MIN`; an order-dependent sum; a naive business-date algorithm that failed on real time
  zone histories (Algiers 1971, Samoa 2011).

### Design (2026-09-27)

Commits `a9fad0c`, `545db4b`.

Six research tracks, the architecture and eleven ADRs, the domain model, the offline and sync
design, the feature catalog and the roadmap. See the [README](../README.md).

## Waiting on a decision

- The license, and the product name ("Keel" is a codename).
- The first payment processor (decision gate G2 in the roadmap).
- Verifying the research's unverified claims, and interviews with merchants.
- Updating the pinned Rust toolchain from 1.94 to 1.98.
- License and security-advisory checks in CI (`cargo-deny`), GitHub Actions pinned to commit
  hashes, and CI on macOS and Windows.

## Known gaps

Work deliberately left for later, so it isn't forgotten:

- **`keel-events`:**
  - Enrollment and revocation records signed by the Device CA, and their distribution to replicas.
  - A way to re-admit genuine events quarantined by a revocation.
  - Hardware signers, which the platform apps provide through the `Signer` trait.
  - Negotiating schema versions between kernels, in `keel-sync`.
- **`keel-types`:** `Locale`, with the first UI.
- **Pricing:** multiplying a unit price by a fractional quantity (weighed items) must round. The
  domain model lists only three places where rounding happens, so pricing v0 has to settle where
  this one belongs.

## Keeping this document current

Update "Where we are" and the current slice's table as work lands. When a slice ships, move it to
"Completed" with what it built, how it was verified, its decisions and its commits. Add every
deferred item to "Known gaps", and remove it when it's done.
