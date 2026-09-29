# Build progress

> A living record of the build: where it stands, what each step delivered and how it was
> verified, and what is waiting on a decision. Updated as each piece of work lands.
> Last updated: 2026-09-29.

## Where we are

**Phase 0 (Foundations), step 5 of 8:** `keel-store`, the SQLite event store, projections and
outbox, in three slices ([ADR-0016](./adr/0016-device-store.md), proposed). Slice 1, the event
log store, is built and verified, and waiting for review. Step 4, `keel-domain` and
`keel-pricing` v0, is done: built in four slices, each reviewed.

## Phase 0 milestones

From the [roadmap](./roadmap.md#8-first-engineering-milestones-the-next-build-steps).

| # | Milestone | Status | Where |
|---|---|---|---|
| 1 | Monorepo scaffold | Rust workspace and CI done. The Android, web and schema directories arrive with their first code. | [`Cargo.toml`](../Cargo.toml), [CI](../.github/workflows/ci.yml) |
| 2 | `keel-types`: value types | Done, 2026-09-27 | [`core/crates/keel-types`](../core/crates/keel-types/) |
| 3 | `keel-events`: the signed, hash-chained event log | Done, 2026-09-27. Its schema registry was built with the first domain events, in `keel-domain`. | [`core/crates/keel-events`](../core/crates/keel-events/), [ADR-0012](./adr/0012-event-wire-format.md) |
| 4 | `keel-domain` (order, check, payment) and `keel-pricing` v0 | Done, 2026-09-29: built in four slices, each reviewed | [`core/crates/keel-domain`](../core/crates/keel-domain/), [`core/crates/keel-pricing`](../core/crates/keel-pricing/), [ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md), [ADR-0014](./adr/0014-pricing-engine-v0.md), [ADR-0015](./adr/0015-checks-and-payments.md) |
| 5 | `keel-store`: SQLite events, projections and outbox | In progress: slice 1 of 3 built, waiting for review | [`core/crates/keel-store`](../core/crates/keel-store/), [ADR-0016](./adr/0016-device-store.md) (proposed) |
| 6 | `keel-sim` and `keel-sync` v0 | Not started | |
| 7 | Android register shell | Not started | |
| 8 | Cloud cell v0 | Not started | |

Step 4 was built in four slices, each ending with a review. It was planned as three; the third,
checks and payments, was split in two, so that the payment design was reviewed before it was
built:

1. **Foundations and the order's lines** (built and reviewed 2026-09-28): payload codecs
   and schema registry; the aggregate framework; order events, fold and commands for creating an
   order, adding, changing, removing, firing, voiding and comping lines, changing attributes, and
   voiding or abandoning the order.
2. **Pricing v0** (built and reviewed 2026-09-28): modifier pricing, discounts, US sales tax,
   rounding and allocation, the calculation trace, and the golden-basket suite.
3. **Checks and splits** (built and reviewed 2026-09-28): every order's main check,
   opening checks, allocating lines to checks in whole shares, and pricing each check as its own
   sale.
4. **Payments and closing** (built 2026-09-28, reviewed 2026-09-29): the payment aggregate,
   balances, closing checks with their totals, and closing and reopening orders.

Step 5 comes in three slices, each ending with a review
([ADR-0016](./adr/0016-device-store.md), decision 8):

1. **The event log store** (built 2026-09-29, waiting for review): the device's own events and
   those it receives from other replicas, in one SQLite database; the quarantine and the version
   vector; one transaction per write; crash tests at every point of a write.
2. **Projections and the outbox**, updated in the same transaction as the events.
3. **Encryption at rest and integrity checks**: SQLCipher-class encryption with its key from the
   platform keystore, and checks when the store opens.

Retention, snapshots and backups follow the sync engine (step 6).

## Current slice

Slice 1, the event log store, is built and verified: see "Completed". It waits for review, with
[ADR-0016](./adr/0016-device-store.md), which is proposed. Slice 2, projections and the outbox,
follows.

## Completed

### The event log store: step 5, slice 1 (2026-09-29)

Commit `f9e68d7`. Waiting for review.

- **Built** ([ADR-0016](./adr/0016-device-store.md), decisions 1 to 7):
  - `keel-store`, the kernel's first platform crate: each device's events in one SQLite
    database, through `rusqlite` with SQLite compiled in, in WAL mode, with every commit on disk
    before it returns, and a versioned schema;
  - writes: `Store::write` runs a closure whose appended and received events commit together or
    not at all. Appended events leave the store only once their write has committed, and a
    failed write puts the device's log writer back to its log and clock as stored;
  - received events: verified with the device registry, and for the store's location. The next
    event of its device's log is stored, and the device's clock observes it within the drift
    limit; a duplicate changes nothing; after a gap, the caller learns where the log ends; and
    anything else goes to the quarantine, once, with one of 13 reasons. The device's own events
    from another replica move its writer and clock forward;
  - reads: a device's log from a position, each device's head, the version vector, an event by
    identifier, a stream's events in canonical order, and the quarantine;
  - opening refuses another device or location, a key that didn't sign the device's events,
    and a database a newer kernel wrote;
  - fault points (`Migrating`, `Began`, `Stored`, `Committing`, `Committed`) for crash tests and
    the simulator;
  - in `keel-events`: `LogWriter::restore` and `LogWriter::latest_hlc`, `LogHead::from_parts`,
    and `SignedEvent::from_stored`, behind a `stored` feature that only the store enables.
- **Verified:**
  - Known-answer tests: 15 in `keel-store`, each rule worked through with a few devices,
    among them the durability settings as SQLite reports them, each refusal quarantined once
    with the first event kept, a row changed on disk read back as corrupt, and the failure the
    property test found; 3 in `keel-events`.
  - A property test against a model of the store's rules. A case is up to nine writes and
    reopenings. Each write appends events and receives messages of 16 kinds: the next event of
    a log (sometimes beyond the drift limit), one received before, a fork, one after a gap,
    one that doesn't link or whose clock went back, one signed with another key, from an
    unknown, revoked or foreign device, naming another location, with a used identifier, at the
    last position a store holds or beyond, garbage, a quarantined message again, and the
    device's own events from elsewhere. Steps may share a physical time, so that devices' HLCs
    tie. A write commits, fails, or is interrupted at a point; some reopenings first open the
    store as another device, at another location, or with another key. After every step,
    everything the store reads back is checked against the model, and so is the device's clock:
    after everything it must follow, no later than it had to be, and given back by a failed
    write. The model decides each message's fate from how the test made it, never by calling
    the verifier.
  - A new property in `keel-events`: a writer restored to a stored head and clock carries on
    exactly as a new writer resumed from them, back after a failed write, on past the device's
    own events from elsewhere, or anywhere with any clock.
  - Crash tests, in a child process: an abort at every point of a six-write workload in turn,
    37 crashes; and 32 kills, each a moment after a chosen write's acknowledgement. After each,
    the store holds every acknowledged write and nothing half-written, every log verifies, the
    clock came back, the device's writer carries on, and a crash in a migration left nothing of
    it.
  - Coverage probes, over 1,000 cases: every fate of a received message in 140 to 890 cases
    (but the two catch-all reasons, which nothing reaches today);
    every ending of a write in 137 to 874; a quarantined message whose fate has changed since,
    received again, in 75; ties between devices' HLCs in a stream in 55; the last position a
    store holds and the first beyond it in 80 and 67; and each kind of stranger, with and
    without the device's own events, in 65 to 91. In the restored-writer property, 40% of
    cases restore behind the head and half behind the writer's clock, and a quarter of the
    observations are beyond the drift limit. The kills landed up to four writes after the
    chosen acknowledgement, two of 32 between a commit and its acknowledgement.
  - Planted bugs, all caught: 51 in `keel-store`, 46 of them by the property and crash tests
    alone. The other 5, three settings, a newer kernel's database and a row changed on disk,
    only the unit tests can reach. In `keel-events`, 6 new ones, in restoring a writer, caught
    by its property tests alone. The runner now skips, under `--props`, a bug marked `"unit"`,
    which only unit tests can catch; `keel-events`' five known-answer-only bugs (byte-level
    checks of COSE messages and keys) are marked so.
  - The store's property test, and every log property in `keel-events`, the restored writer's
    among them, each passed 100,000 cases. A larger crash run, not kept, passed too: an abort at
    every point of a 24-write workload (145 crashes), and 300 kills.
  - CI green.
