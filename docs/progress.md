# Build progress

> A living record of the build: where it stands, what each step delivered and how it was
> verified, and what is waiting on a decision. Updated as each piece of work lands.
> Last updated: 2026-09-28.

## Where we are

**Phase 0 (Foundations), step 4 of 8:** `keel-domain` and `keel-pricing`. Slices 1 to 3 of 4
(the order's lines, pricing v0, and checks and splits) are built and reviewed. Slice 4, payments
and closing, is in progress.

## Phase 0 milestones

From the [roadmap](./roadmap.md#8-first-engineering-milestones-the-next-build-steps).

| # | Milestone | Status | Where |
|---|---|---|---|
| 1 | Monorepo scaffold | Rust workspace and CI done. The Android, web and schema directories arrive with their first code. | [`Cargo.toml`](../Cargo.toml), [CI](../.github/workflows/ci.yml) |
| 2 | `keel-types`: value types | Done, 2026-09-27 | [`core/crates/keel-types`](../core/crates/keel-types/) |
| 3 | `keel-events`: the signed, hash-chained event log | Done, 2026-09-27. Its schema registry was built with the first domain events, in `keel-domain`. | [`core/crates/keel-events`](../core/crates/keel-events/), [ADR-0012](./adr/0012-event-wire-format.md) |
| 4 | `keel-domain` (order, check, payment) and `keel-pricing` v0 | In progress: slices 1 to 3 of 4 built and reviewed; slice 4 in progress | [`core/crates/keel-domain`](../core/crates/keel-domain/), [`core/crates/keel-pricing`](../core/crates/keel-pricing/), [ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md), [ADR-0014](./adr/0014-pricing-engine-v0.md), [ADR-0015](./adr/0015-checks-and-payments.md) |
| 5 | `keel-store`: SQLite events, projections and outbox | Not started | |
| 6 | `keel-sim` and `keel-sync` v0 | Not started | |
| 7 | Android register shell | Not started | |
| 8 | Cloud cell v0 | Not started | |

Step 4 is split into four slices, each ending with a review. It was planned as three; the third,
checks and payments, was split in two, so that the payment design is reviewed before it is built:

1. **Foundations and the order's lines** (built and reviewed 2026-09-28): payload codecs
   and schema registry; the aggregate framework; order events, fold and commands for creating an
   order, adding, changing, removing, firing, voiding and comping lines, changing attributes, and
   voiding or abandoning the order.
2. **Pricing v0** (built and reviewed 2026-09-28): modifier pricing, discounts, US sales tax,
   rounding and allocation, the calculation trace, and the golden-basket suite.
3. **Checks and splits** (built and reviewed 2026-09-28): every order's main check,
   opening checks, allocating lines to checks in whole shares, and pricing each check as its own
   sale.
4. **Payments and closing** (in progress): the payment aggregate, balances, closing checks with
   their totals, and closing and reopening orders.

## Current slice

Slice 4, payments and closing, builds decisions 5 to 9 of
[ADR-0015](./adr/0015-checks-and-payments.md):

| Piece | Status |
|---|---|
| Closing a check, with the snapshot of what it was charged; closed checks frozen; post-close checks | Not started |
| Closing and reopening the order | Not started |
| The payment aggregate, with cash and card | Not started |
| Checkout: each check's balance, and the rules that span an order and its payments | Not started |

## Completed

### Checks and splits: step 4, slice 3 (2026-09-28)

Commit `fdd6d6f`. Reviewed 2026-09-28.

- **Built** ([ADR-0015](./adr/0015-checks-and-payments.md)):
  - every order's main check, whose identifier is the order's own, and `order.check_opened` for
    more checks, numbered in the order they are opened;
  - `order.lines_allocated`, which moves or splits lines among checks in whole shares, kept in
    lowest terms so each split has one encoding;
  - the fold's rules for concurrent splits: for each line, the later allocation wins; one naming
    a check that doesn't exist yet doesn't apply, and is reported; a check opened twice keeps
    its first opening; allocating a removed or voided line changes nothing;
  - commands to open checks and allocate lines, checked against the device's view;
  - each check's basket, which prices the check as its own sale;
  - in `keel-pricing`, a line's *share*: a basket holds its part of a line split among several,
    by largest remainder, and the trace records the split.
- **Verified:**
  - Known-answer tests, with the arithmetic worked out: 4 more in `keel-domain` (28 in all),
    among them a table split by seat with the wine shared three ways and each check taxed on its
    own; 2 more in `keel-pricing` (18 in all).
  - The two new schemas' pinned payloads were encoded independently by Python's `cbor2`, from
    the documented key tables, and the kernel's encoding matches them byte for byte.
  - The payload model covers allocations: out of order, repeated, not in lowest terms, empty, or
    with a share of zero, each accepted exactly when valid. A new property checks that
    allocations have one form, whatever order they are given in.
  - The fold model, written as queries over the events' positions, covers checks and splits,
    and a new property aims at splits. The command model covers opening checks and allocating
    lines, faulty commands included. Invariants check that every live line belongs to existing
    checks, in lowest terms; that each check's basket holds its part of each line allocated to
    it; and that the checks' parts of a line add up to the line.
  - In `keel-pricing`, the exact oracle and the independent Python oracle price shares, with 31
    new golden baskets (140 in all). A new property checks that a shared line's parts add up to
    it, each within one minor unit of its exact share, and invalid shares are refused.
  - Planted bugs, all caught, and all caught by the property tests alone: 94 in `keel-domain`,
    22 of them aimed at checks and splits, and 51 in `keel-pricing`, 6 of them aimed at shares.
  - Every crate's property tests also passed 100,000 cases each.
  - CI green.
- **Decisions:** [ADR-0015](./adr/0015-checks-and-payments.md), accepted 2026-09-28.
  - **Checks are part of the order's stream**, and every order has a main check with the order's
    identifier, so a one-check order needs no check events.
  - **Lines are split among checks in whole shares**, in lowest terms: exact, and the parts add
    up by construction.
  - **Each check is priced as its own sale**, so its tax is computed on it. A split order's
    checks can pay a cent or so more or less tax than the order would unsplit.
  - **Concurrent splits:** the later allocation of a line wins.
  - **Payments and closing**, designed now and built in slice 4: a snapshot of what each check
    was charged when it closes; closed checks frozen; post-close checks for lines added
    concurrently; the payment aggregate, with five events for cash and card; and checkout rules
    that keep a check from being charged twice.
- **Found and fixed during the build:**
  - The first generators rarely split a line among checks: a probe of 2,000 cases found only 20
    orders ending with a line shared by several checks. A new property with generators aimed
    at splits reaches that in nearly a quarter of its cases, with over a thousand allocations
    applied per 2,000 cases.
  - An old planted bug, reason codes of 33 bytes, escaped the property tests: with two more kinds
    of event, the wide generators reached a 33-byte code too rarely. A new property tries reason
    codes at every length around the limit.

### `keel-pricing` v0: step 4, slice 2 (2026-09-28)

Commit `7638563`. Reviewed 2026-09-28.

- **Built:** a pure pricing function from a basket, the order's lines as rung up, and the
  location's rules to the totals ([ADR-0014](./adr/0014-pricing-engine-v0.md)):
  - line extension: the item's price plus its modifiers', nested and counted per unit, times the
    quantity, rounded for weighed and measured items;
  - comps, line discounts and order discounts, each taking from what is left, a percentage
    rounded and an amount limited to what is left; order discounts allocated to the lines by
    largest remainder;
  - US sales tax: taxes by category, optionally only on the premises or only to go, on each line's
    net, rounded per line or once per document and then allocated; exemptions by tax;
  - totals for every line and for the order, and a step-by-step trace that explains them;
  - cash rounding, for the tender to call;
  - validation that refuses invalid input rather than guessing;
  - `Order::basket` in `keel-domain`, which turns an order's live lines into a basket, so an
    order can be priced.
- **Verified:**
  - 16 known-answer tests and a doctest, each with its arithmetic worked out, and 7 property
    tests, which also passed 100,000 cases each. A known-answer test prices a real order, and the
    order property tests check every folded order's basket against its live lines.
  - An exact oracle in arbitrary-precision integers, written from the ADR with its own rounding
    and allocation: the engine agrees on every amount, and reports an overflow exactly when an
    amount doesn't fit. Generators aim at ties at every rounding point, allocation ties
    (repeated lines), exact-amount discounts and tax rates above 100%; a probe of 20,000 cases
    confirmed each happens more than a hundred times.
  - 109 golden baskets (café, restaurant, grocery with weighed items, exemptions, yen and dinar,
    edge cases, a tie under each rounding mode), with totals from an independent Python oracle
    that uses exact fractions. CI regenerates them and fails on any change.
  - The trace replays into the totals and accounts for every tax; the parts add up; reordering
    the lines changes neither the gross, the discounts nor the net, nor, without order discounts,
    the tax.
  - 45 planted bugs in `keel-pricing` and 5 in the order's basket, all caught; the property tests
    alone catch every one.
  - CI green.
- **Decisions:** [ADR-0014](./adr/0014-pricing-engine-v0.md), accepted 2026-09-28.
  - **Snapshot prices:** pricing uses the prices recorded when a line was rung up; price lists and
    the catalog's modifier rules apply then, and come with the catalog.
  - **Five rounding points**, each with an explicit mode (half away from zero by default):
    extension, percentage discounts, allocation, tax, and cash. The domain model said three.
  - **Discounts take in turn from what is left**, so they can never take more than the line or
    the order; an amount discount larger than what is left is limited, and the trace says so.
  - **A percentage order discount is rounded once** on the order, then allocated, so it is exactly
    that percentage of the order.
  - **Discounts reduce the taxable amount; taxes don't compound.** Per-line and per-document tax
    rounding are both supported, as jurisdictions require.
  - **Lines, discounts and taxes are identified by position** in the basket and the rules.
- **Found and fixed during the build:**
  - The domain model's "rounding happens in exactly three places" missed weighed items and
    percentage discounts; it now lists five.
  - Planted bugs found three gaps in the first tests: no test at all covered an amount discount of
    exactly what is left, and the property tests alone missed the deepest allowed modifiers and
    cash rounding. Each now has a known-answer test and a property.
  - A final review found two more gaps, each confirmed by a planted bug that the property tests
    missed: the engine accepts tax rates above 100%, as some excise taxes need, but no generator
    produced one; and the trace replay didn't check that every tax is accounted for, so a trace
    that left out an exemption passed. The generators now include such rates, the replay
    accounts for every tax, and known-answer tests cover a tax above 100% and an empty basket
    (the case on which a property test once caught a bug in its own replay).

### `keel-domain` slice 1: payloads, the schema registry, and the order's lines (2026-09-28)

Commits `fbd1507`, `1b6dd56`, `147870c`, and after review `0188d4d`, `13a2847`, `16df799`.
Reviewed 2026-09-28.

- **Built:**
  - Payload codecs: strict maps with integer keys; money, quantity, identifier, text, code and
    identifier-set encodings.
  - The schema registry, with a pinned example payload for every schema. The fold entry point
    skips events of unknown schemas, malformed payloads and other streams, and reports them.
  - The canonical order of provisional events: by clock, then device, then log position.
  - The order aggregate: ten version-1 event schemas; the order's state, with a total fold that
    applies the sync design's conflict rules and reports 15 kinds of conflict; and commands
    checked against the device's view, each producing one event that must decode under its own
    schema before it is recorded.
- **Verified:**
  - 24 known-answer tests and 12 property tests. The property tests also passed 100,000 cases
    each.
  - The pinned payloads of all ten schemas were decoded independently with Python's `cbor2`, and
    match the documented key tables.
  - Independent models:
    - a validity model of every schema, against payloads with fields changed, removed or added;
    - a model of the fold, written as queries over the events' positions. It is checked on any
      events, on events aimed at the conflict rules (colliding lines, other currencies, units
      and locations), and on devices acting concurrently on stale views;
    - a model of the command rules, checked with deliberately faulty commands.
  - 64 planted bugs, all caught, and the property tests alone catch every one.
  - CI green.
- **Decisions:** [ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md), accepted 2026-09-28.
  - **Payloads** are canonical CBOR maps with small integer keys, decoded strictly. Money is
    `[minor units, currency code]`, and quantity `[millionths, unit code]`. Enumerations are
    integer codes.
  - **Schema evolution:** every payload change is a new version. Writers use a version only once
    every kernel at the location knows it (negotiated by sync), so a newer version never meets
    an older kernel in normal operation.
  - **Undecodable events** from a trusted device stay in the log, so the device's chain never
    stalls, but the fold skips them and the order reports them ("needs update", or malformed).
  - **Text** in names and notes can't contain control characters, which could otherwise drive
    receipt printers and kitchen displays.
  - **Events that would break the order don't apply:** a line or modifiers in another currency,
    a quantity in another unit, an event from before the order's creation or from another
    location. They stay in the log and leave a conflict, so every price in an order can always
    be added up. Conflicts are derived by the fold, not recorded as events.
  - **Commands** refuse no-op changes. A void needs a fired line (a pending line is removed). The
    first removal or void of a line wins, and so do the first comp and the first close.
  - **Flags** for a fired line removed rather than voided, and for lines added or fired after
    the order was closed.
  - **Stage** (Draft, Open, Submitted) is derived from the lines, not stored.
  - **A line records** the item's variant, name, tax category, unit price and the catalog
    version it was priced from.
  - **Version 1 is positive:** quantities above zero and prices of zero or more. Returns will
    have their own events.
  - `Id::cast`, to use an order's identifier as its event stream's.
- **Found and fixed during the build:**
  - Planted bugs found gaps twice. At first, 3 of 51 escaped every test: payload rules that span
    fields. A unit test and sharper generators closed that gap. The property tests alone then
    still missed 18 that unit tests caught. A full model of the fold, faulty commands, and
    generators aimed at rules that span fields or sit on a boundary (one currency per payload at
    any depth, text lengths, identifier sets out of order) closed it. The 13 planted bugs added
    for the new models were all caught.
  - `provisional_order` had no test; it now has known answers and a property.
- **After review:** abandoning an order needs that nothing in it was ever fired, so an order the
  kitchen worked on always ends up voided. A line now remembers whether it was ever fired, and the
  fold reports an order abandoned with fired lines. Seven planted bugs aimed at the rule, three of
  them new, are all caught by the property tests alone, which makes 67 for the slice.
- **After review:** every crate's planted bugs now live in `tools/planted-bugs/`, with a runner
  that anyone can rerun. Running them all again found a bug in the record: an old script counted a
  bug that didn't compile as caught (in `keel-events`). It also showed that `keel-events`'
  property tests reached three envelope edge cases too rarely: a sequence number of 0, a padded
  date, and a field under the negative of its key. A new property tries a near miss for every
  field of the envelope, and every field under its negative key, and catches all three every time.

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
  - 58 planted bugs, all caught; the property tests alone catch 53. The other 5 are the
    byte-level checks of COSE messages and keys, which known-answer tests catch. (Recounted on
    2026-09-28 with the planted-bug runner, which found that one listed bug had never compiled:
    the earlier scripts counted a compile error as a caught bug. The lists hold 58 bugs; the
    earlier record said 59.)
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
  16 planted bugs, all caught; the property tests alone, with the exhaustive sweep, catch 13, and
  unit tests the other 3 (counted on 2026-09-28).
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
- **Orders:**
  - Permissions, approvals and ownership leases aren't checked yet (`keel-policy`, `keel-sync`).
  - Lines added after an order is closed should go to a post-close check, once orders close
    (slice 4).
  - Check names, and putting a new line straight onto the check it is for (a new version of
    `order.line_added`); re-splitting a line after part of it was paid (ADR-0015).
  - Events for a person's resolution of a conflict.
  - A tool that labels payload fields, for auditors and support.
- **Pricing:** everything ADR-0014 defers: price lists and the catalog's modifier rules,
  promotions, service charges, fees and surcharges, tips, tax-inclusive prices and compound
  taxes, per-item thresholds, tax holidays, the SNAP portion, manufacturer coupons,
  destination-based tax, and returns.

## Keeping this document current

Update "Where we are" and the current slice's table as work lands. When a slice ships, move it to
"Completed" with what it built, how it was verified, its decisions and its commits. Add every
deferred item to "Known gaps", and remove it when it's done.