- **Decisions:** [ADR-0016](./adr/0016-device-store.md), proposed, with the details the build
  settled under "As built":
  - the checks on a received event run in a fixed order, each refusal with a stable code;
  - the quarantine keeps a message's first reason, even when its fate has changed since;
  - a failed write gives back the clock as well as the log;
  - reading an event back checks its stored hash, not its signature.
- **Found and fixed during the build:**
  - The property test found that an interrupted write could leave the device's clock an hour
    ahead: the write had received the device's own event from elsewhere, stamped an hour
    ahead, and the rollback put back the log but not the clock. `LogWriter::restore` now takes
    both, the store restores both after any failed write, and the test checks the clock after
    every write.
  - A unit test that reopened a store with the same entropy at the same millisecond minted an
    identifier the store already held, and its write failed. The tests now reopen with new
    entropy; the simulator must give each restart its own (see "Known gaps").
  - The first kill test slept fixed delays, 5 to 405 ms, against a workload that takes 330 ms
    in a debug build, so where its kills landed depended on the machine. It now kills a moment
    after a chosen write's acknowledgement.
  - The child's first acknowledgement shared a line with the test harness's own output, and was
    missed; the parent now looks for acknowledgements anywhere in a line.
  - A quarantined message whose fate had changed came up in 2.5% of cases; aiming the
    generator at them raised it to 7.5%.
  - The first run of the planted bugs found one that no test caught: a migration committing
    before its fault point. The unit test only checked that the store opened afterwards, which
    an empty store also does. It and the crash test now check that nothing of the migration was
    kept. Three checks on opening were first unit-only; the property test now opens stores as
    strangers, so they are its to catch.

### Payments and closing: step 4, slice 4 (2026-09-28)

Commit `c78bc15`. Reviewed 2026-09-29.

- **Built** ([ADR-0015](./adr/0015-checks-and-payments.md), decisions 5 to 9):
  - closing a check: `order.check_closed` records what pricing charged the check, line by line
    and tax by tax, the version of the pricing rules, and the payments that settled it;
  - the fold's rules for closed checks: the snapshot stands; a closed check freezes what its
    lines cost and where they are paid; a line a close left off moves to an open check, or to a
    post-close check opened for it; and a conflict for each surprise;
  - closing and reopening the order: `order.closed` and `order.reopened`, and commands that
    refuse to change a frozen line, or to void or abandon an order with a closed check;
  - the payment aggregate, whose identifier is the idempotency key: `payment.initiated`,
    `payment.authorized`, `payment.captured`, `payment.failed` and `payment.voided`, for cash
    and card; a state machine that keeps money that may have moved when events arrive out of
    turn; and commands that record a payment's outcome;
  - checkout, across an order and its payments: each check's balance, starting a payment,
    closing a check, and the issues it reports.
- **Verified:**
  - Known-answer tests, with the arithmetic worked out: 21 more in `keel-domain` (49 in all),
    among them a table paying check by check (a tip, a split tender, change from a note, and
    each check's tax shared among its lines by largest remainder); a closed check freezing its
    lines; lines a close left off; post-close checks; and the payment state machine.
  - The eight new schemas' pinned payloads were encoded independently by Python's `cbor2`, from
    the documented key tables, and the kernel's encoding matches them byte for byte.
  - The payload model covers the eight new schemas. Three new properties aim at processor
    references at their length limits; snapshots with one rule broken and the others kept (a
    gross below its net, a net, tax or taxable amount at or below zero with the sums still
    adding up, lines or taxes out of order or repeated, no lines, an amount in another
    currency, a sum a unit or two off); and cash a unit or two short of what is paid.
  - The fold model follows the checks and the lines' allocations in one pass over the events,
    and queries the rest; a new property aims at closing, forged check identifiers included.
    The command model covers closing checks through checkout, with real payments, closing and
    reopening orders, and every refusal the freeze adds, and now says why a command is refused,
    not only whether. Commands are also checked against it on merged orders, and on every state
    a merge passes through, from the order's location and another: a new property races devices
    closing checks against others removing, firing or moving lines, and then a device carries
    on with the merged order.
  - The payment fold against a model written as queries (the first capture, and runs of
    outcomes before it); the payment commands against a model of their rules, which also says
    why a command is refused, with commands at the edges of the rules (a unit over the
    authorization, say) tried at every state; devices acting concurrently on one payment.
  - Checkout against a model of its rules: starting payments and closing checks refused exactly
    when the model refuses them, and for the same reason; every balance and issue after every
    action, with payments given twice and another order's payment left out; each close
    recording exactly what pricing charged and who paid; and the issues of devices working a
    table concurrently, merged and in each device's partial view.
  - Coverage probes, over 1,000 or 2,000 cases each: every new conflict in the closing
    property in 170 to 850 cases; all eight out-of-turn payment outcomes in 170 to 550; checks
    closed about 1,800 times in 1,000 runs of checkout; every issue, a payment after a close in
    240 of 1,000 concurrent tables, an underpaid check in 440 partial views; and in the race, a
    closed check whose lines were all removed in 6% of cases, and an order abandoned after its
    only line was fired in 2%.
  - Planted bugs, all caught, and all caught by the property tests alone: 220 in
    `keel-domain`, 126 of them new, aimed at closing, freezing, payments and checkout.
  - Every property test in `keel-domain` also passed 100,000 cases.
  - CI green.
- **Decisions:** [ADR-0015](./adr/0015-checks-and-payments.md), accepted 2026-09-28, with the
  details the build settled recorded under "As built", accepted after review on 2026-09-29:
  - **A closed check freezes what its lines cost and where they are paid.** Their seat, course
    and notes can change, and they are still fired, so an order paid first is still prepared.
  - **An allocation doesn't move a frozen line**, even from a device acting concurrently, since
    that would charge its paid part twice; every other change to it applies, and is reported.
  - **Lines a close left off** move to the first open check that doesn't already hold part of
    them; a new line goes to the first open check. Otherwise a post-close check opens, named
    after the event that opened it, so every replica opens the same one.
  - **Reopening** reopens every closed check with the order; a voided or abandoned order is
    final.
  - **A capture always stands.** After it, nothing changes the payment; an authorization or a
    capture after a failure or a void applies, since the card may be held or charged.
  - **Checkout also reports** a payment after its check closed, and a closed check its payments
    no longer cover.
  - **Voiding or abandoning an order with a closed check** is refused: reopen it first.
- **Found and fixed during the build:**
  - The first checkout generators closed a check only 82 times in 1,000 runs, so a payment
    after a close came up 8 times in 1,000 concurrent tables. A "settle" action, which pays a
    check in full and closes it, each step still checked against the model, raised that to about
    1,800 closes and 240 cases.
  - Two rules were out of the property tests' reach: forged check identifiers, which only a
    hostile device writes, never met the new-check fallback, and checkout never saw a payment
    given twice or another order's payment. A forgery aimed at the next event that opens a
    check, and noisy payment lists, now reach both.
  - A payment a cent short of its check came up too rarely to test the close rule's boundary;
    the generators now pay a cent short on purpose.
  - The first full run of the planted bugs found 20 that the property tests alone missed, and
    the rerun a 21st that the first had caught only by luck. The tests were strengthened until
    all were caught:
    - eight of the snapshot's rules. The snapshot property only moved an amount, which broke a
      sum as well, and never reordered or emptied a list; the property that changes any payload
      rarely reached a snapshot. The snapshot property now breaks one rule at a time, and moves
      the sums to match;
    - five refusals that a second rule backs up, such as a payment in another currency, which
      also fails the limit it can't be compared with, and closing an empty check, whose snapshot
      also fails its schema. The models of order, payment and checkout commands now say why a
      command is refused, not only whether;
    - a capture for more than was authorized came up too rarely: the payment tests now try
      commands at the edges of the rules at every state;
    - sorting a check's lines for its snapshot never mattered, because new lines always had
      rising identifiers: the checkout tests now hand them out in no particular order;
    - reopening a closed order from another location came up too rarely: commands on merged
      orders are now tried from the order's location and another;
    - abandoning an order with a closed check, and abandoning one whose only line was fired
      and then removed, happen only when devices race. The second came up often enough until
      this slice's events joined the generators. The new race property reaches both. Only a
      concurrent removal can empty a closed check, so no single device's commands reach the
      first, and a known-answer test covers it too;
    - three planted bugs couldn't change what the code does. Two removed checks that another
      rule backs up with the same result (a line's amount, and a cash rounding, in another
      currency), and are no longer listed; one targeted code that moved into `lines_on`, and
      was re-aimed.
  - 17 planted bugs no longer matched the code they target, rewritten for closing; their
    anchors were refreshed.

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
- **`keel-store`:**
  - Power-loss tests, which need keel-sim's simulated disk; until then the durability settings
    are checked as SQLite reports them.
  - Projections and the outbox (slice 2); encryption at rest and integrity checks (slice 3);
    retention, snapshots and backups, after the sync engine.
  - Releasing or discarding quarantined messages: they are kept for a person to look at, with
    no tools yet.
  - The simulator must give each restart of a device fresh entropy: a store reopened with the
    same entropy in the same millisecond mints identifiers it already holds, and its writes fail.
  - Two processes opening one store at once: a store is one process's. Opening re-checks the
    schema version inside its transaction, but no test races two openers.
- **Orders:**
  - Permissions, approvals and ownership leases aren't checked yet (`keel-policy`, `keel-sync`).
  - An order with payments on its open checks can be voided, and checkout reports the payments:
    refusing it needs the payments, and refunds come with returns.
  - Check names, and putting a new line straight onto the check it is for (a new version of
    `order.line_added`); re-splitting a line after part of it was paid (ADR-0015).
  - Events for a person's resolution of a conflict.
  - A tool that labels payload fields, for auditors and support.
- **Payments:** everything ADR-0015 defers: refunds, returns and disputes; store-and-forward
  and asynchronous rails; tip adjustment and incremental authorization; stored value, house
  accounts and external accounts; surcharges; receipt details such as card brand, entry mode and
  EMV data, which come with the first connector. A check that closes again after a reopening is
  priced by the rules then in force. Starting a payment checks its identifier against the
  order's own payments only: one that another order's payment already has (which only a bug or
  a forgery could produce, since identifiers are random) is left to that payment's fold, which
  reports a second initiation.
- **Pricing:** everything ADR-0014 defers: price lists and the catalog's modifier rules,
  promotions, service charges, fees and surcharges, tips, tax-inclusive prices and compound
  taxes, per-item thresholds, tax holidays, the SNAP portion, manufacturer coupons,
  destination-based tax, and returns.

## Keeping this document current

Update "Where we are" and the current slice's table as work lands. When a slice ships, move it to
"Completed" with what it built, how it was verified, its decisions and its commits. Add every
deferred item to "Known gaps", and remove it when it's done.
