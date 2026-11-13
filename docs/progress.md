# Build progress

> A living record of the build: where it stands, what each step delivered and how it was
> verified, and what is waiting on a decision. Updated as each piece of work lands.
> Last updated: 2026-11-12.

## Where we are

**The MVP, steps 1 to 5 of 10, are done.** Step 1, staff, roles and approvals
([ADR-0029](./adr/0029-staff-roles-and-approvals.md)); step 2, selling at the register
([ADR-0030](./adr/0030-selling-at-the-register.md)): discounts with reasons and limits, prices
set by hand, line notes, split checks and partial cash payments, and orders parked on one
register and recalled on another through the hub; step 3, cash
([ADR-0031](./adr/0031-cash-drawers-and-day-reports.md)): drawer sessions with a float, paid in,
paid out, safe drops and no-sales, a blind close, and X and Z reports from local data; step 4,
returns and refunds ([ADR-0032](./adr/0032-returns-and-refunds.md), accepted after its review):
a paid order found from its receipt, lines returned whole or in part, each refunded what was
paid for it and never more, in cash out of the drawer; step 5, availability
([ADR-0033](./adr/0033-availability.md), accepted after its review): an item or a modifier 86'd
for the day or until put back on any register, sold out on every register of the store within a
second through the hub, and refused when rung. Each in the kernel, the bindings and the Android
register. Step 6, the kitchen, is next.

**CI is out of minutes for the month**: the work since is verified here as CI would verify it,
but for the app's instrumented tests, which need an emulator. Step 3 changed them, and fixed
three that had failed in CI since step 2: they run when CI does.

**Phase 0 (Foundations) is built: all 8 steps**, each reviewed, its designs accepted, ADR-0012 to
ADR-0028. What remains of it waits for the reference device, as do the performance budgets that
decision gate G1 weighs.

## Phase 0 milestones

From the [roadmap](./roadmap.md#8-first-engineering-milestones-the-next-build-steps).

| # | Milestone | Status | Where |
|---|---|---|---|
| 1 | Monorepo scaffold | Rust workspace and CI done. The Android, web and schema directories arrive with their first code. | [`Cargo.toml`](../Cargo.toml), [CI](../.github/workflows/ci.yml) |
| 2 | `keel-types`: value types | Done, 2026-09-27 | [`core/crates/keel-types`](../core/crates/keel-types/) |
| 3 | `keel-events`: the signed, hash-chained event log | Done, 2026-09-27. Its schema registry was built with the first domain events, in `keel-domain`. | [`core/crates/keel-events`](../core/crates/keel-events/), [ADR-0012](./adr/0012-event-wire-format.md) |
| 4 | `keel-domain` (order, check, payment) and `keel-pricing` v0 | Done, 2026-09-29: built in four slices, each reviewed | [`core/crates/keel-domain`](../core/crates/keel-domain/), [`core/crates/keel-pricing`](../core/crates/keel-pricing/), [ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md), [ADR-0014](./adr/0014-pricing-engine-v0.md), [ADR-0015](./adr/0015-checks-and-payments.md) |
| 5 | `keel-store`: SQLite events, projections and outbox | Done, 2026-09-30: built in three slices, each reviewed | [`core/crates/keel-store`](../core/crates/keel-store/), [ADR-0016](./adr/0016-device-store.md), [ADR-0017](./adr/0017-projections-and-outbox.md), [ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md) |
| 6 | `keel-sim` and `keel-sync` v0 | Done, 2026-10-02: built in four slices, each reviewed | [`core/crates/keel-sync`](../core/crates/keel-sync/), [`core/crates/keel-sim`](../core/crates/keel-sim/), [ADR-0019](./adr/0019-replication-and-deterministic-simulation.md), [ADR-0020](./adr/0020-hub-sequencing.md), [ADR-0021](./adr/0021-ownership-leases.md), [ADR-0022](./adr/0022-hub-election-and-failover.md) |
| 7 | Android register shell | Done, 2026-11-10: built in five slices, each reviewed; the reference device's numbers wait for the device | [`core/crates/keel-runtime`](../core/crates/keel-runtime/), [`apps/android`](../apps/android/), [ADR-0023](./adr/0023-register-shell.md) to [ADR-0027](./adr/0027-android-app.md) |
| 8 | Cloud cell v0 | Done, 2026-11-10: built in four slices, reviewed | [`core/crates/keel-cloud`](../core/crates/keel-cloud/), [ADR-0028](./adr/0028-cloud-cell-v0.md) |

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

Step 5 was built in three slices, each ending with a review
([ADR-0016](./adr/0016-device-store.md), decision 8):

1. **The event log store** (built and reviewed 2026-09-29): the device's own events and
   those it receives from other replicas, in one SQLite database; the quarantine and the version
   vector; one transaction per write; crash tests at every point of a write.
2. **Projections and the outbox** (built and reviewed 2026-09-29): order and payment
   projections recomputed in each write's transaction, rebuilt from the log when their version
   changes; and an outbox of effects that commit with the events that cause them.
3. **Encryption at rest and integrity checks** (built and reviewed 2026-09-30): SQLCipher
   encryption with a key the platform protects, a store that fails closed when damaged, cheap
   checks when it opens, and a full check on request.

Retention, snapshots and backups follow the sync engine (step 6).

Step 6 was built in four slices, each ending with a review
([ADR-0019](./adr/0019-replication-and-deterministic-simulation.md), decision 8):

1. **Replication and the simulator** (built and reviewed 2026-09-30): `keel-sync`'s protocol v0, anti-entropy
   by version vector over any transport, and `keel-sim`, devices, the hub and the cloud with
   real stores under seeded faults, checking the protocol's rules and the invariants.
2. **Hub sequencing** (built 2026-09-30, reviewed 2026-10-01,
   [ADR-0020](./adr/0020-hub-sequencing.md)):
   `store_seq` per epoch, confirmed and provisional events, store durability, and the cloud's
   durable-ack watermark.
3. **Ownership leases** (built and reviewed 2026-10-01,
   [ADR-0021](./adr/0021-ownership-leases.md)): the hub grants and transfers orders' ownership;
   island mode and the manager's override.
4. **Hub election and failover** (built 2026-10-01, reviewed 2026-10-02,
   [ADR-0022](./adr/0022-hub-election-and-failover.md)): priorities, heartbeats, the hot standby,
   epochs and fencing, and a split brain healing.

## Current slice

The MVP's step 6, the kitchen: routing to stations, kitchen tickets and a KDS, ticket aging,
fallbacks (D-01…D-03, D-06, D-07, D-11) ([ADR-0034](./adr/0034-the-kitchen.md), proposed), is
built in three slices, each verified as it lands, then reviewed together:

1. **The domain and the store** (built 2026-11-13): profile format 3 (stations, routes,
   allergens), routing, the kitchen ticket aggregate and its five schemas, the projection.
2. **The register and the kitchen**: firing, the tickets, voids and their slips, the KDS's
   queries and intents, the fallback print jobs and the kitchen ticket's document, the bindings.
3. **The shell**: the Android register's Send and voids, and the app as a KDS.

Next: slice 2.

Built in slice 1:

- **The profile, format 3** (`keel-domain`): the kitchen (stations, each expo or not, shown on a
  screen or not, with a printer or not; routes by item, modifier and order mode; an optional
  default station; aging thresholds; a fallback printer) and the catalog's allergens. Format 3
  is written only for a profile with a kitchen or allergens, and allergens only in it, so a
  profile without them is byte for byte as before, its catalog's version too, and a kernel
  before format 3 refuses one whole. The demo café is format 3: a bar, a kitchen and expo, its
  water and beans to expo alone. The cloud's catalog editor keeps each item's allergens.
  - Routing (`Profile::route`): a line to every station a route names for its item in its
    order's mode, or the default; a modifier a route names, at any depth, to its station as a
    component; every expo station whole.
  - Known answers: the demo's routes, a component, a route by mode, a default, no kitchen; every
    rule a kitchen breaks; format 3 for a kitchen or allergens alone; the demo re-pinned from
    the independent `cbor2` builder, extended with the kitchen and allergens.
  - Properties: routing against a model of the decision's words; random profiles with kitchens
    and allergens round-trip, and every change to their encodings is refused or canonical.
- **Kitchen tickets** (`keel-domain`): `kitchen.ticket_sent`, `_shown`, `_bumped`, `_recalled`,
  `_rerouted`, v1, pinned as `cbor2` encodes them. The first sending, showing and reroute
  stand, and the latest bump or recall; an event before the sending or at another location
  doesn't apply; a ticket both shown and rerouted is reported. Commands refuse what the fold
  reports and what changes nothing, a showing after a reroute among them.
  - Properties: the fold against a model of the rules; a command accepted exactly when its event
    changes the ticket and reports nothing; the payload model knows the five schemas and a
    ticket's items.
- **The store's `kitchen_tickets` projection**: open tickets, a day's, an order's, those whose
  fallback is due, the rerouted, and a version that moves with every kitchen event.

Waiting on CI's minutes: the Android app's instrumented tests, steps 3, 4 and 5's among them, and
the three of step 2 the scrolling ticket pane should mend.

## Completed

### Availability: MVP step 5 (2026-11-13)

The MVP's step 5, availability: a global 86 in-store, within 10 s (G-06)
([ADR-0033](./adr/0033-availability.md), accepted after its review), was built in three slices,
each verified as it landed, then reviewed together:

1. **The domain and the store** (built 2026-11-13): `availability.changed`, its fold and
   commands, the projection.
2. **The register** (built 2026-11-13): the runtime's 86 and un-86, the menu's availability,
   the refusal when ringing, the bindings, the propagation test.
3. **The shell** (built 2026-11-13): the Android register's 86.

Built in slice 1:

- **`keel-domain`'s availability**: an aggregate per item, its one schema `availability.changed`
  (v1: kind, available, until, reason), written by any device; the latest change in canonical
  order stands, so concurrent 86s and un-86s resolve alike everywhere. A change at another
  location doesn't apply; one naming the other kind applies; both are reported. Sold out at a
  time is a query of the change that stands. Commands refuse a change that changes nothing, an
  86 ending by its own time, the other kind, another location, and an item put back until a
  time. A time is encoded as microseconds since the Unix epoch.
  - Known answers: an 86 until 9:00 put back by another device and 86'd again; each conflict and
    refusal; the payload pinned as `cbor2` encodes it.
  - Properties: the fold against a model of positions, and sold out at any time; the commands
    against a model of the rules; the payload model knows the schema, with booleans and times.
- **`keel-store`'s `availability` projection**: each item's standing change, the items sold out
  at a time, and a version (the number of availability events) that moves with every change,
  whatever its HLC; rebuilt from the events, no new store schema.

Built in slice 2:

- **`keel-runtime`'s availability**: `eighty_six` until the business day ends (the next day's
  start, by the profile's cutoff), a time, or put back, and `put_back`, each gated as
  `item.availability`, decided before an approval is asked and again inside the write;
  `sold_out` with when each is back, in the location's time; `availability_version`. Ringing
  refuses a sold-out item, or a modifier chosen at any depth; a line rung before stays. The menu's
  buttons and the modifier sheet's options say what is sold out, and until when.
  `Profile::modifier` finds a modifier in any group.
  - Known answers: an 86 for the day refusing a ring until 4 am and not a minute later, with no
    event; until put back, a week on; until a time, and one already past; oat milk 86'd in the
    sheet, a latte rung before it staying; ketchup under a hash brown under a sandwich; the gate
    with and without the role, its approval spent; an item the catalog lacks, or of the other
    kind.
  - Propagation (`tests/sync.rs`): an 86 on the hub is sold out on the register 1 s later in the
    runtime's time, with no push from the shell, and a put-back on the register reaches the hub as
    fast; a shell polling every 2 s shows it within 3 s, inside G-06's 10. 86s made on both at
    once end the same on both.
  - 12 planted bugs, all caught.
- **The bindings**: `eighty_six` (variant or modifier; for the day, until put back, or until a
  time; an optional reason and note), `put_back`, `sold_out` and `availability_version`; the
  menu's buttons and the sheet's options carry `sold_out` and `back_at`. Sold out and refused
  changes are `Refused`, an unknown item `Catalog`. Known answers through the register object;
  5 planted bugs, all caught.

Built in slice 3:

- **The Android register**: an 86 button turns menu taps into marking: a tap opens the item's 86
  sheet, the variant and each modifier its item offers, 86'd today or until put back, or put
  back, once per tap however fast. Sold-out buttons are greyed with when they are back, a line
  above the menu lists what is sold out, and a tap on one is refused at once, no order begun. The
  modifier sheet disables a sold-out modifier, and drops one chosen when another register's 86
  arrives. The model reads the availability version and what is sold out every 2 s, with the
  sync status, and reads the menu, the open sheet and the 86 sheet again when either moved: an
  86 whose time passes moves no version, so the list is compared too. An 86 beyond the role asks
  a manager.
  - Model tests against the fake till, which refuses and reports as the kernel does: an 86 and a
    put-back from the menu, a double tap, the 86 sheet's modifiers, another register's 86 reaching
    the menu and the open sheet, an 86 expiring, the approval. An instrumented test 86s the
    bottled water until put back and puts it back; it waits for an emulator.
- **Verified**: `cargo fmt`, clippy with all features and none, the docs, every workspace test
  with the exhaustive sweeps, the wasm build, the planted bugs' staleness check, the Kotlin
  bindings' tests, `:register:jvmTest`, `:kernel:testDebugUnitTest` and the app's builds.

The review (2026-11-13), the kernel and the Android shell each by a reviewer who hadn't built
them; ADR-0033's As built lists what it found and what remains. Fixed, each with a test:

- **Ringing**: more of a line sold out since it was rung is refused, for its variant or any
  modifier on it; fewer, or as many, is not.
- **The runtime**: an 86's day's end is that of the change's own time, read once; what is sold out
  is looked up in the catalog by identifier, whatever kind the change names; the re-decision
  inside the write appends what it decides.
- **Tests**: the propagation test holds the hub to 1 s and checks the later change stands, 86
  against 86, 86 against put-back, and the reverse; an 86 for the day at 2 am, and on the night
  the clocks go back.
- **The register**: a poll applies what it reads to the state as it is, keeping choices made on
  the sheet meanwhile and the error shown, and says nothing when a read fails; a tap made
  marking never rings; an item 86'd with its sheet open closes it, saying so; the 86 sheet works
  until it shows the change; the sheets show their refusals; a publication refreshes the open
  sheets; the fake till refuses an unknown item put back; the instrumented test opens the Food
  page, 86s the water for the day, and puts back what a run before left.
- **Planted bugs**: 3 more in `keel-runtime`, each caught, and 3 re-aimed at the changed code.
- **Verified**, as CI would, CI's minutes being spent: `cargo fmt`, clippy with all features and
  none, the docs, every workspace test with the exhaustive sweeps, the wasm build, the planted
  bugs' staleness check, the Kotlin bindings' tests, `:register:jvmTest`,
  `:kernel:testDebugUnitTest` and the app's builds. The instrumented test waits for an emulator.


### Returns and refunds: MVP step 4 (2026-11-13)

The MVP's step 4, returns and refunds, linked to the original lines (B-09 basic, I-08)
([ADR-0032](./adr/0032-returns-and-refunds.md), accepted after its review), was built in three
slices, each verified as it landed, then reviewed together:

1. **The domain** (built 2026-11-13): `order.lines_returned`, the refund aggregate and
   `drawer.cash_refunded`, their folds and commands; the store's `drawers` projection's version 3.
2. **The register** (built 2026-11-13): the projections, the runtime's lookup, return and
   refund, the refund receipt, the reports, the bindings.
3. **The shell** (built 2026-11-13): the Android register's returns.

Built in slice 1:

- **`keel-domain`'s returns**: `order.lines_returned` (v1), on a closed order, under its
  ownership: each line once, live, in its own unit, no more than is left of it; each refunded its
  share of what its closed checks' snapshots charged it, its net and tax, of what is returned so
  far, rounded half up, so that partial returns add up and the last refunds exactly what is
  left. The fold applies a return and reports one on an order that isn't closed, beyond a line's
  quantity or charge, of an unknown line, in another unit or currency, or with a refund used
  before; and a reopening after a return, which commands refuse.
- **`keel-domain`'s refunds**: an aggregate of `refund.initiated` and `refund.completed` (v1),
  single-writer; a completion by another device, or beyond the start, applies and is reported.
- **`drawer.cash_refunded`** (v1): cash handed back, which what the drawer should hold leaves
  out, once per refund. The store's `drawers` projection's version 3 keeps each session's refunds.
  - Known answers: three at $4.00 discounted to $10.00 with $0.89 of tax, returned one at a time
    for $3.33, $3.34 and $3.33, and $0.30, $0.29 and $0.30; a line shared by two checks; a comp
    refunding nothing; every refusal and conflict; the four payloads pinned as `cbor2` encodes
    them; every rule a payload breaks.
  - Properties: returns and reopenings by command against a model with its own arithmetic, on
    closed orders of shared and unshared lines in two units; returns and reopenings folded, by
    the owner or another device, against a model; the refund's fold and commands against models;
    the drawer's models know refunds; the payload model knows the four schemas.
  - Planted bugs: 20 in `keel-domain` and 2 in `keel-store`; three re-aimed that reopening's
    refusal had moved.

Built in slices 2 and 3:

- **`keel-store`'s `refunds` projection**: a refund's order, tender, amount, state, business
  date and completion, found by order and by date; rebuilt from the events, no new store schema.
- **`keel-runtime`**: `find_orders` by the receipt's short code, closed orders only;
  `returnable`, each live line with what is left of it and what it was charged; `return_lines`,
  gated as `refund.issue` for what it refunds, refusing an order a card paid until card refunds
  arrive, which records the return, the refund started and completed, the drawer's cash handed
  back (rounded as the location rounds cash), the drawer's kick and the refund's receipt in one
  write; a return that refunds nothing pays nothing back and opens no drawer. The drawer's close
  and the day's reports list refunds; a day's reports list each tender's refunds, by the day each
  was made, and the cash refunded with no drawer open.
  - Known answers: three espressos returned one, then two, for $3.54 and $7.08, $10.62 in all,
    each approved by a manager; $3.54 handed back as $3.55 where cash rounds to five cents; a free
    line refunding nothing; an open order and an unknown one refused; a role refunding up to
    $5.00; the day's refunds, one with no drawer.
- **`keel-ffi`**: `find_orders`, `returnable`, `return_lines` (a count, or all that is left),
  the reports' refunds, the drawer's; tested through the register.
- **The Android register**: a paid order found by its receipt code, or from today's orders; a
  return page picking each line, one at a time or whole for one sold by measure, a reason, the
  refund, and what to hand back; refunds on the drawer page and the reports. Model test: a tea
  returned once a manager approves, no more than is left. An instrumented test, for CI's
  emulator: an espresso sold and returned.
- **Verified**: `cargo fmt`, clippy with all features and none, the docs, every workspace test,
  the wasm build, the planted bugs (9 more in `keel-runtime`, each caught), the Kotlin bindings'
  tests, `:register:jvmTest`, `:kernel:testDebugUnitTest` and the app's builds.

The review (2026-11-13), the kernel and the Android shell each by a reviewer who hadn't built
them; ADR-0032's As built lists what it found and what remains. Fixed, each with a test:

- **Cash**: handed back rounded over the order's refunds and capped at the cash it took ($3.55,
  $3.55 and $3.50 back for three espressos paid as $10.60, not $10.65).
- **Shares**: a return refunds its share less what was refunded, so returns worked out on two
  devices still add up to the charge.
- **The gate**: asked for as all the order's refunds, so a return split in parts passes no limit
  part by part.
- **Paid in cash**: only if every payment the closed checks name is here, captured, in cash.
- **The write**: the order decided again inside it.
- **Reports, bindings, the register**: a refund handing back nothing isn't drawerless; a count of
  a measured line refused; a double tap refunds once; a receipt's code is six characters, and
  two orders with one are listed to choose from; a reason is chosen; picks kept to what is left
  after a refusal; Return hidden on another register's order; errors shown once.
- **Planted bugs**: 5 more, each caught; one dropped as equivalent to the code (the share of all
  of a line is all of it), and four re-aimed.
- **Verified**, as CI would, CI's minutes being spent: `cargo fmt`, clippy with all features and
  none, the docs, every workspace test with the exhaustive sweeps, the wasm build, the planted
  bugs' staleness check, the Kotlin bindings' tests, `:register:jvmTest`,
  `:kernel:testDebugUnitTest` and the app's builds. The instrumented tests wait for an emulator.


### Cash: MVP step 3 (2026-11-12)

The MVP's step 3, cash ([ADR-0031](./adr/0031-cash-drawers-and-day-reports.md), accepted after
its review), was built in four slices, each verified as it landed, then reviewed together:

1. **The drawer session** (built 2026-11-12): five v1 schemas, the fold, the commands, and the
   store's `drawers` projection.
2. **Drawers at the register** (built 2026-11-12): the runtime's intents, cash counted into the
   drawer as it is taken, the gates, the drawer's report, the bindings.
3. **X and Z reports** (built 2026-11-12): the runtime's reports and their printing, the
   bindings.
4. **The shell** (built 2026-11-12): the Android register's drawer and reports.

Built in slice 1:

- **`keel-domain`'s `drawer`**: a single-writer aggregate. `drawer.opened` (a float of zero or
  more), `drawer.cash_taken` (a payment's cash, more than zero), `drawer.cash_moved` (paid in,
  paid out or dropped, with a reason), `drawer.no_sale`, and `drawer.closed` (the count, zero or
  more, and what was expected), each v1. What it should hold is its float and the cash taken
  and paid in, less what was paid out and dropped. The fold counts every cash: an event from
  another device, or after the close, applies and is reported; cash taken twice for a payment,
  an amount in another currency, an event before the opening or elsewhere, a second opening or
  close, don't apply and are reported. Commands refuse each.
  - Known answers: a session from $150.00 to an $0.39 short close; every conflict, each named;
    every refusal; the five payloads pinned as `cbor2` encodes them; every rule a payload breaks.
  - Properties: the fold against a model reading the events as queries over their positions;
    the commands, faulty ones included, against a model of the rules; the payload model knows
    the five schemas, changed on their own.
  - Planted bugs: 11, every one caught.
- **`keel-store`'s `drawers` projection**: a session's device, state, business date, float,
  sums, no-sales and close, found by device and state and by date; rebuilt from the events, no
  new store schema; the golden stores open and build it. Known answers, including a session
  closed after midnight staying on the day it opened. Planted bugs: 3, one caught only once the
  test closed a session on the next day.

Built in slices 2 and 3:

- **`keel-runtime`**: `open_drawer` (one at a time), `pay_in`, `pay_out` (gated as
  `drawer.paid_out` for its amount), `drop_cash`, `no_sale` (gated as `drawer.no_sale`, which
  kicks the drawer), and `close_drawer`, blind: what the drawer should hold isn't shown until
  the count is recorded, and a variance is gated as `drawer.variance` for its size; its report
  prints. Every cash payment's cash goes into the device's open drawer in the payment's write;
  with none open, the sale goes on. `day_report` and `print_report`, X or Z, for a business day:
  orders, gross, discounts, comps, net, tax and total, each tender, voids (orders voided, or
  dropped once they had lines), each drawer session of the day, and cash with no drawer open; a
  Z waits for the device's drawers of the day to close.
  - Known answers: a $150.00 float, two cash sales, $20.00 in, $100.00 dropped, $4.00 and $6.00
    out each approved, $67.08 expected, $0.08 short; a $5.01 short close waiting for a manager;
    a no-sale gated and kicking the drawer; a day of four sales, two discounted ($13.44 in all),
    one with no drawer open, and a void; the Z waiting for the close; the printed report.
- **`keel-ffi`**: the drawer's intents and view, the day report and its printing; a drawer
  opened, taking cash, paid out on approval, closed and reported through the register.
- **Verified**: `cargo fmt`, clippy with all features and none, the docs, every workspace test.
- **Planted bugs**: 13 in `keel-runtime`. Two went uncaught at first, each now caught by a test
  of its own: the drawer taking a payment's amount rather than the cash received once rounded
  ($3.54 is $3.55 in cash), and a short close asking for approval of its signed variance rather
  than its size.

Built in slice 4:

- **The Android register**: after sign-in with no drawer open, a banner offers to open one with
  a float, never in a sale's way; the drawer page shows what moved through the drawer, pays in,
  pays out (a manager approving beyond the role), drops, opens with no sale, and closes blind
  with a keypad count, then shows the close and its variance; today's X and Z reports read and
  print. `KernelTill` maps the bindings; `FakeTill` keeps a drawer for the model's tests.
  - Model tests: a drawer opened with a float, taking a cash sale and closed blind; a paid-out
    beyond the cashier waiting for a manager.
  - Instrumented test, for CI's emulator job: a $100.00 float, an espresso in exact cash, a
    blind count of $103.54, exact, and the X report.
- **Verified**: `:register:jvmTest`, `:kernel:testDebugUnitTest`, `:app:assembleDebug` and
  `:app:assembleDebugAndroidTest`.

The review (2026-11-12), the kernel and the Android shell each by a reviewer who hadn't built
them; ADR-0031's As built lists what it found and what remains. Fixed, each with a test:

- **The blind close**: an open session's view, on the register and on a report, has its float
  and no-sales alone; the bindings' sums are optional, set once it is counted.
- **Reports**: tenders are of every payment taken on the day, of any order, by the payment's own
  business date, cash rounding included; which cash a drawer took is projected per payment
  (`drawer_payments`, the `drawers` projection's version 2), so a session opened the day before
  counts; a Z waits for every open session of the device; an order in another currency is left
  out before it is priced. Known answers: an order rung one day and paid the next, $1.00 toward
  an order still open, the drawer of the day before holding the day's cash, $3.54 rounded to
  $3.55.
- **The drawer at the register**: a session the register can't work here, as one opened before
  the location's currency changed, is left open and another opened, and a drawer's refusal
  never refuses a sale; amounts that overflow are refused by the commands and reported by the
  fold as overflows.
- **The Android register**: a movement of nothing refused before the kernel sees it; each
  purpose's own reason, a no-sale's chosen; the count read once its digits are in, as the price
  sheet's now; reports cleared when the drawer page shows again or cash moves; the Z not offered
  with the drawer open; comps and each drawer on the report; the banner kept until a drawer
  opens. The fake till refuses and gates as the kernel does. Model tests: a short close and a
  no-sale each approved, a count tapped closed before its digits are in, a drop of nothing.
- **CI's emulator job had failed since step 2**: the ticket pane didn't scroll on a phone, and
  three tests tapped buttons below the screen. The pane scrolls; the tests scroll to what they
  tap. Five of `keel-ffi`'s planted bugs, gone stale in step 2, failed CI's staleness check from
  then on too: each is aimed again, and caught.
- **Verified**, as CI would, CI's minutes being spent: `cargo fmt`, clippy with all features and
  none, the docs, every workspace test with the exhaustive sweeps, the wasm build, the planted
  bugs' staleness check, the Kotlin bindings' tests, `:register:jvmTest`,
  `:kernel:testDebugUnitTest` and the app's build with its instrumented tests. Those tests wait
  for an emulator: none runs here.

### Selling at the register: MVP step 2 (2026-11-11 to 2026-11-12)

The MVP's step 2, selling at the register
([ADR-0030](./adr/0030-selling-at-the-register.md), accepted after its review), was built in four
slices, each verified as it landed, then reviewed together:

1. **Discounts** (built 2026-11-11): the schemas, the fold and the baskets, the closing
   snapshot, the gate, the bindings, the Android register's discount sheet, and the receipt.
2. **Price overrides and notes** (built 2026-11-11): the schema, the fold, the gate, the notes
   intent, the bindings, and the Android register's price and notes sheets.
3. **Split checks and partial payments** (built 2026-11-12): the runtime's splits, moves and
   payments toward a check, tickets built check by check, a receipt per check, the bindings,
   and the Android register's checks.
4. **Park and recall across devices**, through the hub (built 2026-11-12): tickets naming their
   owner, the runtime's recall and take-over, the bindings, and the Android register's list.

Built in slice 4:

- **`keel-runtime`**: an open order stays parked on the device that owns it, and every device
  lists the location's orders, each ticket naming its owner and the devices whose requests wait
  for the hub. `recall` asks the hub for an order another device owns, and the device owns it
  once the hub's grant comes back through replication; one owned here, or asked for already,
  asks nothing. `take_over` takes an order without the hub on a manager's word, gated as
  `order.void`; the domain flags it for reconciliation (ADR-0021).
  - Tests, across a hub and a register replicating: an order rung on one, listed on the other
    as the first's, refused changes there, recalled, granted, then rung on and paid; another
    taken over once a manager approves, refused a second time with no approval asked, and the
    hub learning of it. Planted bugs: 4.
- **`keel-ffi`**: a ticket's owner and the devices asking, `recall`, `take_over`, and the
  register's device identifier.
- **The Android register**: today's orders show one parked on another register; opening it
  recalls it, and it opens once the hub grants it; Take over asks a manager. Tests: the model's
  recall, waiting then granted, and a take-over approved.

Built in slice 3:

- **`keel-runtime`**: `split_evenly`, every live line split evenly among two to twenty checks,
  the order's open ones and as many new as it takes; `move_line`, a line moved whole to an open
  check or a new one; `pay_check_cash`, cash toward one check, part of what it owes or all of
  it with the location's cash rounding. A check closes once its payments cover it, and its
  receipt prints as that check's ("Check 2"), with its own payments; the order closes with its
  last check that holds something. `pay_cash` pays the main check whole. Tickets are built check
  by check, from the snapshot of a closed one or by pricing an open one's basket, and the
  order's amounts are the checks' added up, so they are what customers pay; a split order's
  ticket lists each check with its own lines and amounts.
  - Known answers: two $3.25 espressos three ways as $2.17, $2.17 and $2.16, $2.36, $2.36 and
    $2.35 with tax, the order $7.07 where priced whole it would be $7.08; $1.00 of the first,
    then the rest from $5.00, $3.64 back; more than a check owes refused; a closed check holding
    the split; each check's receipt, its number and its payments; the order closing with the
    last; a line moved to a check of its own and back; splits of one way, more than twenty, or
    nothing, refused.
  - Planted bugs: 8, and 9 of the earlier ones re-aimed at the rewritten ticket and cash paths:
    all 17 caught, once a check's receipt showing every payment of the order was: the test now
    looks at the last check's receipt too, which the first's payments must stay off.
- **`keel-ffi`**: the three intents, and a split order's checks crossed field for field.
- **The Android register**: Split 2, 3 or 4; each check with its total and a Pay button; a line
  moved to another check or its own; the tender for a check pays all of it, or what was typed as
  a part. Tests: the model's split paid part by part, then check by check, closing with the
  last; a line moved and back; on an emulator, two espressos split two ways and paid.

Built in slice 2:

- **`keel-domain`**: `order.line_repriced` v1, a line priced again at a unit price of zero or
  more, with a reason and an optional note; the line keeps the price it was rung at, and prices
  at the new one, with its modifiers, from then on, the last that applies winning. The fold
  reports a price for a removed line or in another currency, neither applying, and one on a
  closed check, applying; commands refuse those and a price that changes nothing.
  - Known answers: two at $3.95 instead of $4.50; free; every conflict and refusal, each named;
    the payload pinned as `cbor2` encodes it.
  - Properties: both order models generate and model prices set by hand, at prices that
    sometimes match the rung one; the payload model knows the schema; the basket invariants
    price each line at its own unit price.
  - Planted bugs: 9.
- **`keel-runtime`**: `reprice_line`, gated as `price.override` for the difference over the
  whole quantity, after the order's own rules; `set_line_notes`, setting or clearing a pending
  line's notes. Tickets show a line's price as set, as rung, and why, and its notes.
  - Known answers: a cashier's $2.75 for two $3.25 espressos approved for $1.00; a manager's
    $3.00 alone; free, and less than free refused; the same price asking for no approval; notes
    set, the same refused, cleared. Planted bugs: 3, every one caught; with slice 1's two
    re-aimed, caught again.
  - **The receipt** prints a line's notes under it, its line breaks as spaces. Known answer:
    "one extra hot, no lid" under two espressos. Planted bugs: 1.
- **`keel-ffi`**: both intents, and a line's price and notes crossed field for field.
- **The Android register**: a price sheet (reasons, a price typed) and a notes sheet for each
  line; a line's price and notes on the ticket. Tests: the model's price approved by a manager,
  notes kept, unchanged and cleared; on an emulator, a cashier's price approved, then a note.
- **Verified**: `cargo fmt`, clippy with all features and none, the docs, every workspace test,
  the wasm32 build; the Android register's JVM tests, the kernel's unit tests, the app and its
  instrumented tests built. The domain's 9 planted bugs and the runtime's 5, every one caught.

Built in slice 1:

- **`keel-domain`**: `order.discount_applied` v1, a percentage (1 to 10,000 basis points) or an
  amount (more than zero) off a line, or a percentage off the whole order, with a reason code
  and an optional note; and `order.discount_removed` v1. The fold keeps each line's discounts and
  the whole order's, in the order applied; a discount's identifier names it for good, even once
  it is taken off. A discount on a removed line or in another currency doesn't apply; one on a
  line with a part on a closed check, or on the whole order once it is closed or a check is,
  applies and is reported; taking off one never applied is reported, and one taken off already,
  as two devices may, is not. Commands refuse all of these. Baskets carry the discounts to
  pricing; an amount off a line shared among checks is split among them as the line's gross is.
  `Order::discount_effect` prices what a discount would take, and the percentage it counts as:
  an amount's share of what is left of its line, rounded up.
  - Known answers: a line's 2.50 and 10%, then 20% off the order, priced to 9.40; a removal;
    1.01 off a shared line split 0.51 and 0.50; every refusal and every conflict, each named;
    the effect of each kind, a comped line's, and a removed line's none; both payloads pinned
    as `cbor2` encodes them, an order-wide percentage too, and every cross-field rule broken.
  - Properties: both order models (the command model and the fold model) now generate and model
    discounts, so every merge, race and refusal they check covers them; the basket invariants
    check each line's discounts and the order's, and that the checks' parts of an amount off a
    shared line add up to it; the payload model knows both schemas, with discounts changed on
    their own so each field is tried often.
  - Planted bugs: 22, every one caught.
- **`keel-runtime`**: `apply_discount`, gated as `discount.apply` for what the discount would
  take before tax and its percentage, after the order's own rules have had their say, so a
  discount the order refuses asks for no approval; `remove_discount`, not gated. Tickets list each
  line's discounts and the whole order's, with what each took, open or closed.
  - Known answers: a cashier's 10% off four lattes alone; 20% off the order once a manager
    approves, the approval recorded for $3.78 and 20%; $1.00 off counting as 5.30%; $6.00 off
    needing a manager, for 33.52%; a removal; a closed ticket's discounts; refusals asking for no
    approval. Planted bugs: 5, every one caught.
  - **The receipt** shows each discount as less, under its line or above the subtotal: found
    building it, the receipt listed lines before their discounts and a subtotal after them, so
    they didn't add up. Known answer: two espressos, 10% and $0.25 off, then 12.5% off the
    order, printed line by line. Planted bugs: 2.
- **`keel-ffi`**: percentage and amount discounts, their removal, and the tickets' discounts,
  crossed field for field.
- **The Android register**: a discount sheet for a line or the order, with the register's
  reasons, six percentages, and an amount typed as a cash keypad types it; each discount on the
  ticket, with a way to take it off; a discount beyond the cashier's role waits for a manager and
  goes on as it was asked.
  - Tests: the model's discounts on a line and the order, an amount typed, an order refusing an
    amount, a removal, and an approval retried as it was asked; percentages in words; and, on
    an emulator, a cashier's 20% off an order once a manager approves, totalling $2.83.
- **Verified**: `cargo fmt`, clippy with all features and none, the docs, every workspace test;
  the Android register's JVM tests, the kernel's unit tests, the app and its instrumented tests
  built.

The review, of the kernel and of the Android shell, each by a reviewer who hadn't built them,
found:

- **In the kernel**: discounts stacked past a role's limits, five of 10% taking 41% with no
  approval, now gated together, compounded; an amount larger than its line, taking more as the
  line grew, now refused; a raised discounted or priced line, and a price set in steps, now asked
  for again, from the rung price; a change to a partly paid check, leaving change owed, now
  refused; a split order emptied of its last open check stuck open for good, now closed; an order
  whose lines had all left its main check unpayable, now listed and paid; a split order's
  reprint missing all but its last check's receipt, now printing each.
  - Tests: the known answers reworked for the new rule ($5.88; $6.68 at 14.77%; $11.48 at
    43.34%), and one for each bypass and fix. Planted bugs: 14, and 9 re-aimed.
- **On the Android register**: a double tap putting on two discounts; a take-over's refusal
  speaking of dropping an order; Take over offered with the hub up; parked orders from earlier
  days not listed; a check's Pay using the check as drawn; buttons squeezed off a phone; actions
  offered that the kernel would refuse; an emulator test passing on a stale change. Each fixed;
  model tests cover the double tap and the earlier day's order.
- Gaps left, recorded in ADR-0030: a line both discounted and priced raised in quantity is asked
  for as a price override only; a price's approval names the order, not the line; an order
  discount is asked for as priced whole, which a split's checks can charge a cent differently; a
  partly paid check's lines frozen until refunds come.

### Staff, roles and approvals: MVP step 1 (2026-11-11)

The MVP, the pilot's counter service ([roadmap §3](./roadmap.md#3-mvp--pilot--counter-service-one-location-us)),
is built in ten steps, planned on 2026-11-11
([roadmap §8](./roadmap.md#the-mvps-build-steps)). Step 1, staff, roles and approvals
([ADR-0029](./adr/0029-staff-roles-and-approvals.md), accepted after its review), came in three
slices, each verified as it landed, then reviewed together:

1. **The profile and the policy** (built 2026-11-11): profile format 2, PINs, permissions, the
   policy's decisions, and `approval.granted`.
2. **The runtime and the bindings** (built 2026-11-11): PIN sign-in and lockout, approvals,
   the gated intents.
3. **The shells** (built 2026-11-11): the Android register's PIN pad and approval sheet, and the
   back office's staff and roles.

Built in slice 1:

- **`keel-domain`'s `staff`**: ten actions with stable codes and names, of which five are limited
  by an amount and discounts by a percentage too; a role's permissions, each action once, with
  limits only where it has them; a request, permitted when the role lists its action within its
  limits, an amount counted by its size; `PinHash`, Argon2id with ADR-0029's parameters over 4 to
  8 ASCII digits, compared in constant time; and `approval.granted` v1, in a stream of kind
  `approval`.
  - Known answers: PINs hashed as `argon2-cffi`, the reference implementation's bindings, hashes
    them; PINs of every wrong shape refused, and only a PIN's own verifies; limits at their
    bounds, a cent and a basis point over, and a negative amount by its size; a role's rules;
    the actions' codes and names pinned; a cashier asking a manager, and no one approving their
    own request; two approval payloads pinned as `cbor2` encodes them, and every rule a payload
    breaks refused.
- **The profile, format 2**: key 12, roles, and each member's role and PIN. A profile without
  roles is format 1, byte for byte as before, and lets every member do everything with no PIN;
  with roles, every member has one of them and a PIN. `Profile::permits`, `may_approve` and
  `verifies_pin`. The demo café is now format 2: Alex a manager (PIN 1234), Sam a cashier (PIN
  2468), its catalog's and rules' versions as before. The golden script builds it with `cbor2`
  and `argon2-cffi`; the bakery stays format 1.
  - Known answers: the demo pinned anew (2,632 bytes); format 1 refusing key 12, format 2
    requiring it with at least one role; a member with no role, no PIN, or a role the profile
    doesn't have; two roles of one identifier; a role or PIN without roles; the most roles.
  - Properties: half the random profiles now have roles, permissions drawn with limits at their
    bounds, and members with PINs; they round-trip, and every change to their encodings is
    refused or canonical.
  - Planted bugs: 13 new, all caught (392 in `keel-domain`'s list).
- The dependency policy gains `argon2`, which builds for `wasm32`.
- The workspace's tests, clippy with all features and none, the kernel crates for `wasm32`, and
  every planted bug's text.

Built in slice 2:

- **The runtime** (`keel-runtime`'s `staff`): `sign_in(member, pin)`; five wrong PINs in a row
  lock PIN entry for 30 s, then twice as long after each further five, up to 15 minutes, a clock
  set back leaving at most that; a right PIN starts the count again. `approve(approver, pin)`
  holds an approval for 60 s, for the member signed in, dropped when anyone signs in or out.
  Each gated intent asks `gate`: nothing if the member's role permits it, the approval to write
  if one waits that permits it, `ApprovalRequired` or `NotApprover` otherwise. The approval is an
  `approval.granted` event in the intent's write, its actor the member who asked, which the
  intent's events name in their envelope; it is spent only by a write that succeeds. Gated now:
  dropping an order with lines (`order.void`), taking a line off (`line.remove`), and printing a
  receipt again (`receipt.reprint`, which now needs someone signed in).
  - Known answers: only a member's own PIN signs them in; the lockout's 30 s, 60 s and 15-minute
    cap, and a clock set back a day; a cashier's void refused, refused again on their own
    approval and on a wrong PIN, done on a manager's, written once and named by the two events
    it approved, spent, not needed for an empty order or by a manager; an approval expiring after
    a minute, dropped by signing in or out, and surviving a refusal for another reason; line
    removal and reprints gated by a stricter role; a profile without roles.
  - Planted bugs: 12 new, all caught.
- **The bindings**: `sign_in(member, pin)`, `approve(approver, pin)`, the errors `WrongPin`,
  `LockedOut`, `ApprovalRequired` (naming the action) and `NotApprover`, and each member's role
  and whether they have a PIN. Known answers in Rust and in the Kotlin library: a wrong PIN, a
  void approved, the lockout.
  - Planted bugs: 2 new, both caught.

Built in slice 3:

- **The Android register**: a PIN pad when a member with a PIN signs in, its digits never in the
  register's state; and when an intent needs an approval, a sheet that asks who approves, then
  their PIN, and tries the intent again. `RegisterModel` keeps the refused intent and runs it
  once approved, or drops it on Cancel. The instrumented tests type the demo's PINs, and a new
  one drops a cashier's order with a manager's approval on the device.
  - Known answers: `RegisterModel`'s 16 JVM tests, 2 new: a wrong PIN and the right one; an
    approval asked for, a wrong approver's PIN, the intent done once approved, and Cancel.
- **The back office**: the cell's staff JSON (`keel-cloud`'s `staff`) and routes,
  `GET /api/locations/{id}/staff`, `POST …/staff/check` and `PUT …/staff`, published as the
  catalog is, against the version read. No PIN is ever read back: a member has one or not, and a
  new PIN's digits are hashed by the cell with a salt it draws. A Staff tab edits the team, each
  member's role and PIN, and the roles: each action, with its limits where it has them.
  - Known answers: staff read and written back to the same profile; a new PIN hashed with a
    fresh salt each time; a member new to the team refused without a PIN; refusals saying why;
    no PIN without roles; the API's routes, with no PIN material in what they answer; and an
    end-to-end test in Chromium, a cashier's role and PIN changed, a short PIN refused,
    published, taken by the register, and a new member refused without a PIN.
  - Planted bugs: 2 new, both caught.

**Review** (2026-11-11), by an independent reviewer, which found, each now fixed with a test or a
planted bug (ADR-0029, as built):

- Any right PIN started the count of wrong ones again: a cashier could guess at a manager's PIN,
  four tries between each of their own sign-ins, and never be locked. Every wrong PIN on the
  device counts now, and only 15 quiet minutes start the count again.
- An approval covered whatever gated intent came next: one given for a reprint that then failed
  covered another order's void. An approval is for the request asked (the action, its amount or
  percentage, and its subject), and `approve` checks the approver may give it; `approvers()` says
  who may. A reprint asks for one only once there is a receipt.
- A cashier could take each line off, then drop the empty order with no approval; and fewer of a
  line needed none. Both are gated.
- A member signed in stayed so when their PIN or role changed under them.
- The Android register replayed an approver's PIN when an intent was refused again, kept PIN
  digits in the state Android saves, listed every member as an approver, and showed a PIN pad's
  refusal twice.
- The back office showed a member with no role as having the first, so a location couldn't be
  given its first role, showed typed limits after Discard, and published with a limit that
  wasn't one; the cell's `Debug` would have shown a new PIN's digits.

Its other points are recorded as gaps in ADR-0029. Checked and found sound: the format-2 profile
and its compatibility, the pinned encodings, the property tests, the no-PIN path, `PinHash`,
self-approval, approvals across members, the gate's re-check, the approval event and its
naming, the API's PIN handling, and the project's rules.

- **After the review:** the workspace's tests, clippy with all features and none, the
  documentation, every planted bug's text, and the new ones caught (11 more); `RegisterModel`'s
  17 JVM tests, one new, its bug planted by hand and caught; the Android kernel module's 6 tests,
  the app and its instrumented tests built; and the back office's 7 end-to-end tests, twice.

### The cloud cell v0: step 8 (2026-11-10)

Step 8, the cloud cell v0 ([ADR-0028](./adr/0028-cloud-cell-v0.md), accepted after its
review), came in four slices, each verified as it landed, then reviewed together:

1. **Sync ingest** (built 2026-11-10): roster v1, `keel-net`'s named peers, the cloud's kit, the
   durable peer named, and `keel-cell`'s replication for each location.
2. **Catalog publishing** (built 2026-11-10): `profile.chunk` and `profile.published`, the
   runtime taking publications, and the cell publishing.
3. **The back office's API** (built 2026-11-10): an Axum service in the cell's process.
4. **The back office** (built 2026-11-10): `apps/web/backoffice`, React and Vite, tested end to
   end in a browser.

Built in slice 1:

- **Roster v1** (`keel-domain`), format 2: the cloud, `{1: device, 2: host, 3: port, 4: name}`,
  under key 5. The cloud must be one of the roster's devices and never host the hub; its host a
  DNS name or an address; its name a DNS name, not the hub's. A roster naming no cloud is still
  written in format 1, byte for byte as before, and a format-1 roster can't name one. Pinned by a
  second golden roster (429 bytes) beside the first, unchanged.
  - Known answers: the format-2 roster pinned; each of the four rules refused; format 2 without
    a cloud refused.
  - Planted bugs: 4 new, all caught (373 in all).
- **`keel-net`'s named peers**: `Net::hub` now dials the roster's cloud beside its listener, on
  the one network, so the hub's loop is unchanged and sends to the cloud as to any peer; the
  cloud's certificate must name the roster's cloud device and carry its name. `Net::cell`, the
  cell's listener for a location, admits the devices that may host the hub alone. `Net::cloud`
  dials the cloud alone. The listener and the dialer are now one each, shared.
  - Known answers: a hub reaching its cloud, a frame each way, and a register still served on the
    same network; a register refused by the cell; an impostor at the cloud's address, with its
    name but another device's certificate, refused; a roster without a cloud has none to dial.
  - Planted bugs: 3 updated to the refactored code, 2 new, all 11 caught.
- **The cloud's kit** (`keel-provision`): `Spec.cloud` and `--cloud <host>:<port> --cloud-name
  <name>` add a device named `Cloud` to the roster, never a hub, whose certificate serves the
  cloud's name and its own; its kit is `Provisioned::cloud`, written to `<out>/cloud`, apart from
  the location's devices'.
  - Known answers: the roster naming the cloud, the kit apart, the certificate's names and its
    client and server use; a location without a cloud has no cloud's kit.
  - Planted bugs: 2 new, both caught.
- **The durable peer named** (`keel-runtime`): a device that listens as the hub names the
  roster's cloud its durable peer; `SyncStatus` gains `backed_up`, how far the cloud holds the
  device's own log, and `head`, how far the log goes. `sync_status()` now reports a store error
  instead of hiding it. The Android hub dials the cloud through `Net::hub`, unchanged.
  - Planted bugs: 2 new, caught by the cell's ingest test, which keel-runtime's list now runs.
- **`keel-cloud`** and its binary, `keel-cell <cloud kit>...`: for each location, a `Location`,
  the cloud device's runtime over a store of its own, opened with no one signed in and
  replicating through `Net::cell` on a thread of its own, stepped by `keel-hub`'s loop. Anything
  else that reads or writes the store hands the thread a job (`Location::with`), run between two
  turns, so the store keeps one writer. PostgreSQL, NATS and object storage wait (ADR-0028
  amends ADR-0007).
  - Known answers: a location end to end over real TLS on the host, provisioned with its cloud:
    a register rings and pays a sale, the hub dials the cloud, and within seconds the cloud's
    store holds the order closed and the register hears its whole log is backed up, though not
    before it has left; a kit that isn't its location's cloud's refused; a job's answer coming
    back, and the store reopened as it was.
  - Planted bugs: 3, all caught.
- The workspace's tests, clippy with all features and none, and every planted bug's text.

Built in slice 2:

- **`keel-domain`'s `publication`**: `profile.chunk` v1, `{1: version, 2: index, 3: bytes}`, and
  `profile.published` v1, `{1: version, 2: size, 3: chunks}`, in a stream of kind `profile`;
  `ProfileVersion`, the SHA-256 of a profile's encoding. A chunk holds 1 byte to 192 KiB, a
  publication 1 byte to 64 MiB in exactly as many chunks as its size takes, each full but the
  last. `publish` cuts a profile into its events; `assemble` joins a publication's chunks, held
  in any order and any number of times, checking each one's length, that no two of an index
  differ, the hash, and that they decode to a profile.
  - Known answers: three payloads pinned as Python's `cbor2` encodes them; the registry; each
    rule at its bounds; a payload that breaks one refused; the demo café's profile in one chunk
    and one of 6,000 more items in three, joined again; a chunk missing, another version's, two
    that differ, one changed, one of the wrong length, and chunks that aren't a profile.
  - Planted bugs: 6, all caught.
- **The runtime taking publications** (`keel-runtime`): `publish_profile`, the cloud's alone,
  writes a profile's chunks and its publication in one write, in a stream of their own. Each
  runtime looks, when replication starts and after each frame, in the part of the cloud's log it
  hasn't looked at, and takes the latest publication that passes every check, replacing the
  profile between two intents; a cashier the new team doesn't have is signed out.
  `profile_version()` answers which it rings from. A device restarted takes again, as
  replication starts, the latest its store holds.
  - Known answers, a cloud, a hub and a register in memory: a publication reaching the hub and
    the register through the hub, the next line priced by it and the line rung before keeping its
    price; a later publication replacing an earlier, and the provisioned profile published again
    taking it back; a register restarted opening with what it took; only the cloud publishing,
    and only its location's profile; a publication in a device's own log, written under a roster
    that named it the cloud, never taken under the location's. In the crate: a publication of
    another location, one missing a chunk, and a team without the cashier signed in.
  - Planted bugs: 7, all caught.
- **The cell publishing** (`keel-cloud`): `Location::publish`, a job on the location's thread,
  and `Location::profile_version`.
  - Known answers, over real TLS on the host: a catalog published reaches the register in about a
    second, well within the 60 s budget, and the next sale is priced by it; the hub takes it too;
    the cell, restarted, opens with what it published.
- **The shells**: `keel-ffi`'s `SyncState` gains `backed_up`, `head` and `profile_version`, in
  hexadecimal; the Android register reads its team and menu again when the version changes, and
  goes back to signing in when the new team doesn't have the cashier.
  - Known answers: `keel-ffi`'s sync test checks the provisioned version and the marks; a
    `RegisterModel` test checks a new menu, nothing read again when nothing changed, and a
    cashier the new team doesn't have.
- The version types gain `to_hex()`, 64 lowercase hexadecimal digits.
- The workspace's tests, clippy with all features and none, the kernel crates for `wasm32`, the
  documentation, every planted bug's text (one of `keel-provision`'s, left stale by slice 1's
  refactoring, aimed again and caught), the register module's 22 JVM tests, the Android kernel
  module's 6, and the app built.
- The disk filled again, with 21 GB of debug builds from the planted-bug runs: cleared.

Built in slices 3 and 4:

- **The API** (`keel-cloud`'s `api`, `catalog` and `reports`), served by `keel-cell --api
  <address> --token-file <file> [--web <dir>] <cloud kit>...`: the locations, each device's
  replication (connected to the cell, and how far the durable copy holds its log), the catalog
  as JSON, checked or published whole, and sales over business days. The operator's bearer token
  is compared by its SHA-256, every byte. JSON errors throughout, the JSON extractor's own
  included; `409` for a stale catalog, `422` for one the kernel or the API refuses, with why.
  Each request's work runs as a job on its location's thread; checking a stale catalog's base and
  publishing are one job. `keel-hub::step_seeing` tells the cell who connects and leaves.
  - Known answers, through the router in the process: only the operator's token, not one that
    only begins as it; a location the cell doesn't serve; the catalog read, checked to its own
    version, a price changed checked and published, nothing published by a check, a stale write
    refused with the current version, and refusals by the API (an identifier that isn't one, an
    unknown field) and by the kernel (a group's fewest above its most, a button for a variant the
    catalog doesn't have); with a hub and a register over real TLS, two sales whose report totals
    are the register's own tickets', by day, item and tender, today's by default, an empty range,
    and bad ranges refused; the hub connected to the cell, the register not, its log held. In the
    crate: the demo café's catalog read and written back to the same profile, groups in any
    order, refusals saying why, and a price changed making a new version that keeps the rest.
  - Planted bugs: 10, all caught (13 in `keel-cloud`'s list).
- **The back office** (`apps/web/backoffice`): sign-in with the operator's token, kept for the
  session; a location's catalog editor (items, variants and prices typed as decimals, tax
  category, modifier groups and modifiers, menu pages and their buttons) with Check, Publish and
  Discard; sales by day, item and tender over a range; and the devices, read again every 5 s.
  React 19.3, TypeScript 5.9 (strict), Vite 8.3, each version pinned and settled for weeks; light
  and dark; no horizontal scroll at a phone's width.
- **`keel-cloud`'s demo** (`examples/demo.rs`): a location on the host in one process, the cell
  serving the back office, a hub and a register over real TLS, the register's three sales and
  one more for each publication it takes.
- **End-to-end tests in Chromium** (Playwright 1.63, 5 tests, about 8 s): a token that isn't the
  cell's refused, by the screen and the API; the hub connected and the register's log held; the
  reports totalling the register's sales, and a bad range refused; a price changed in the editor,
  a group that breaks the kernel's rule refused with why, checked, published, and taken by the
  register within the 60 s budget; and a stale catalog refused. Run five times in a row, the
  last four with no failure: the first found a report's late answer overwriting the dates typed
  after it, now dropped.
- **CI**: a job builds the demo, installs and builds the back office, and runs its end-to-end
  tests in Chromium.
- The dependency policy gains axum, serde and tower-http for the cell, tower and http-body-util
  for its tests, and the back office's packages.

**Review** (2026-11-10), by an independent reviewer, which found, each now fixed with a test or a
planted bug (ADR-0028, as built):

- A register restarted rang from the provisioned profile until it joined its network, and the
  Android register kept the menu it had read: the runtime takes what its store holds as soon as
  it is given the roster (`take_published`, called by `keel-ffi` before anything that can fail),
  the bindings answer `profile_version()` joined or not, and the register reads its team and menu
  again whenever the till's version isn't the one they were read at.
- The cloud could publish before its log was settled: it waits, and the API answers 503.
- The editor reformatted a price at each key, and published the last valid amount behind an
  invalid one: it keeps what is typed, and holds back Check and Publish while a price isn't an
  amount; removing a variant or a group removes what refers to it.
- A catalog with no `base` overwrote what it never read: `base` is required, compared without
  regard to case.
- Statuses: the JSON extractor's own (413, 415, 422), 404 for an unknown route under `/api`, 422
  for a profile too large.
- A report failed whole on items of one name in two units, or an order in another currency.
- The hub admitted its cloud inbound, which would replace the connection it dialed.
- A burst of requests could hold up replication: at most 8 jobs between two turns.
- Nits: `SyncStatus`'s docs; `start_sync` failing after replication had started when the store
  couldn't be read; the end-to-end test leaving a group changed.

Its other points were checked and found sound: the trust in publications, the cursor of the
cloud's log, nothing caching the profile, the catalog's round trip, the token's comparison, the
cell's concurrency, and `keel-net`'s refactoring.

- **After the review:** the workspace's tests, clippy with all features and none, the
  documentation, every planted bug's text, and the new ones caught (10 more, all caught:
  `keel-runtime` 64, `keel-net` 12, `keel-cloud` 19); the back office's 6 end-to-end tests, three
  runs in a row; the register module's 22 JVM tests, the Android kernel module's 6, and the app
  built.

### The Android register shell: step 7 (2026-10-06 to 2026-11-10)

Step 7, the Android register shell ([ADR-0023](./adr/0023-register-shell.md), accepted after
slice 1's review), comes in five slices, each ending with a review, each designed in detail in an
ADR of its own when it begins:

1. **The device runtime and the location profile** (built 2026-10-06, reviewed 2026-11-09):
   `keel-runtime`, which turns a cashier's intents into the kernel's commands, one write each,
   and the store into views; the location profile, with its catalog and the rules for ringing an
   item with modifiers; and `Locale` v0, the kernel's display text for amounts.
2. **Receipts and printing** (built and reviewed 2026-11-09,
   [ADR-0024](./adr/0024-receipts-and-printing.md)): KeelDoc v0 in `keel-doc`, an ESC/POS
   renderer, the print driver and raw TCP to port 9100 in `keel-print`, and print jobs in the
   outbox, never sent twice by themselves.
3. **The bindings** (built and reviewed 2026-11-09, [ADR-0025](./adr/0025-bindings.md)):
   `keel-ffi` with UniFFI, and a Kotlin/JVM library tested on the host.
4. **The Android app** (built and reviewed 2026-11-10, [ADR-0027](./adr/0027-android-app.md);
   built in five parts, the skeleton first): the register's screens, keys in the Android
   Keystore, the build for Android, CI on an emulator, and the performance harness on a
   reference device.
5. **The local hub** (built and reviewed 2026-11-09, [ADR-0026](./adr/0026-local-hub.md); built
   before slice 4): `keel-net`, WebSocket over TLS 1.3 with mutual TLS, the hub role in the
   app and in a Linux daemon, and a provisioning tool standing in for the Device CA.

Built in slice 4, each verified as it landed, then reviewed:

- **The skeleton** (`apps/android`): the Android Gradle Plugin 9.5.1 on Gradle 9.8.2, Kotlin
  2.4.21, Compose, `minSdk` 28, `targetSdk` 36, compiled against API 37 (Compose's BOM needs it),
  NDK r30. Three modules: `:register`, Kotlin Multiplatform, with `Till`, what the register asks
  of the kernel, and `RegisterModel`, its state over one worker; `:kernel`, `keel-ffi` built by
  `cargo ndk` for `arm64-v8a` and `x86_64` and its bindings generated by the build, with
  `KernelTill`; and `:app`, a Compose screen of the menu's pages and the order, paid with exact
  cash, the device's identity drawn by `keel-ffi`'s new `new_device()` and its keys wrapped by an
  AES-256-GCM key in the Keystore (StrongBox where there is one), kept out of backups.
  - Known answers: `:register`'s 2 JVM tests against a fake till (a cashier signs in, rings two
    items and pays; a refusal shown until the next intent); `keel-ffi`'s new test, a new device
    opening a register of its own, its parts each drawn on its own, and its 2 planted bugs caught.
  - The app and its instrumented tests build here; the APK carries the kernel for both ABIs, 7.4
    and 8.0 MB, stripped, uncompressed and aligned to 16 KB pages (`zipalign -c -P 16`), and no
    ABI the kernel isn't built for. The instrumented test, an espresso rung and paid on a device,
    runs in CI's new emulator job (API 30, 2 GB): this environment has no KVM.

- **CI's first emulator runs** found two things, both fixed: the crash tests' children, spawned
  side by side, hold a copy of a store's lock from fork to exec, so a test reopening its store in
  that moment found it busy (the tests retry); and **every store open failed on Android**: std's
  `File::try_lock` has no implementation there. `keel-store` locks with `flock` through `rustix`
  on every Unix (its planted bug re-aimed and caught), and the app words the kernel's failures
  when it opens, where the first run showed only "Something went wrong: ".
- **The register's screens** (part 2): sign in from the team and out; the modifier sheet; a
  line's quantity and removing it; dropping an order; the cash tender, from the kernel's offers
  or typed on a keypad; and today's orders, an open one taken up again.
  - The kernel: `Runtime::team`, `orders_today` (the orders begun on today's business date,
    whatever became of them), `cash_offers` and `amount`, each through `keel-ffi`. Known answers:
    today's orders across the demo café's 4 am cutoff; the team in order; offers for $2.18
    ($3, $5, $10, $20) and $28.00 ($29, $30, $40, $50), and nothing more for an order paid; a
    property test of the offers against an oracle in `i128`, aimed at zero and exact multiples;
    and `keel-ffi`'s two new tests. 9 planted bugs, each caught.
  - `:register`: `ModifierSheet` and the larger `RegisterModel`, 13 JVM tests against a fake till
    (the sheet's rules, signing in and out, a latte rung through its sheet, lines changed and
    taken off, cash tendered from an offer and typed, short refused, today's orders and an open
    one resumed, refusals and failures shown); 4 bugs planted by hand in the sheet, each caught.
  - The instrumented test signs in, rings a small latte with oat milk through its sheet, pays
    the exact cash offered, and finds it paid among today's orders: passed in CI's emulator job
    (API 30), the first run with the lock fixed.
- **Printing** (part 3): `PrintLoop` in `:kernel`, the print driver: each job due taken and its
  outcome recorded on the kernel's thread, sent with `printTcp` on a thread of its own, outside
  the register's lock, with the default printer profile (48 columns, a cutter, confirmed once
  printed) set when it first has an address; a job it may have printed in part is left
  unconfirmed for staff. The printer's address is kept in the app's preferences, set on a printer
  page that lists the jobs and asks staff whether an unconfirmed one printed; a paid order is
  reprinted from today's orders.
  - `:kernel`'s unit tests run on the host against the kernel built for it, through JNA: a
    sale's drawer kick and receipt reach a fake printer each once, in order; with no printer
    set, the jobs wait. A loop that never sets the printer's profile, planted by hand, is caught.
  - `:register`: 2 more JVM tests, 15 in all: the printer's address kept and checked (a blank
    host, a space, ports 0, 65536 and a word refused, nothing refused kept); a paid order
    reprinted, an unconfirmed job printed again, and an unpaid order refused a reprint.
  - The instrumented test points the register at a printer in its own process before the sale,
    and the drawer kick and the receipt reach it.
- **The hub on Android** (part 4, first half): `keel-ffi`'s `start_sync`, `sync_step` and
  `sync_status`, and `NotSettled`; the roster's most preferred hub (the highest priority, then
  the lowest identifier) listens at its port, every other device connects. `keel-hub`'s daemon is
  behind a default feature, so the bindings take `step` without the provisioning tool. In the
  app, a provisioned kit pushed to the app's external files is imported on first launch
  (`DeviceFiles`, in `:kernel`): its secrets wrapped by the Keystore, its plaintext removed, and
  the register opened with its identity and profile and joined to its location, replication
  running on the kernel's thread every 50 ms without waiting; the register shows "Store hub",
  "Synced", "Joining…" or "Offline". `tokio`, `rustls` and `ring` cross-compile for both ABIs;
  the library is 9.8 MB for `arm64-v8a`.
  - Known answers: through the bindings, a location provisioned in memory, its hub and register
    joined over TLS on the host, the register settling and its sale reaching the hub, closed; the
    roster's most preferred of three hubs listening; 4 planted bugs, each caught. On the host,
    `DeviceFiles`: drawn once and the same after, keys kept wrapped; a kit imported once and
    removed, a later kit changing nothing; a kit missing a file left alone. `:register`'s 16th
    test: the sync status read again, and one that can't be read shown as nothing.
  - Parts 2 to 4's instrumented test passed in CI's emulator job: the drawer kick and the receipt
    reach a printer in the test's own process.
- **The hub in the background, and the sync test** (part 4, second half): the kernel, its thread,
  the print driver and replication belong to the app's process (`KernelHost`, held by
  `KeelApplication`), not its screen; a register joined to its location starts `RegisterService`,
  a foreground service of type `connectedDevice`, which keeps the process running and, restarted
  by the system, opens the kernel again. CI's emulator job provisions a location on the host and
  runs its hub there; `SyncTest` imports the register's kit on the app's first launch, waits to
  hear the hub, and sells while synced (the test orchestrator clears the app's data before each
  test). And `tools/android/kernel-tests.sh` runs the golden baskets and the store's, the
  runtime's and the bindings' unit tests on the emulator, cross-built with `cargo-ndk`, where the
  store's lock failed before.
- **The performance harness** (part 5): `keel-runtime`'s `tests/latency.rs`, ignored but for
  `--ignored`, adds `KEEL_SKUS` items (100,000 by default) to the demo café's profile, checks,
  encodes and decodes it as a device receives it, opens a runtime over it, and rings and pays 500
  sales, eleven open at a time; it prints each step's time and each intent's median and 99th
  percentile, and fails only far beyond a register's budget. On the development machine, in
  release: the profile 8.1 MB encoded, built and checked in 0.30 s and decoded in 0.44 s, the
  runtime opened in 7 ms; an intent 0.50 ms at the median and 1.1 ms at the 99th percentile
  (1,500 intents), paying cash 1.2 and 2.8 ms, a ticket 57 and 130 µs, an item 4.8 and 8.8 µs:
  a catalog of 100,000 items costs the intents nothing beyond slice 1's baseline, and its
  decoding half a second at startup. CI runs it on the emulator too, through
  `tools/android/kernel-tests.sh`. `TapToFrameTest` times 100 taps on the emulator, each from the
  tap until the order's total on screen changes, and logs the median and 99th percentile; the
  reference device, with the large catalog in the app, gives the numbers that count, and waits
  for the device to be chosen.
- **All of slice 4's parts passed CI** (run 64, every job green, the Rust kernel's exhaustive
  sweeps included). On the emulator (API 30, 2 GB): the instrumented tests (a latte rung through
  its sheet, paid, and printed to a printer in the test's process; a provisioned register
  importing its kit and syncing over TLS with a hub on the host, whose log shows it settle and
  serve its peer; and the tap to frame); and the kernel's own tests, cross-built and run through
  `adb`: the golden baskets (1), the store (79, its lock among them), the runtime (30) and the
  bindings (18, a hub and a register syncing over TLS among them). The latency harness there,
  with 100,000 more items: an intent 0.80 ms at the median and 1.5 ms at the 99th percentile,
  paying cash 2.1 and 4.0 ms, the profile decoded in 0.50 s.
- **The review**, an independent one of the kernel's part, the platform's and the register's,
  found and these fix, each with a test or a planted bug (ADR-0027, as built):
  - A standby the roster lets host the hub, but which doesn't listen at its address, took the
    role with the hub gone, and every register became an island: only the device that listens
    may host. A test joins a hub, a standby and a register through the bindings, drops the hub,
    and finds the standby not serving 8 s later; planted, the bug is caught.
  - A sync step took in one network event: it takes in what is waiting, within a budget, and caps
    its wait. Joining starts the network first; the hub listens on IPv6 and IPv4. Only another's
    hold of a store's lock reads as busy. Cash for a currency without decimals is offered in
    hundreds. The latency harness times today's orders: 32 ms for a day of 500.
  - The modifier sheet kept choices by modifier, and a group offered in two places was charged
    twice: they are kept by place. Quick taps of + and − counted from the screen. "Synced" showed
    before the log settled. Sign-out dropped the open order. A page holding a variant twice
    crashed the grid. 7 new JVM tests, 22 in `:register`; 4 bugs planted by hand, each caught,
    one only once a modifier offered in two groups was among the fixture's.
  - A failed join left the store locked, a first failed open stopped the loops for good, and a job
    the driver refused stopped all printing: the kernel opens without its network and joins
    after, the loops are supervised, and a refused job is unconfirmed, for staff (a test on the
    host). The foreground service starts in the foreground and at boot, a refusal stops it
    quietly, and it holds a wake lock. A kit's secrets left in the clear are removed whenever the
    device's files load, and the files are written whole and synced. The Keystore losing its key
    says so.
  - CI checks every native library's alignment under `pipefail`, the hub on the host listening,
    builds `--locked`, and runs this build's test binaries on the emulator.
  - Left for later: a hub's sync loop turns every 50 ms whatever it has to do; the emulator test
    doesn't yet check that its sale reached the hub; and a standby's takeover waits for
    discovery.

Built in slice 5, each verified as it landed, then reviewed:

- **The roster v0** (`keel_domain::roster`): a location's devices, each with its signing key
  (by its COSE algorithm, -7 or -8) and its priority as the hub, and where the hub listens and
  the name its certificate carries; canonical CBOR, checked whole (one to 1,000 devices, in
  order, no key twice, at least one that may host the hub, a valid host, and a hub name that is a
  DNS name a certificate can carry, and not a device's), giving the `DeviceRegistry` a store
  verifies received events with. Known answers (4 tests): the demo café's roster round-tripping
  and its registry; pinned byte for byte against Python's `cbor2`, its Ed25519 keys derived from
  the same seeds by Python's `cryptography`; every rule, at its limits; and every encoding
  refused, a P-256 key written compressed included. All 10 planted bugs caught.
- **Replication in the runtime** (`keel_runtime::sync`): `start_sync(roster)`, `on_frame`,
  `on_tick`, `next_tick`, and `take_frames`, which pushes what the device wrote since it last
  did, so a sale reaches the hub at once; `sync_status`. Intents are refused until the device's
  log is settled (`NotSettled`), and a roster that isn't this device's location's, or doesn't
  have it, or names another key for it, is refused (`RosterMismatch`); a device that hears no one
  writes as an island after 3 s. Known answers (7 tests, `tests/sync.rs`): a hub and a register,
  runtimes over their own stores passing frames in memory, the register refusing to write until
  settled, then ringing and paying a sale that reaches the hub, closed, and is confirmed back;
  every roster that isn't the device's refused; a clock set back, which stopped the heartbeats;
  a register alone, writing as an island; the hub's numbering failing, and retried; a hub demoted
  by its roster, which stops serving; and a hub restarted. All 7 planted bugs caught, in the
  runtime and in `keel-sync`.
- **`keel-net`**: `keel-sync`'s frames over WebSocket on TLS 1.3 with mutual TLS, `rustls` with
  `ring`, on a `tokio` runtime of its own that talks to the caller's thread through channels. The
  hub listens; a device connects to the roster's address and again after a backoff, 1 s doubling
  to 30 s. A peer is the roster's device whose name its certificate carries,
  `<uuid>.devices.keel`, exactly one, with no wildcard: a DNS name rather than ADR-0026's URI,
  since `rustls`'s verifier checks names without another parser; the hub's certificate also
  carries the roster's hub name, which the device checks, and names a device that may host the
  hub. Reading and writing run side by side, each side pings every 5 s, a connection silent for
  15 s closes, every queue is bounded, and at most 32 handshakes run at once. Known answers (6
  tests, over real TLS on the host): a hub and a device exchanging frames, the largest a frame
  may be included; a frame one byte larger closing the connection, and the device coming back; a
  certificate from another CA, a device the roster doesn't have, a certificate with no device's
  name, and a hub without the roster's name, each refused; a certificate naming two devices, or
  a wildcard, and a hub naming a device that may not host, each refused; 40 frames of a megabyte
  each way at once, which stalled the first version for good; a quiet connection kept up past
  the silence by the pings; a device reconnecting after its hub restarts. All 8 planted bugs
  caught.
- **`keel-provision`** and **`keel-hub`**: the provisioning tool, which makes a development CA,
  and for each device its identifier, signing key, store key, TLS key and certificate, with the
  roster and the profile, in memory and as a kit of files, its keys readable by their owner
  alone, and its certificates for what each device does (client authentication, and server
  authentication for the hub), valid for 397 days from a day before, under a CA of path length
  0; and the hub's daemon, a runtime over
  its own store listening at the roster's port, with `step`, the loop that joins a runtime to the
  network. Known answers (2 tests, `tests/location.rs`): a location provisioned in memory, a hub
  and a register as runtimes on the system's clock, each on its own thread, joined over real TLS:
  the register settles, rings and pays a sale, which is confirmed by the hub within 2 s of being
  paid (3 s for the whole test), and the hub holds it closed; and the kits written and read back,
  every key drawn on its own, a short key and a missing file refused. Known answers (4 tests,
  `keel-provision`'s `tests/kit.rs`), its certificates read back with `rustls-webpki`, the
  verifier `keel-net` uses: a register's certificate refused for serving, the hub's taken; each
  valid half a day before it was made and refused two days before, and taken at 396 days and
  refused at 398; the CA's constraints and the devices' key usage in their encoding; and the
  kit's secret files 0600 in a directory of 0700, a file there before narrowed. All 11 planted
  bugs caught;
  a fifth, the loop's own push left out, changes nothing a test can see, since `on_frame` and
  `on_tick` push first. The binaries run: `keel-provision` writes a location, and `keel-hub`
  listens on it, waiting, as `keel-sync` requires, for its first device to settle.
- **The review**, an independent one of each part, found and these fix, each with a test or a
  planted bug (ADR-0026, as built): a connection that read and wrote in turn stalled for good
  when both sides wrote more than the sockets held; unbounded queues, and connections silent
  without closing that stayed open; a certificate naming two devices, or a wildcard, taken, and
  any device of the roster taken for the hub; hub names the roster took and `keel-net` refused;
  secrets written readable by anyone, and certificates with `rcgen`'s defaults, valid from 1975
  to 4096 for any purpose; a failed numbering never retried; a demoted hub serving on; a clock
  set back stopping the heartbeats; and the hub's step skipping its tick after a bad frame.
- The workspace's tests, with the exhaustive sweeps (662 tests), and without default
  features (658); clippy with all features and none; the documentation; the build
  for WebAssembly; and every planted bug's text.

Built in slice 3, each verified as it landed:

- **`keel-ffi`**, the bindings, on UniFFI 0.32.2 pinned exactly, its bindgen behind a feature:
  a `Register` object holding one device runtime behind a lock, with a method for each intent
  and view, and for printing; `printTcp`, which runs the driver on the caller's thread, outside
  the lock; `demoProfile`; records for every view, enumerations for modes, prefixes, states and
  outcomes, and `KeelError`, a variant for each refusal a cashier meets. The workspace's lints
  hold, `unsafe_code` forbidden included: UniFFI's generated code isn't the crate's own. The
  register reads the system's clock and the operating system's randomness: the one place the
  kernel does.
  - Known answers (5 tests): flattened choices built into their tree, and every bad parent and
    quantity refused; a register refusing a key of the wrong length, a device that isn't an
    identifier, a profile that doesn't decode, a locale it doesn't know, and another store's key;
    a sale signed in, rung with modifiers, refused by the catalog, changed, refused short and
    negative, paid, its drawer kick and receipt sent over TCP to a fake printer and done, and a
    reprint not sent where no printer listens; every outcome across the boundary and back; every
    prefix and mode.
  - Planted bugs: all 6 caught, one only once the last test was written.
- **`bindings/kotlin`**, the Kotlin library: Gradle with Kotlin 2.4.21 and JNA 5.19.1, its sources
  generated by `generate.sh` from the library built for the host. Two JUnit tests: a sale on the
  demo café's profile, refusals as `KeelException`'s subclasses (not signed in, the catalog's, a
  short tender with what is due and its text, a negative one), cash taken, and the drawer kick and
  receipt sent with `printTcp` to a fake printer written in Kotlin; and bad arguments refused. Both
  pass here and run in CI's new job.
- The dependency policy gains UniFFI, MPL-2.0, the one exception to its permissive list.
- The workspace's tests, with the exhaustive sweeps (631 tests, 4 minutes 28 seconds), and
  without default features (627); clippy with all features and none; the documentation; and
  every planted bug's text.
- **Review** (2026-11-09), by an independent reviewer, which found, each now fixed with a test or
  a planted bug (ADR-0025, as built): a store opened twice had two writers, and the second
  register's writes all failed (the store now takes an exclusive lock, `StoreError::Busy`,
  `KeelError.StoreInUse`); every store failure was `Device`; a panic poisoned the register for
  good; uppercase identifiers were taken and given back lowercase; `printTcp` took a bad process
  ID for an unreachable printer, and tried one address; Gradle skipped the Kotlin tests after a
  change to Rust that left the Kotlin as it was; `generate.sh` failed under macOS's bash 3.2 and
  ignored `CARGO_TARGET_DIR`; the Kotlin tests never closed their registers. Its 25 bugs planted
  in the conversions survived the tests but for 3: the tests now check every field and case,
  and `keel-ffi`'s list holds 28, each caught. Left as a gap: Kotlin's `message` of an exception
  is UniFFI's rendering of its fields, not the Rust error's text.
- **After the review:** the workspace's tests, with the exhaustive sweeps (639 tests, 3 minutes 22
  seconds), and without default features (635); the two Kotlin tests; clippy with all features
  and none; the documentation; every planted bug's text, and all 28 of `keel-ffi`'s caught.
- The disk this environment allows filled twice: the many planted-bug runs leave 13 GB of
  incremental build caches. Local runs now set `CARGO_INCREMENTAL=0`, as CI does.

### Receipts and printing: step 7, slice 2 (2026-11-09)

Commits `7b7fa3d` (the documents and the driver), `e93703c` (print jobs in the runtime), and
`86e1f75`, which fixes what the review found and accepts
[ADR-0024](./adr/0024-receipts-and-printing.md).

Slice 2's design settles what ADR-0023's decision 6 left open: a job is unconfirmed while it is
running and not with the driver, so the outbox holds every state a job has without a new store
schema; staff resolve an unconfirmed job (it printed, print it again marked "REPRINT", or dismiss
it); the printer's profile and address are the device's until routes; and the receipt is built in
the write that closes the order, so it shows the order as it closed. Its research had two of
Epson's and a printer maker's manual hosts outside this environment's network policy: the
status bits follow Epson's reference as widely implemented, and the tests' fake printer pins them.

Built, each verified as it landed:

- **`keel-doc`**, portable and built for `wasm32`: KeelDoc v0 (text, rows, rules, feeds, cuts and
  drawer kicks, in bold, wide and tall), its canonical encoding, Windows-1252 text with
  transliteration and every control dropped, the layout in 24 to 64 columns, and the ESC/POS and
  plain text renderers, which print the same lines.
  - Known answers (17 tests): every command's bytes; a document rendered whole, with and without
    a cutter; every status byte's bits; text wrapped, broken and aligned at each boundary; rows
    sharing a line and not; every class of character, and every character of the first three
    planes yielding no control byte; the document's limits; and an encoding pinned against
    Python's `cbor2`, with every way one is refused.
  - Property tests: documents round-trip their encoding; and any document's bytes, read back by
    an interpreter written for the tests, give the lines a model of the layout gives (written to
    take the longest run of words that fits, where the code streams them), none wider than its
    columns, with no control byte outside the renderer's commands and no real-time command,
    whatever the text. 20,000 cases pass. A probe of 2,000 documents: a word longer than its line
    in 69%, a right text on a line of its own in 26% and sharing one in 61%, wide text in 73%,
    controls in 37%; words exactly as wide as a line were in 4%, so the generator now writes them.
  - Planted bugs: all 12 the property tests can reach are caught by them, once the model stopped
    borrowing the crate's own width of a wide line; the other 7 by unit tests, one of which had
    none until a planted bug showed it (a feed of no lines).
- **`keel-print`**: the driver, one job on one connection, over a `Transport`, and raw TCP to
  port 9100. Tested against a scripted fake printer on TCP: a job printed and confirmed, paper
  near its end, cover open, paper out (offline and online), an error, offline for another reason,
  a byte that isn't a status, a printer that doesn't answer or isn't there, a reset mid-job,
  silence after the job, a printer without `GS ( H`, one that confirms nothing, and chatter before
  the word; and, with an in-memory transport, a send failing before and after its first byte.
  All 7 planted bugs caught, one of them only once that last test was written.

- **Print jobs in the runtime**: paying cash enqueues the drawer's kick, when cash changed
  hands, and the receipt, in the write that closes the order, caused by its closing event. The
  receipt (`receipt.rs`) is built there from the closed ticket and its payments, in English or
  Spanish: the location, the order's short number, its local time of closing and its cashier,
  each line with its modifiers, the totals, the cash, any rounding and the change. The runtime
  hands jobs to a driver one at a time (`next_print_job`), the earliest due and then the
  earliest enqueued, records their outcomes (`print_outcome`), retries what wasn't sent after
  2 s, doubling to 60 s, and leaves what may have been sent unconfirmed for staff to `resolve`:
  printed, again (marked "REPRINT"), or dismissed. `reprint(order)` prints the receipt the order
  printed when it closed, marked above and below, whoever reprints it and whenever.
  `BusinessDayPolicy::local_time` gives a receipt its time.
  - Known answers (8 tests): the demo café's receipt pinned as text; a sale that owes nothing
    opening no drawer; jobs waiting for a printer and going one at a time; the backoff, a
    millisecond either side of each wait; an unconfirmed job never coming back by itself, then
    resolved each way, a reprint of a reprint marked once; a job with the driver when the runtime
    stopped found unconfirmed; reprints showing the close as it was; the receipt in Spanish.
  - A property test against a model of the jobs: sales with and without cash, a printer set or
    not, every outcome, outcomes for jobs not with the driver, staff's decisions, reprints, time
    passing to each wait and a millisecond short of it, and restarts. After each step the jobs
    shown are the model's, each job handed out is the model's, and none is handed out again once
    any of it may have been sent. Its first run found the model wrong: a job waiting to be tried
    again rightly lets a later one go first. A probe of 1,000 runs: 70% hand a job to the driver,
    14% retry one, 43% leave one unconfirmed; the first generator set a printer in a quarter of
    runs and missed a planted bug in the backoff until it set one in most.
  - End to end: a sale rung and paid, its drawer kick and receipt sent over TCP by the driver to
    a fake printer that answers as a ready Epson, the bytes it receives the jobs', and the jobs
    done.
  - Planted bugs: 11 in the runtime's list, the 8 the property can reach caught by it, the other
    3 by known answers; and all 33 of the runtime's list caught, run again since paying cash
    changed.
- The workspace's tests, with the exhaustive sweeps (614 tests, 3 minutes 2 seconds), and
  without default features (608); clippy with all features and none; the documentation; the
  wasm build, `keel-doc` now in it; and every planted bug's text.

- **Review** (2026-11-09), by three independent reviewers, of the documents, the driver and the
  runtime's jobs. They found no path that hands an unconfirmed job to the driver again, and
  every command's bytes right against Epson's reference. They found, each now fixed with a named
  regression test and a planted bug (ADR-0024, as built):
  - A cash order of about 200 lines couldn't be paid: its receipt went past a document's 1,000
    blocks and failed the closing write. Lines are capped by blocks as well as bytes, and a
    receipt that can't be made whole is printed short; nothing about a receipt refuses a sale.
  - After the clock was set back, jobs waited for it to catch up, so the drawer stayed shut. A job
    due later than any wait it could have been given is due at once (`reschedule`, new in the
    outbox).
  - With `GS r 1`, the driver took a byte of an automatic status block for the printer's word;
    for a printer that confirms nothing, it said the job printed before the printer read it; a
    printer that stopped reading held the send for ever; and a printer sending automatic status
    as the connection opened could never print. Answers are read as blocks, the driver waits for
    the printer's own close, sends time out, and automatic status is skipped.
  - Line breaks and tabs glued words together; no-break spaces broke; tags and other format
    characters printed as `?`.
  - Without a cutter, a cut fed lines in the style before it: the property found it once its
    model compared every line's style.
  - Nearly every job's process ID was `0001`; the job views read the whole outbox history; a
    reprint after the locale changed could be marked twice.
  - Tests that couldn't fail: the encoding was pinned for one block only (a consistent swap of
    wide and tall passed everything); the property's model took its text from the crate's own
    table; the test of a job cut off mid-way ended it after the whole job had arrived; and no
    test caught a send that failed part way reported as none of it sent.
  - Left as known gaps: `reprint` reads the whole outbox; full-width lines on generic firmware,
    to measure on the reference printer.
- **After the review:** the workspace's tests, with the exhaustive sweeps (626 tests, 3 minutes 4
  seconds), and without default features (620); clippy with all features and none; the
  documentation; the wasm build; every planted bug in the slice's lists caught (`keel-doc` 24,
  `keel-print` 12, the runtime's 38, and the outbox's two new ones); and soaks of 20,000 cases of
  the documents' properties and 10,000 of the jobs' and the outbox's.


### The device runtime and the location profile: step 7, slice 1 (2026-10-06, reviewed 2026-11-09)

Commits `cf064f5` (the step's design and the slice), `a3901e7` and `484cba6` (the review's first
fixes), and `54b1d2f`, which finishes the review and accepts
[ADR-0023](./adr/0023-register-shell.md).

Built in slice 1, each verified as it landed:

- **`Locale` v0** (`keel-types`): amounts and quantities as `en-US` and `es-US` show them, from
  a snapshot of CLDR 48's data (`data/cldr/`), which a generator turns into a table and CI
  checks. Known answers; every currency's symbol and both locales' separators checked against
  the snapshot; a property test against a model built from arbitrary-precision integers; and
  2,254 amounts (seven of each of the 161 currencies, in both locales) identical to ICU 78.2's,
  which has CLDR 48. All ten planted bugs caught, nine by the property and snapshot tests.
- **The location profile** (`keel_domain::profile`): a location's settings, pricing rules,
  catalog, menu and team, in canonical CBOR, checked whole; the catalog's and rules' versions
  are SHA-256 hashes of their encodings; ringing a variant with a cashier's choices. Known
  answers: the demo café's items rung, free applications at ties, every refusal, every rule a
  profile is checked against, and the demo pinned byte for byte, with its versions, against the
  same profile built independently with Python's `cbor2`. Property tests: ringing against a
  model written separately, over random catalogs and choices aimed at each group's bounds and at
  ties in price (10,000 cases pass), and profiles round-tripping. Of 19 planted bugs, the 13 the
  property tests can reach are caught by them; the first run missed one, ties in price going to
  the modifier listed last, until the generator tied prices within groups and made half its
  choices valid. The other six are caught by unit tests.
- `keel-events` exposes its SHA-256 (`hash::sha256`), which the profile's versions use.
- **`keel-runtime`**: a device's store and its location's profile, with the intents a register
  makes, each one write of the store: start an order; add an item, rung from the catalog; change
  a line's quantity; take a line off; abandon an order, its lines taken off first; and take cash,
  the payment started and captured with what was tendered and its cash rounding, then the check
  and the order closed. Its views are whole, every amount with its text: an order's ticket, the
  open orders, the menu, and an item's modifier groups. A team member signs in, and every event
  records them, the business date the profile's policy gives the clock's time, and a
  correlation per intent. Refusals are typed.
  - Known answers (11 tests): the demo café's sale rung, changed and paid, with change, and a
    tender short of it refused; cash rounded to five cents, a tender short of the rounded cash
    refused, and the payment recording the tender and the rounding; an order that costs nothing
    closing with no payment; an order's owner; events' business dates either side of the cutoff
    (a second before 4 am in New York is still the day before) and their correlations; the menu's
    pages, prices and required choices, and an item's groups with those under its modifiers.
  - A property test against a model of a register: random intents on the demo café, with and
    without cash rounding, tendering on either side of what is due before rounding and after,
    some interrupted at each point of their write where the store can fail, and the store now
    and then opened again. After each intent every ticket matches the model's: its lines as the
    catalog rings them, with their modifiers, and its amounts as pricing prices the model's
    basket. Each intent is refused exactly when the model says, and one interrupted leaves
    nothing behind. 2,000 cases pass.
  - A probe of 1,000 cases: every intent done, interrupted at each point of its write, and
    refused in each way the model knows; cash rounded up in 6% of cases and down in 10%; a
    tender short only of the rounded cash in 3%; the store opened again with an open order in
    22% and with a closed one in 12%; a modifier chosen under another on a ticket in 5%, the
    most the demo nests. The probe found that the property never tendered between what is due
    before rounding and after: tenders now count from either, aimed at their boundaries.
  - All 19 planted bugs caught: the 12 the property can reach by it, and the other 7 (who may
    sign in, events' business dates and correlations, an order's owner, the menu and the item
    view) by unit tests.
  - Found and fixed during the build: a closed order's ticket listed its lines in the order of
    their identifiers, as the check's snapshot does, rather than the order they were rung.
    Opening the store again starts the runtime's identifiers afresh, so a line rung after a
    restart could sort first. The property found it; a named regression test keeps the case,
    and a planted bug the fix.
  - Latency on the development machine (a 2.1 GHz Xeon, the store on ext4 with every commit
    synced), the baseline for slice 4's harness: 500 sales, each paid once ten more were
    started, so that eleven are open at a time. An intent, its ticket included, takes 0.9 ms at
    the median and up to 2.7 ms at the 99th percentile; paying cash, four events in one write,
    1.8 ms and 4.2 ms. A ticket takes 59 µs, the open orders 0.7 ms for eleven, the menu 5 µs,
    an item 3.5 µs. On tmpfs, without the sync, an intent takes 0.4 to 0.5 ms and paying 1.2 ms.
- The workspace's tests, with the exhaustive sweeps (558 tests, 3 minutes 44 seconds), clippy
  with all features and none, the documentation, the wasm build, every planted bug's text, and
  both generated tables pass, as CI runs them.

- **Review** (2026-10-06 to 2026-11-09). Found and fixed, in `keel-runtime`:
  - An intent wrote, then made its ticket. An order the domain accepts can still overflow when
    priced, as a line of a huge price can, so such an intent was done but answered with an error,
    and its order could no longer be shown. Each intent now makes its ticket inside its write: it
    happens and answers, or is refused whole.
  - A paid order's ticket listed its taxes as the check's snapshot does, by identifier, not in
    the location's order an open ticket uses. The property now runs with a second tax as well,
    and compares each tax by its identifier.
  - A negative tender, on an order that owes nothing, closed it with negative change. It is
    refused, `RuntimeError::NegativeTender`.

  Each has a known answer and a planted bug the tests catch (22 in the runtime's list now), and
  a sale that cash rounds to nothing is a new known answer: it closes with nothing tendered.

  Found and fixed since, by three independent reviews of `Locale`, the profile and ringing,
  committed as work in progress:
  - `Locale` showed nothing wrongly: 104,976 strings, 324 values in each of the 161 currencies in
    both locales, as amounts and as quantities, are identical to ICU 78.2's (CLDR 48). But the
    table's generator checked the spacing rule for a symbol after the digits, which `Locale` never
    writes, not the one before them, and left the decimal pattern and currencies' own
    separators unchecked; it checks them all now. The comparison with ICU is a committed tool
    (`examples/locale_dump.rs`, `tools/icu_crosscheck.js`) that CI runs with Node.js 22.22.
  - The CBOR decoder reserved room for as many elements as an array declared, checked only
    against the bytes left: arrays nested 32 deep could reserve about a thousand times their
    input before failing. Arrays and maps now reserve room for at most 1,024 up front.
  - The profile: a tax rate's text with more digits than a rate holds was read, rounded, as
    another rate's; tax categories were a list, so one set had many encodings and versions;
    rounding modes were coded by their position in a list; cash rounding to the minor unit was a
    second way to say none. Each is refused or fixed, and catalogs have limits on their variants
    and modifiers in all.

  Found and fixed in the review's last part, each with a known answer and a planted bug:
  - Free applications went to modifiers that cost nothing, so a free "no whip" could take a
    group's free syrup. They go to the lowest-priced applications that cost something
    (ADR-0023, as built).
  - The catalog's modifier groups were a list, so one catalog had many encodings and versions:
    they are written in the ascending order of their identifiers, as tax categories are.
  - `BusinessDayPolicy` took `jiff`'s `Etc/Unknown`, which isn't the database's, against its
    documentation; and the profile took `Factory`, the database's zone for a device not yet set
    up. Both are refused.
  - Choosing a modifier twice was found by comparing each choice with every one before it: a
    shell sending tens of thousands of choices would have held the register for seconds. It is
    a set now.
- **Verified in the review's last part:**
  - Known answers: every count's limit, at it and past it; a catalog at every limit at once
    (200,000 items, 1,000,000 variants and modifiers, 100,000 groups), checked, encoded and
    decoded (48 s in a debug build, a sweep CI runs); a second golden profile, a Toronto
    bakery's, pinned byte for byte against Python's `cbor2` like the demo, writing every field
    the demo leaves out (cash rounding, taxes by dining, a group's minimum and free applications
    together, every prefix, a modifier chosen under a modifier in a group that repeats); its
    tarts rung; free applications skipping what costs nothing; a line's tax category; placeholder
    time zones.
  - Property tests, rewritten: ringing over catalogs of up to eight groups of six modifiers,
    prices that tie, from nothing to $9,999,999.99, every prefix, groups as wide as a group
    allows, quantities to 255 and items of three tax categories, against the model, which now
    checks each line's variant, name and tax category; whole profiles with every field drawn
    (five currencies, seven zones, cash rounding in every mode, up to four taxes with rates of
    up to 28 decimals, menus, teams, addresses) round-tripping, and refused when a tax's
    categories are out of order; and every change to any node of a profile's encoding (a value
    replaced, a key added or taken away, elements swapped, repeated or removed) either refused
    or decoding to a profile whose encoding is exactly the changed one, with an unknown key, a
    required key gone and a set out of order or repeated always refused. 20,000 cases of each
    pass.
  - A probe of 2,000 cases: 80% of choices ring; 9% choose a modifier that costs nothing in a
    group with free applications; 4% offer a group of the widest bounds, 1% ring a quantity of
    100 or more, and 13% ring modifiers under modifiers. The first version of the encoding
    property picked its node among all of a profile's, and missed both planted set-order bugs:
    it now picks a kind of node first, leaves a quarter of the time, and reaches every kind.
  - Planted bugs: 33 in the profile's list, the 23 the property tests can reach caught by them
    and the other 10 by unit tests; and one more in `keel-types`', caught by a unit test. Running
    the unit-only ones found that the earlier refusal of cash rounding to the minor unit had no
    test: it has a known answer now.
  - The workspace's tests, with the exhaustive sweeps (573 tests, 3 minutes 10 seconds), and
    without default features (567); clippy with all features and none; the documentation; the
    wasm build; and every planted bug's text, as CI runs them.


### Hub election and failover: step 6, slice 4 (2026-10-01)

Commit `d14e128`, and `abf80e4`, which raised CI's time limit, with CI green on it (51.3
minutes). The run on `d14e128` passed everything up to its second test pass, where the old
45-minute limit cut it off. Reviewed 2026-10-02: the review's fixes, below, are in `8977e85`,
which accepts [ADR-0022](./adr/0022-hub-election-and-failover.md), and `f5da291`, with CI green
on it (52.8 minutes); pushing `f5da291` cancelled the run on `8977e85`. Where the review changed
the code, the verification below was run again on it.

- **Built** ([ADR-0022](./adr/0022-hub-election-and-failover.md)):
  - `keel-domain`:
    - `hub.claimed`, version 1: a claim of the Store Hub's role, in a stream of its own of kind
      `hub`: its epoch, the claimant's priority, the claim it succeeds, and its **cuts**, how far
      into the log of each device on the chain it succeeds the claimant held.
    - `hub::chain`: the winning claim, by epoch, then priority, then the lower device, then the
      earlier claim; the chain of terms back from it, as far as the replica holds it; and each
      term's cut, the least any later claim on the chain gives. A record counts when its device
      holds the term of its epoch, and it follows the term's claim and lies within its cut.
    - `Claimed::succeeding`: the next claim, cutting each device on the chain where the claimant
      holds its log to, refused, `ClaimError::Behind`, unless the claimant holds the whole chain
      back to epoch 1 and every record that counts on it.
  - `keel-store`:
    - The claims projection, version 1, which works out the chain of terms again at each write
      that touches a claim; the integrity check reports a chain kept that isn't the one the
      claims make, `Problem::Terms`.
    - Confirmation, the feed and sequencing count only the records that count. Sequencing
      numbers each device's log after the last position such a record covers, and the hub's own
      log after its latest record of its epoch as well.
    - `Store::term()`, `terms()` and `claim(priority, now)`. `sequence(now)` and
      `answer_requests(now)` take their epoch from the store's own winning claim, and refuse,
      `StoreError::NotHub`, when it isn't the store's; a store behind the chain refuses to claim,
      `StoreError::Behind`.
  - `keel-sync`:
    - The heartbeat, `[1, 3, location, priority, epoch, hub, acting, beat]`, to every peer every
      second: the sender's priority, 0 while its log is unsettled or forked; its term, the epoch
      and hub of the winning claim it holds; whether it acts as that hub; and the hub's beat,
      which rises every period the hub acts, and which a peer that had it directly passes on.
      Beats are compared only within one hub's term.
    - The election. A replica that can be hub claims the next epoch as a heartbeat period begins
      when its log is settled and not forked, it has heard no hub and no preferred candidate for
      three periods, and it holds as much of each earlier hub's log as its other peers say they
      hold, or has waited three more periods. It serves only while it holds the winning claim with
      its log settled and not forked, and stops the moment a better claim reaches it.
    - Settling against every peer that answers; a fork, told by a peer refusing the replica's own
      log; and islands, `hub_reachable()`.
    - `Roles` gives a replica's priority as hub, and `SyncConfig` the heartbeat period and the
      periods of silence. `Replica` gains `terms()` and `claim(priority, now)`; `Replicator`
      gains `is_hub()`, `serving()`, `term()`, `hub_reachable()` and `forked()`. `keel-sync` now
      depends on `keel-domain`, for the chain.
  - `keel-sim`:
    - A standby: the hub at priority 2 and the standby at 1, linked to each other, to every
      device and to the cloud. No one holds a term at the start; the hub claims epoch 1.
    - Splits, which cut the location in two and leave a hub on each side until it heals; devices
      that are islands by their own heartbeats, not by the simulator's partitions; and runs in
      which the hub crashes without other faults.
    - New invariants: at the end, every replica holds the same winning claim, whose device alone
      serves, and a whole chain; the records that count number every event exactly once, each
      epoch's numbers gapless and each device's log in order; as the run goes, a candidate writes
      records and answers only while it serves, in its term's epoch, and a device's log settles
      only when no peer it heard from holds more of it; no request is answered in two terms.
  - Tooling: the planted-bug runner's `--release`, for the lists the simulator takes most of the
    time of, and `--stale`, which checks every bug's text is still in the code, and which CI now
    runs for every crate.
  - CI: the job's time limit rises from 45 minutes to 75. This slice's tests, the store's terms
    property, the protocol property and the simulator's failover runs above all, take each of
    its two test passes from 14 minutes to 25.
- **Verified:**
  - Known answers:
    - `keel-domain`, 18 tests: the payload pinned byte for byte, in payloads Python's `cbor2`
      encoded from the key tables, and each way it is refused; which claim wins; the chain and its
      cuts, with two claims of one epoch, a hub elected twice, a later claim that cuts no part of
      a device and a chain the replica holds only part of; and succeeding, refused when behind.
    - `keel-store`, 9 tests: claiming the first epoch; a claim's business date; a successor
      fencing what the hub it succeeds wrote after it claimed; a store behind the chain refusing to
      claim until it catches up; two claims of one epoch, the losing side's events numbered again;
      a hub elected again numbering what only a record its successor cut off numbered; a record
      before its term's claim counting for nothing; the integrity check finding a chain that
      isn't what the claims make; and the rows of the claims projection, its chain of terms and
      the sequence projection, pinned for a fixed history.
    - `keel-sync`: the heartbeat pinned byte for byte, six ways, at both ends of each field's
      range, in frames Python's `cbor2` encoded, and 22 ways it is refused; 18 tests of the
      election: the most preferred candidate claiming at 3 s, the standby at 8 s and not before,
      deferring to a preferred candidate, hearing the hub through a peer and no further, waiting to
      catch up, claiming after waiting, not waiting for what only the silent hub held, stepping
      down, going on hearing its hub beside a losing hub of its epoch, a heartbeat acting as
      another device's hub, resuming after a restart, resuming with the clock set back, at once
      and after a while, hearing of its last beat from two hops away, settling against every
      peer, a peer holding more of the log, clock jumps, and a forked replica; two tests of
      telling a fork; and a restart that sets the clock back taking no old acknowledgement for a
      new one.
    - `keel-sim`: without other faults, a crashed hub is succeeded within 5 s, in 8 seeds.
  - Property tests:
    - `keel-domain`: the chain and its cuts against a model of the rules, whatever order the
      claims arrive in; claiming against a model of succeeding; epochs up to the largest; and, at
      a location of three or four replicas that append, replicate a little at a time, claim and
      number, every event counting under at most one number, and in the end exactly one. The
      payload properties take the claim too.
    - `keel-store`: stores that claim, take each other's logs a few events at a time, number and
      confirm, against a model of the chain and of what each should number; the sequencing and
      ownership properties now run under claims.
    - `keel-sync`: the protocol property, rewritten for the election: replicas whose priorities
      may tie or be none, on a star, a line or a mesh, crashing, some down for seconds and a third
      of them starting again with their clock set back, under loss, delay, duplication and cuts.
      Frame by frame its model checks each heartbeat, each claim's preconditions, that a log
      settles exactly by the rule and a replica serves exactly while the rule says, that records
      are written only while serving, in the term's epoch, and that a replica takes a `have` for
      the acknowledgement of its last batch only if it acknowledges that very batch; once the
      replicas agree on all else after the faults stop, that those within two hops of the hub
      hear it at once; and at the end that the replicas agree, each having held the hub's term a
      while, with one hub serving, heard within two hops, and every event numbered once by the
      records that count. Eleven failures it found are named regression tests, five of them in
      the review, and a twelfth named case keeps one of the planted bugs in its reach.
    - 100,000 cases each passed, in release builds, alongside other runs: the protocol property
      in 24 minutes on the review's code (43 before it), the hub properties in 24 s (a million
      earlier, in 209 s), the payload properties in 3 minutes, and the store's sequencing
      property in 82 minutes, its terms property in 58 and its ownership property in 57.
  - The simulator: 5,000 seeds passed in a release build on the review's code, in 40 minutes on
    two threads, alongside other runs.
    - The candidates made 8,452 claims, more than one in 2,878 runs; the winning epoch was 1 in
      2,414 runs, 2 in 2,223, 3 in 349 and 4 in 14. 4,993 splits, in 3,325 runs, ran a hub on
      each side. Where the node serving as the hub crashed, in 762 runs, another claimed 3.5 s
      after at the median, 6.5 s at the 90th percentile and 24 s at most, the other faults
      delaying it.
    - Devices made 109,388 moves as islands, hearing no hub, in every run: no hub is heard until
      the first claim, three seconds in. A run of the same seeds before the heartbeat fix made
      114,072, and 29 more claims.
    - Devices made 47,506 requests for orders; hubs granted 40,085 and refused 1,338 for a moved
      lease, 7,828 for a payment in progress and 1 for the device owning the order already. 4,698
      overrides applied and 168 were stale; 1,412 grants were stale; 2,581 events were recorded
      by a device that didn't own the order.
    - The runs met 683,442 events appended; 17 million frames, of which 973,000 were lost,
      941,000 duplicated, 950,000 cut off and 287,000 sent to a node that was down; 3,472 crashes
      between writes, 3,517 in the middle of one, 3,711 rollbacks, 7,284 clock jumps and 7,575
      cuts; 2,758 events lost to rollbacks, held by no other replica, and 210 devices forked;
      20,658 writes held back until a log settled. 391,155 records that count numbered 735,300
      events, and replicas sent 4.4 million `durable` frames.
    - The replicas agreed after healing within 0.8 s at the median, 1.6 s at the 90th
      percentile, 2.3 s at the 99th, and 6.5 s at most.
  - Failover: in 1,000 seeds without other faults, the hub crashing at moments spread over a
    heartbeat period, the standby claimed 3.0 to 4.0 s after the crash, spread evenly, a quarter
    in each 250 ms, before the review and on its code alike.
  - Coverage probes, all passing:
    - the hub property, over 2,002 cases: chains of two terms or more in 46%, three in 11% and
      four in 1.6%; records that a later claim cut off in 40%, and records of a claim that lost
      in 74%; two claims of one epoch in 80%; a claim refused as behind in 2.3%;
    - the protocol property, over 1,000 cases of the review's code: more than one claim in 33%,
      an epoch of 3 or more in 7% and of 5 at most, two claims of one epoch in 10%, records that
      don't count in 24%, a candidate down for a second or more in 45%, a peer refusing a
      replica's log, a fork until it takes it, in 54%; clocks jumping in 67%, and two claims of
      one epoch with them in 7%; another heartbeat period than a second in 30%; a replica
      starting again with its clock set back in 43%, the serving hub in 13%; a hub given a floor
      at or above its clock's reading in 10%, nearly all while it served; and an
      acknowledgement from before a restart arriving after it in 62%;
    - the simulator, over 300 seeds: more than one claim in 177, the winning epoch past 1 in
      154 and past 2 in 26; 296 splits; 50 failovers; 6,438 moves made as islands; 2,928
      requests for orders, 331 overrides and 89 stale grants; 24,211 records counted; 18 devices
      forked.
  - Planted bugs, all caught, on the final code:
    - `keel-domain`: the 34 new ones, by the property tests; slice 3's 50 of ownership, run again
      against this slice's changes, 49 by the property tests and the one that only a unit test
      can reach, the largest lease; and the six that had gone stale, re-texted, by the property
      tests.
    - `keel-store`: 136, the 43 new or changed and every other one in a file this slice changed:
      110 by the property tests, and 26 that only unit tests can reach.
    - `keel-sync`: all 124, on the review's code: 90 by the property tests and the simulator,
      and 34 that only unit tests can reach, each confirmed against them. The review added 12,
      and left out one that no longer changed anything.
  - A CI-equivalent run passed every step before the last property and heartbeat changes:
    formatting, the planted bugs' texts, both lint runs, the tests with the exhaustive sweeps and
    4,096 cases a property (41 minutes alongside other runs), the tests without default features
    (37), the docs, the `wasm32` build, the currency table and the golden baskets. CI passed
    them all on the final code, and on the review's.
- **Decisions:** [ADR-0022](./adr/0022-hub-election-and-failover.md), accepted 2026-10-02 after
  review, with what building and reviewing it settled under "As built": a claimant holds the whole
  chain; the hub's beat in place of "hears the hub directly", and a heartbeat naming its hub's
  term, beats compared only within one term; the term's floor in every heartbeat, which a hub's
  beats go above; learning of a claim counts as hearing its hub; a peer's latest heartbeat acting
  as the hub kept; catching up on what peers other than the old hub hold; forks told by a peer
  refusing the replica's own log; priority 0 while unsettled or forked; every pair of candidates
  within two hops; and batches numbered from random bits at each start, amending ADR-0019.
- **Found and fixed during the build:**
  - The design let a successor cut only the devices it held claims of. With a gap in its chain,
    a claim of an earlier term it didn't hold could make records count again that a later claim
    had cut off; so could a field saying how far back its chain went. A claimant now holds the
    whole chain (the hub property found both).
  - A hub elected again numbered from after its latest record that counted: a later claim may
    cut off records written together, and their numbers with them, so it now numbers from after
    its latest record of its epoch (the hub property).
  - The design's heartbeat state "hears the hub directly", which a peer repeats for three periods
    after the hub falls silent, would have let a standby take over three periods late. Counting
    how many periods ago a peer heard the hub was a period out either way; the hub's beat replaced
    both (the protocol property).
  - A deposed hub whose epoch a new claim had just raised heard no hub of it, and claimed again
    at once: two candidates took the role from each other every second. Learning of a claim now
    counts as hearing its hub (the protocol property).
  - A hub restarting says it isn't acting until its log settles, which overwrote its last acting
    heartbeat, and it was taken for lost. Each peer's latest acting heartbeat is kept.
  - The design's fork test, a peer refusing its own log, was the wrong way round, and a refusal,
    once noted, never ended; a hub that began to serve again didn't number what came meanwhile;
    and replicas were taken to agree before the hub's beats had reached them all (the protocol
    property; each a named regression test).
  - The simulator missed a fork when the device's log had settled against the peers that
    answered (seed 104), and checked numbering over a forked log and orders only another version
    of it held (seeds 455 and 465): it now tells forks by what the other replicas hold, and leaves
    forked logs and their orders out of those checks.
  - Once its clocks jumped, the protocol property found two hubs of one epoch, which a split had
    left, masking each other: beats compared by epoch and number made the losing hub's, its clock
    3.8 s ahead, look newer than the winner's, and a replica that had both took its hub for silent
    and claimed again, four periods after the replicas had agreed. A heartbeat now names its hub's
    term, beats are compared only within one term, and the property checks every replica's
    hearing against the rule at every step, not only the claims it leads to.
  - Once crashes stopped falling at the same moment of a heartbeat period, a standby waited on the
    crashed hub's last word of its own log, and claimed 7 s after the crash, past the 5 s the
    design allows (the simulator). A candidate now waits only on what its other peers hold.
  - The first runs of `keel-sync`'s planted bugs missed four of 106:
    - a log settling while a peer held more of it: the protocol property took a replica's word
      that its log was settled. It now models settling, and the simulator checks that no log
      settles while a peer it heard from holds more of it;
    - the next tick forgetting rounds: heartbeats tick every replica each period, and the
      property's fell due with its rounds, so no round was ever late. A third of its cases now
      have another heartbeat period;
    - periods counted off the clock: the property's clocks never jumped. They now jump forwards
      now and then, by up to half a minute;
    - a heartbeat sent as another kind of frame: both ends read the kind they write, so only the
      pinned bytes can tell, and it is left to them.
  - Two of `keel-sync`'s bugs that only the known answers had caught, a log settling against the
    first peer to answer and one waiting for every peer, are now the property's to catch too, now
    that it models settling.
  - Six of `keel-domain`'s planted bugs had gone stale in slice 3, when the commands they plant
    in changed, unnoticed because only that slice's own bugs ran. They plant again, all caught,
    and CI now checks every list (`--stale`).
- **Found and fixed in review** (2026-10-02):
  - A hub's beat began again from its clock's reading as it restarted. A hub whose clock was set
    back while it was down, as a clock without its battery is, beat below its last beats, and its
    peers took it for lost while it served, for as long as the clock had gone back: islands, or a
    standby claiming needlessly. Heartbeats now carry their term's floor, the latest beat of it
    their sender knows, however old; replicas keep the latest floor they hear; and a hub's beats
    go on above every floor naming it. Found reading the code; known answers now pin a hub
    restarting at once and after a while, and hearing of its last beat from two hops away.
  - The protocol property had never set a clock back. A third of the replicas that crash now
    start again with their clock set back, and once the replicas agree on all else after the
    faults stop, those within two hops of the hub must hear it at once, not only in the end. It
    then finds the bug above on its own, even with its model sharing it. Against a first fix,
    which gave the hub of a replica's term the latest beat the replica knew, it found a replica
    two hops away that knew the hub's last beat after the peer between had restarted: hence
    floors that every replica passes on.
  - With clocks set back, the property found slice 1's batch numbers repeating. Batches were
    numbered from the clock's reading as the replicator started, so a restart that set the clock
    back numbered its batches as the run before had, and a peer's late acknowledgement of one of
    those passed for a new one; in this slice that can make a replica take its own log for
    forked, and a hub stop serving. Each start now numbers its batches from random bits the
    caller draws (`Replicator::start`'s `nonce`), and the property checks that a replica takes
    an acknowledgement only for the very batch acknowledged. A first fix, numbering past a
    peer's last acknowledgement from an earlier run, failed when two quick restarts numbered
    past the same one.
  - Its 100,000-case soak then found the new hearing check asking too much. A replica that took
    on the hub's term late, after the hub had restarted with its clock set back, knowing a later
    beat of the term from before, heard no hub for a moment: its floor took four periods to reach
    the hub and the hub's next beat to come back, one more than learning of the claim counts as
    hearing. That is the protocol working, a period late at most; agreement now counts only once
    every replica has held the hub's term for the periods of silence and two more, and ADR-0022
    records the delay.
  - The rows of the sequence projection (slice 2), and of the claims projection and its chain
    of terms, weren't pinned, though the projections' docs said every projection's were: a
    change to what they make of the events, without a new version, would have left stores'
    rows stale. They are pinned now.
  - Two doc comments ran past the line width, and one split a paragraph in two.
  - Run again on the review's code, `keel-sync`'s planted bugs found one that the protocol
    property had reached only by chance, and no longer did once the review changed how crashes
    are drawn: a replica passing on, as its own term's beat, a beat its term's hub gave acting in
    a later term. A named case now reaches it every time (`f5da291`). Another, a beat without a
    term accepted, could no longer change anything, since a beat comes with a floor and a floor
    only with a term; it is left out of the list.
  - Left for later (see "Known gaps"): a peer refusing a replica's log reads as a fork, whatever
    the peer's reason.
  - The final code passes the known answers and both lint runs; 100,000 cases of the protocol
    property, 5,000 simulator seeds and the failover probe, above; every `keel-sync` planted
    bug; and CI.

### Ownership leases: step 6, slice 3 (2026-10-01)

Commit `710ed27`, with CI green on it (33.6 minutes). Reviewed 2026-10-01.

- **Built** ([ADR-0021](./adr/0021-ownership-leases.md)):
  - `keel-domain`:
    - Four order events, version 1: `order.ownership_requested`, `_granted`, `_refused` and
      `_overridden`. `Lease` counts an order's changes of owner, from 0; `Epoch` is the hub's.
    - The order's fold keeps its owner, lease and waiting requests. The creator owns the order
      under lease 0; a grant or an override applies only from the current lease. An answer
      names its request wherever it folds. Four new conflicts: `NotOwner`, `StaleGrant`,
      `Overridden` and `StaleOverride`.
    - `Order::decide` takes the device: commands that need ownership are refused to any other
      device, `NotOwner`, and requests and overrides are commands of their own. Checkout's
      `start_payment` and `close_check` need ownership too.
    - `Order::answers(paying, epoch)`: the hub's grants and refusals, a pure function of the
      order and whether a payment of it is in progress.
  - `keel-store`: the orders projection, version 2, with each order's owner, lease and waiting
    requests, and `Store::answer_requests(epoch, now)`, the hub's answers in a write of their
    own, each caused by the request it answers.
  - `keel-sync`: `Replica::answer_requests`. The hub answers whenever it would sequence, then
    numbers its answers with everything else.
  - `keel-sim`:
    - A move that needs an order another device owns becomes a request, or, from a device cut
      off from the hub, an override. Devices also ask for each other's orders, and half the
      moves on other orders go to the latest two, so that devices contend for them.
    - Three devices in half the runs, up from three in ten.
    - Four new invariants, checked at the end of each run, and the hub's answers and the
      orders' ownership conflicts counted in its report.
  - CI: the job's time limit rises from 30 minutes to 45. Slice 2's commit took 28.6 of them,
    in two test passes of 14 minutes each, and this slice's tests add to both: its own commit
    took 33.6.
- **Verified:**
  - Known answers:
    - `keel-domain`, 14 tests of ownership:
      - the four payloads pinned byte for byte, in payloads Python's `cbor2` encoded from the
        key tables, and each way each is refused;
      - the fold: the creator's ownership, a grant, a stale grant, a refusal, an override and a
        stale one, every owner-only event recorded by another device, an answer before its
        request, and one before the order's creation;
      - each command's ownership check, requests one at a time, and a closed order asked for;
      - each of the hub's answers;
      - leases and epochs in range.
    - Checkout: only the owner starts a payment or closes a check, and a payment's outcome is
      recorded whoever owns the order by then.
    - `keel-store`, 5 tests: the hub granting, every store seeing the new owner; a payment in
      flight holding the order where an authorized one doesn't; two requests from one lease;
      a request waiting for its order's creation; an interrupted answering leaving nothing.
    - `keel-sync`: a hub, in epoch 2, answering a request it receives and numbering its answer
      in the same pass.
  - Property tests:
    - `keel-domain`:
      - the payloads against the codec's model, with a property of their own changing one field
        at a time, and a sweep of every lease and epoch across the ends of its range;
      - the fold against a model of ownership, over requests, grants, refusals and overrides
        from three devices and the hub in any canonical order, sometimes before the order's
        creation;
      - the hub's answers against a model of their own, the grants a chain from the owner, with
        a payment in progress or not; folded after everything, they leave no request waiting;
      - commands tried from the owner, from each device whose request waits, and from a device
        with none, on every state merged orders pass through; a device asking twice, then
        overriding, with the requests it leaves and their leases the model's;
      - checkout's decisions for a device that doesn't own the order.
    - `keel-store`: a hub in an epoch of its own takes in two devices' logs a few events at a
      time, asks for orders itself, and answers, sometimes interrupted. Each answer must be the
      order's rules applied to what it holds; at the end every request has one answer, and
      another replica, taking every log in any order, agrees.
    - `keel-sync`: the protocol property, its model replica asserting that the hub answers
      before it numbers, every time.
    - 100,000 cases each passed, in release builds: `keel-domain`'s payload properties in
      112 s, its order properties in 211 s and checkout's in 85 s; the store's ownership
      property in 40 minutes and its projection properties in 29; and the protocol property in
      11 minutes.
  - The simulator: 5,000 seeds passed in a release build, in 46 minutes, alongside other runs,
    three devices in 2,438 of them.
    - Devices made 43,783 requests and 3,173 overrides. The hub held 43,219 of the requests,
      the rest lost to rollbacks, and answered each once: 34,975 grants, 1,205 refusals of a
      moved lease, 7,039 of a payment in progress, and none of an owner, which the simulator's
      star rules out.
    - In the orders' folds on the hub: 2,783 overrides applied, in 1,812 seeds; 192 stale
      overrides, in 177; 243 stale grants, in 221, each after an override of the lease it
      replaced; and 3,298 events recorded without ownership, in 1,617.
    - The runs met what slice 2's did: 672,198 events appended; 5.2 million frames, of which
      265,000 were lost, 287,000 duplicated, 124,000 cut off and 71,000 sent to a node that was
      down; 3,530 crashes between writes, 3,422 in the middle of one, 3,633 rollbacks, 7,526
      clock jumps and 7,575 cuts; 5,159 events lost to rollbacks, held by no other replica, and
      528 devices forked. The hub wrote 406,327 records, numbering 700,104 events, and replicas
      sent 1.1 million `durable` frames.
    - The replicas agreed after healing within 1.5 s at the median, 2.1 s at the 90th
      percentile, 5.1 s at the 99th, and 10.1 s at most.
  - Coverage probes, all passing:
    - the fold property, over 1,000 cases: stale grants in 81%, applied overrides in 40%, stale
      overrides in 73%, events flagged `NotOwner` in 76%, an answer before its request in 71%,
      leases of 2 or more in 41%; the hub's answers refused a moved lease in 46%, an owner in
      7% and a payment in progress in 15%, granted in 15%, and granted twice in 1.8%;
    - commands, over 1,000 merged orders: a request from another device waited in every one,
      and in 69% of the 22,500 states tried; the device carrying on had its first request
      accepted and its second refused as pending in 649, the rest ended orders;
    - checkout: of 7,206 payments a device that didn't own the order tried to start, 75% were
      refused for that, and 36% of 9,430 closes; the others failed earlier rules first;
    - the store property, over 1,000 cases: 4,053 answering passes, 1,789 with answers: 553
      grants, 3,009 refusals of a moved lease, 647 of an owner and 355 of a payment in
      progress; 760 passes answered both orders, 67 granted twice; 993 answers were
      interrupted, 325 with answers to lose; 505 of 3,000 passes held a request for an order
      not yet created.
  - Planted bugs, all caught: 51 in `keel-domain`, 15 in `keel-store` and 4 in `keel-sync`.
    The property tests alone catch all but two: no fold reaches the largest lease, after 2^63 − 1
    changes of owner; and every simulated hub is in epoch 1, so only the known answer, in epoch
    2, sees a store answering in the wrong one. Of `keel-sync`'s other three, the simulator
    catches all three and the protocol property two: the third is the store's, which its model
    replica stands in for.
  - A CI-equivalent run passed every step, in 34 minutes alongside the soaks: formatting, both
    lint runs, the tests with the exhaustive sweeps and 4,096 cases a property (17 minutes), the
    tests without default features (16), the docs, the `wasm32` build, the currency table and
    the golden baskets.
- **Decisions:** [ADR-0021](./adr/0021-ownership-leases.md), accepted 2026-10-01 after review,
  with what the build settled under "As built":
  - refusals coded from 0, as every code is, and an override's reason with an optional note;
  - an answer names its request wherever it folds, even before the order's creation;
  - the order of the checks: the order's state, then ownership, then the command's own rules,
    and the hub's lease, then device, then payment;
  - the hub's answers recorded by `System("hub")`, caused by their requests, under the order's
    business date;
  - the simulator's devices contending for the latest orders, and three of them in half the
    runs.
- **Found and fixed during the build:**
  - The design assumed a request folds before its answer. A store keeps an event from a device
    whose clock is too far ahead without moving its own clock past it, so the hub's answer can
    sort first: the request would have waited for ever, and the hub answered it at every write.
    The fold now remembers every request an answer names.
  - Reviewing the fold found the same with an answer before the order's creation, which the
    fold sets aside: a hub more than a minute behind two devices would answer the same request
    at every write. Answers are now noted before anything else.
  - Writing the planted bugs showed that `keel-store`'s property uses the order's own rules as
    its model of the hub's answers, so no property checked the rules themselves.
    `keel-domain`'s ownership property now has a model of the answers.
  - The first run of the planted bugs missed nine of 50, in five places the property tests
    didn't reach:
    - a grant's lease or epoch of 0 came up once in thousands of cases: a payload property for
      the four schemas, and a sweep of their numbers, now reach them every time;
    - every racing device overrode the order first, so no merged order had a request waiting:
      one device in two now asks first, commands are tried from each device whose request
      waits, and the device carrying on asks twice before it overrides;
    - no command anyone may make was tried from another device;
    - requests were compared by device, not by the lease they name;
    - checkout's model ignored ownership.
  - The first simulator runs never met the race that makes a grant stale, in 64 seeds: devices
    spread their moves over too many orders, and three devices were too rare. Moves on other
    orders now go to the latest two half the time, an island always overrides, and three
    devices run in half the seeds: 12 stale grants in 256 seeds.

### Hub sequencing: step 6, slice 2 (2026-09-30)

Commit `7b6bd52`, with CI green on it (28.6 minutes). Reviewed 2026-10-01.

- **Built** ([ADR-0020](./adr/0020-hub-sequencing.md)):
  - `keel-domain`: `sequence.assigned` v1, the hub's sequencing record. It holds an epoch, the
    number of its first event, and 1 to 1,024 runs, each a device, a stretch of its log, and the
    hash of the stretch's last event. Every rule is judged in one place, for records made and
    records decoded alike.
  - `keel-events`: `LogWriter::generate_id`, a new aggregate's identifier drawn from the log
    writer's entropy, so that simulated runs stay deterministic.
  - `keel-store`:
    - `Store::sequence`: for each device, the store numbers the events after the last position
      any record covers, records aside, in the order it received them, from where its own last
      record of the epoch left off. It writes several records where a device's runs wouldn't
      follow on, or a record is full.
    - The `sequence` projection, a row per run, and confirmation by hash: `confirmed()`,
      `store_seq()`, and `sequenced()`, the feed. Indexes on each run's length bound every
      search.
    - The full check compares every row of a stream, now that a stream may have several.
  - `keel-sync`:
    - A replicator's roles: the Store Hub numbers after every write that stores events it
      received, after its own appends, and when its log settles; never before.
    - `store_durable()`: how far into the device's own log a peer has said it holds.
    - The `durable` frame, and the watermark: the durable peer's `have`, kept at its highest and
      relayed to the other peers.
  - `keel-sim`: the hub sequences in epoch 1 and names the cloud its durable peer, and the
    simulator checks the new invariants, some as a run goes and the rest at its end.
- **Verified:**
  - Known answers:
    - `keel-domain`, 9 tests: the payload pinned byte for byte in two payloads Python's `cbor2`
      encoded; each rule a record is refused by, made or decoded; strict reading; the registry.
    - `keel-events`: identifiers drawn between events are fresh and in order.
    - `keel-store`, 11 tests:
      - numbering in the order received, and records ending where a device doesn't follow on
        or a record is full;
      - confirmation by hash, a forked version never confirmed, and the feed;
      - records that disagree, where an event's first number counts;
      - the confirmed start of a log;
      - an interrupted sequencing leaving nothing, and each epoch numbering from 1;
      - the check finding a run that isn't what its record says.
    - `keel-sync`: the `durable` frame pinned byte for byte, and 5 ways it is refused. Six
      replicator tests, frame by frame against the model replica:
      - the hub numbering what it stores, once its log is settled, and a replica that isn't the
        hub numbering nothing;
      - store durability, as far as a peer holds the device's log;
      - the durable peer's `have` as the watermark, and a `durable` frame raising it, each
        passed on;
      - the watermark with every round, and in answer to a `have` that asks.
  - Property tests:
    - `keel-domain`: records against the codec's model, as for every schema; spoiled records
      refused exactly when the model refuses them; the numbers a record gives, counted run by
      run; and records around the 1,024-run limit.
    - `keel-store`, against a model:
      - a hub takes in two devices' logs and appends events of its own, sequencing now and
        then, sometimes interrupted;
      - another replica takes in the hub's log and the devices' logs in any interleaving, one of
        them perhaps forked;
      - that replica then sequences in the next epoch, and the hub takes in its records.

      After every step, each store's numbers, confirmed starts, and the feed of each epoch and a
      page of it, must be the model's.
    - `keel-sync`: the protocol property, with one replica the hub and one durable. All along,
      every `durable` frame claims no more than the durable replica holds and never goes to it,
      and every watermark only rises. At the end, every replica's watermark reaches what the
      durable replica holds.
    - 100,000 cases each passed, in release builds: `keel-domain`'s 18 payload properties in
      80 s, the protocol property in 13 minutes, and the store's in 54.
  - The simulator: 5,000 seeds passed in a release build, in 43 minutes, alongside other runs.
    - The hub wrote 367,065 records, numbering 597,945 events, and replicas sent 961,128
      `durable` frames.
    - The runs met what slice 1's did: 613,260 events appended; 4.6 million frames, of which
      240,000 were lost, 256,000 duplicated, 116,000 cut off and 64,000 sent to a node that was
      down; 3,524 crashes between writes, 3,410 in the middle of one, 3,627 rollbacks, 7,526
      clock jumps and 7,575 cuts; 5,317 events lost to rollbacks, held by no other replica, and
      535 devices forked.
    - The replicas agreed after healing within 1.4 s at the median, 2.1 s at the 90th
      percentile, 5.1 s at the 99th, and 10.9 s at most, now that agreeing includes every
      watermark reaching what the cloud holds.
  - A coverage probe over 1,000 seeds, all passing:
    - the hub wrote 72,983 records, numbering 118,617 events, 1.6 a record;
    - 57,239 runs reached a device before the events they cover, in 998 seeds;
    - the hub was interrupted in the middle of a replicator's write 166 times, in 158 seeds;
    - devices were told 118,788 events were store-durable. Rollbacks then took 5,443 of them
      from their devices, in 525 seeds, and none was lost: the hub gave them back;
    - 1,653 of forked devices' own events stayed unconfirmed where the devices held them, in
      87 seeds, as they should;
    - of 194,241 `durable` frames, 12,326 (6%) went back to the hub from its devices, and none
      to the cloud.
  - Planted bugs, all caught: 22 in `keel-domain`, 27 in `keel-store`, 14 in `keel-sync`, and 1
    in `keel-events`, caught by its unit test.
    - The property tests alone catch 46 of the 63 in the first three: `keel-domain`'s 20,
      `keel-store`'s 17, and `keel-sync`'s 9. The protocol property catches all 9 of those, and
      the simulator 5 of them.
    - Only the unit tests reach the other 17:
      - 6 need records that only a second sequencer or a misbehaving device writes: two
        devices' records in one epoch, records that disagree about an event, or a device that
        isn't the hub writing a record between its own events;
      - 2 are refused by another rule as well, and only the error's rule name tells them apart;
      - a record of 1,024 runs, an epoch out of range, a record's business date, and a
        projection row changed behind the store's back;
      - the hub numbering before its log is settled, which can't fork the numbers while the
        simulator never rolls the hub back;
      - store durability, where the simulator's devices each have one peer;
      - a watermark held back until the next round, twice;
      - a watermark from another location, where every replica is at one.
  - A CI-equivalent run passed every step, in 28 minutes: formatting, both lint runs, the tests
    with the exhaustive sweeps and 4,096 cases a property, the tests without default features,
    the docs, the `wasm32` build, the currency table and the golden baskets.
- **Decisions:** [ADR-0020](./adr/0020-hub-sequencing.md), accepted 2026-10-01 after review,
  with what the build settled under "As built":
  - what the hub numbers, for each device: the events after the last position any record
    covers, which holds even when the hub's own records come back from its peers;
  - a record ends where a device's runs wouldn't follow on;
  - confirmation worked out as the store is read, from a row per run, never per event;
  - where records disagree, an event's first number counts;
  - the watermark relayed on every rise, with every round, and in answer to a `have` that asks.
- **Found and fixed during the build:**
  - Seed 1: the simulator marked store-durable events by position. A device rolled back wrote
    its lost positions again, and a later mark made an old, lost event look store-durable. Marks
    now go to the event the device held at each position when it was told.
  - The first sequencing read every event, and the hub's own records again, at every write:
    37 ms a write at 20,000 events. It now reads only each device's events after its last
    covered position, and the hub's own log after its latest record: 1.6 ms.
  - The first run of the planted bugs found `keel-domain`'s test records too regular: the
    property tests missed 7 of 22 bugs, and the unit tests one, a decoder keeping only the first
    byte of each run's hash, since every hash they used was one byte repeated. Records now have
    up to three devices taking turns, runs near the largest position as well as the first, and
    hashes whose bytes all differ; spoiled runs reach each end of the range; and one property
    builds records around the 1,024-run limit, which random records almost never reach.
  - After the store's queries were rewritten for speed, a planted bug that numbers records went
    unnoticed: a hub on its own never reads its own records again. The store's property now
    has another replica sequence in the next epoch, reading the hub's records. That also leaves
    gaps in what a replica holding a forked log has confirmed, so the property now catches a
    confirmed start that passes a gap, which only a unit test caught before.
  - The CI failure that the SQLCipher race caused, below.

### A race opening a process's first stores (2026-09-30)

Found when CI failed on `28aed7a`, the commit accepting ADR-0019. Fixed in commit `0118401`, with
CI green on it (19.8 minutes), on ADR-0019's acceptance and ADR-0020's design with it.

- **What failed:** 1 ms into the `keel-store` unit tests, as their threads opened their first
  stores at once, one store didn't open. SQLCipher refused its key: "An error occurred with
  PRAGMA key or rekey". The same code had passed CI twice.
- **Why:** SQLite runs SQLCipher's initialization only after it has marked itself initialized
  and let other threads on. A connection keyed on another thread meanwhile finds SQLCipher not
  ready, and its key is refused. SQLCipher, up to 4.19, still initializes this way.
- **Fixed:** the store opens the process's first connection alone, under a `Once`, before every
  connection it opens (`keel_store::init_sqlite`); other threads wait for it. The tests' own
  connections call it too, and so must any code that opens SQLite connections in a process with
  stores. ADR-0018's "As built" records it.
- **Verified:** a new test opens 32 stores at once on 32 threads, as its process's first use of
  SQLite. Run 2,000 times, four processes at a time, alternating with the same test built
  without the fix: without it, 44 runs failed; with it, none. Of 1,000 more runs without it, 19
  failed, each with SQLCipher refusing the key of 1 to 23 of the 32 stores, as in CI. The race
  needs the thread initializing SQLite to stop within a few instructions, so no test can force
  it: the test catches it only as often as it happens, about 1 run in 50 on a loaded machine and
  1 in 200 on an idle one. So no planted bug stands for it: the fix closes it by construction.

### Replication and the simulator: step 6, slice 1 (2026-09-30)

Commit `c5240e1`. Reviewed 2026-09-30.

- **Built** ([ADR-0019](./adr/0019-replication-and-deterministic-simulation.md)):
  - `keel-sync`, replication v0: anti-entropy by version vector, correct over any transport,
    with no I/O of its own. The caller hands it each frame received and a tick when
    `next_tick` asks, and sends the frames it returns.
  - Two frames, `have` and `events`, in canonical CBOR. A replica tells each peer what it holds
    when it starts, after each batch it receives, and every round.
  - It sends each peer one batch at a time of what the peer lacks, the peer's own log first
    unless the peer refused it. It pushes new events at once, never echoes them back, and after a
    batch the peer took none of, waits for its next round.
  - Until it hears from a peer, its `have` asks for the peer's in return, which comes at once.
    So a device settles its own log, holding what its peers hold of it, before it writes.
  - It works through a `Replica` trait, implemented for `keel-store`; a model replica in memory
    serves the property tests.
  - `keel-sim`, the deterministic simulator: two or three devices, the hub and the cloud, each
    with a real encrypted store on a RAM disk, in one thread and virtual time, with every choice
    drawn from one seed.
    - Faults: frames lost, duplicated and reordered; partitions; crashes between writes and in
      the middle of one; stores restored from older copies; clocks offset and jumping.
    - Workload: devices ring and settle orders through `keel-domain`'s commands.
    - Checks: each batch against the protocol's rules as it is sent. After healing, the
      replicas must agree within a minute; then convergence, no loss, causality, forks and
      quarantine, and every store's full check.
    - A failing seed prints the command that replays it, and `KEEL_SIM_LOG` prints every
      action.
  - `keel-events`: an event is at most 256 KiB. The log writer won't make a larger one, and the
    registry refuses one, which the store quarantines.
  - The planted-bug runner can run other crates' tests with a crate's, and leave its known
    answers out of `--props`.
- **Verified:**
  - Known-answer tests:
    - frames pinned byte for byte, and 22 ways a frame is refused;
    - 17 replicator tests, frame by frame against the model replica: starting, asking and
      answering, batches and their acknowledgement, shares and the peer's own log first, stalls
      and the rounds that end them, timeouts, echoes, settling, a replicator out of step with
      its store, a clock set back, and the regressions of seeds 162 and 1645;
    - two against real stores, one interrupted in the middle of a write;
    - the event size limit, at the writer and the registry, to the byte.
  - The protocol property. Cases: 2 to 4 replicas as a star, a line or a mesh, over a network
    that delays every frame, loses, duplicates and cuts, with restarts, replicas declining
    batches, and batches of 1 to 6 events, some with too few bytes for two.
    - At the end, every replica holds exactly the events appended, has refused none, and has
      its log settled.
    - All along, every frame keeps the protocol's rules: no event the peer has said it holds
      or sent; batch sizes; one batch at a time; nothing more to a peer that took none of the
      last batch until the next round, which comes when due; fresh batch numbers, even across
      restarts; acknowledgements and answers at once; asking exactly until the peer is heard
      from; no tick that does anything before `next_tick` said.
    - 100,000 cases passed.
  - The simulator: 5,000 seeds passed in a release build, in 34 minutes. What they met:
    - 612,799 events appended, by 17 kinds of move;
    - 2.4 million frames, of which 161,000 were lost, 136,000 duplicated, 54,000 cut off and
      25,000 sent to a node that was down;
    - 3,525 crashes between writes, 3,408 in the middle of a write (in 2,504 seeds), 3,629
      rollbacks (in 2,880 seeds), 7,526 clock jumps and 7,575 cuts;
    - 13,593 events received after a gap, 177,201 duplicates, 119,518 timeouts, and 1,078
      stalls, in 497 seeds;
    - 5,112 events lost to rollbacks, held by no other replica, in 1,056 seeds; 528 devices
      forked, in 509 seeds; 24,961 writes held back until a log settled;
    - agreement after healing within 1.2 s at the median, 2.0 s at the 90th percentile, 5.0 s
      at the 99th, and 8.5 s at most.
  - Planted bugs: 50 in `keel-sync`, all caught, and 4 in `keel-events` at the size limit,
    caught by its unit tests.
    - The property tests alone catch 33 of `keel-sync`'s: the protocol property 29, 10 of them
      alone, and the simulator 23, 4 of them alone. Those 4 need a real store or a rollback:
      forgetting that a restored peer holds less, a log settled from the start or when a peer
      holds more, and the store adapter skipping an event.
    - Only the unit tests reach the other 17:
      - 8 frames or inputs that no replica or test network makes;
      - 3 bugs of a replicator out of step with its store;
      - 2 in how a batch is shared out, which is fairness, not correctness;
      - the pace of `have` repeats before a peer is heard from;
      - a clock set back further than the simulator's clocks go;
      - a batch half stored;
      - seed 1645's bug, which the simulator meets in about 1 seed in 1,000.
  - CI green.
- **Decisions:** [ADR-0019](./adr/0019-replication-and-deterministic-simulation.md), accepted
  2026-09-30 after review, with the details the build settled under "As built". Among them:
  - the fold order: every replica folds in HLC order, and the hub's order will confirm, never
    reorder, amending offline-and-sync §4;
  - a `have` asks for one in return until its sender has heard from the peer;
  - batches carry the peer's own log first, unless the peer refused it;
  - rounds keep their own clock;
  - events are at most 256 KiB;
  - the simulator's faults, its workload, and the rules it checks as runs go.
- **Found and fixed during the build:**
  - Seed 13: a device restored from an older copy, whose one `have` at startup was lost, waited
    a round (5 s) for its peer, longer than its settle wait (3 s), and forked its log. A replica
    now repeats its `have` every 2 s until it hears from a peer.
  - Seed 162: a hub stalled towards a device for good. The device kept sending batches the hub
    refused, and each acknowledgement counted as the round that ends a stall, so the round never
    came. Rounds now keep their own clock, and a named regression test pins it.
  - The first coverage probe showed 294 seeds in 1,000 forking a device's log. A replica that
    started learned what its peers held only at their next round: now its `have` asks for
    theirs, which cuts forks to 98. Seed 17 then showed a restored device getting its own
    events back at half speed, sharing each batch with another device's: batches now carry the
    peer's own log first, and 88 seeds fork. The three examined were islands by force, their
    links to the hub cut through the settle wait.
  - Seed 1645, in the soak: putting a peer's own log first starved a device that had forked
    its log. The hub's version of that log filled every batch, the device took none of each,
    and it never got another device's events. A peer's own log that it refused now takes a
    share like any other's, and a named regression test pins it.
  - A replica that sends a peer its refused batch again as soon as the peer acknowledges it met
    every final invariant. So the simulator now checks each batch against the protocol's rules
    as it is sent, and the protocol property checks every frame.
  - The stall rules were met only in the simulator, whose 32 seeds missed seed 162's bug: it
    fails 12 seeds in 500, the first of them seed 84. The protocol property now has replicas
    decline batches, so senders stall, and checks the stall rules too; it catches the bug, and
    every other planted stall bug.
  - A planted bug that took any later position of a device's log for its next went unnoticed:
    the known answer for a replicator out of step with its store had only one device out of
    step. It now has two.
  - The store adapter storing each event of a batch in a write of its own went unnoticed too:
    the known answer interrupted the write at the first event, where one write and many roll
    back alike. It now interrupts at the second.
  - `next_tick` was tested only by known answers. The simulator now ticks each node when it
    asks, and the protocol property checks that no tick does anything sooner.

### Encryption at rest and integrity checks: step 5, slice 3 (2026-09-30)

Commit `fcef53f`. Reviewed 2026-09-30.

- **Built** ([ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md)):
  - encryption at rest: SQLCipher 4.14, compiled in with OpenSSL 3.6 built from source, the same
    on every platform. Every page, the WAL's too, is encrypted with AES-256 and authenticated
    with HMAC-SHA512, with SQLCipher 4's settings pinned, and temporary storage stays in memory.
    The store opens only with its key: 32 bytes the platform keeps safe, zeroed when dropped;
  - changing the key, in one transaction, proved by opening the file again: SQLCipher's rekey
    reports success when it fails, so the store never trusts it;
  - a damaged store fails closed: a page that fails its authentication, or a file SQLite finds
    malformed, makes the store refuse everything until it is reopened, and it never returns
    what it read there;
  - checks: opening checks what costs the same whatever the store holds, and says whether the
    store was closed cleanly last time; a full check, for when the device is idle, reports every
    problem with the file's pages, SQLite's structure, the events and each device's log, the
    store's identity and clock, the projections against a rebuild, the outbox and the
    quarantine;
  - fault points for rekeys. The error for a signer that didn't sign the device's events is now
    `WrongSigner`, beside the errors about the store's key.
- **Verified:**
  - Known-answer tests: 20 more in `keel-store` (53 in all). Among them: nothing in the clear in
    the database or its WAL; only the store's key opens it, and neither an unencrypted database
    nor a file of garbage opens; the pinned settings; rekeys, interrupted and failed ones
    included; a journal left behind; a damaged page failing the store closed, and one that
    reads as zeros; a damaged first page; files cut short and added to; each change behind the
    store's back the check finds, an event from another location, and a forged event whose
    clock runs backward; a malformed database, which stops the check; a check another reader
    holds up.
  - The golden store: made once with a fixed key, kept in the repository, and read back as it
    was made, against pinned heads and a store stocked afresh.
  - Property tests against models:
    - keys: only the store's key opens it, through writes, rekeys, interrupted and refused
      rekeys, and reopenings with four keys, and it holds what was written;
    - damage to the file: bits flipped anywhere, bytes added or cut. Every read, and every
      write that loads an aggregate, gives what the sound store gave or reports the damage, and
      after it, so does every read and write; a damaged first page refuses the key; the check
      reports exactly the damaged pages;
    - changes behind the store's back, with the key: events changed, refiled, deleted or
      forged, streams dropped, projection rows changed, added or deleted, effects' causes and
      attempts, the clock, quarantined digests and the store's identity. The check reports
      exactly the problems a model predicts, and the same again.
  - Every earlier test now runs on encrypted stores. The crash tests change the key halfway
    through their workload. After every crash, exactly one key opens the store, it knows it
    wasn't closed cleanly, and its full check finds nothing. Kills spread over a rekey of about
    4 ms left the old key in 3 cases and the new one in 13.
  - Coverage probes, over 1,000 cases:
    - damage to the file: none in 157, bits flipped in 765, bytes added in 164 and cut in 159
      (a whole page in 81); several pages in 603, overflow pages in 265, the last page of a long
      value, which reads as zeros, in 107, and the first page in 52. The key refused in 52, the
      damage met by opening in 439, by a read in 250 (a write, first, in 43), and by the check
      alone in 102;
    - changes: every kind of problem, in 79 to 396 cases each; several problems at once in 456;
      a forgery breaking the link after it in 108; a changed row in a stream with an unreadable
      event, which can't be judged, in 23; a stream that lost some of its events in 153, and all
      of them, with its row, in 75;
    - keys: two rekeys or more in 646, back to an earlier key in 364, to the same key in 478; a
      reopening right after a rekey in 392, after an interrupted one in 165, trying another key
      first in 676; a write right after a rekey in 538.
  - Planted bugs, all caught: 139 in `keel-store`, 45 of them new, aimed at the key, encryption,
    damage and the check, and 9 re-aimed at code this slice changed. The property and crash
    tests alone catch 120, two of which only the unit tests reached before: a journal that isn't
    the WAL, which the crash tests now see in a crashed store that looks closed cleanly, and a
    stored event's hash left unchecked, which the full check now meets. The other 19 only the
    unit tests reach: settings and versions nothing else can see, invalid input, and failures
    only a unit test provokes, such as a rekey SQLCipher can't do.
  - Every property test in `keel-store` passed 100,000 cases.
  - CI green.
- **Decisions:** [ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md), accepted
  2026-09-30 after review, with the details the build settled under "As built":
  - opening tells a key that doesn't open the store from a file SQLite finds malformed. When a
    journal was left behind, the store checks the key through a connection that can't write
    first, since one that can would tidy the journal away, the only sign of a crash;
  - after each read, one more, from the cache, proves the connection met no damage, at a cost
    of about 2 µs; a damaged store refuses everything until it is reopened, and its check opens
    the file afresh;
  - costs, against the same store unencrypted on the development machine: a write of one event
    90 to 210 µs more, the more pages it touches; for a store of 100,000 events, 72 MB, opening
    in 0.8 ms, the full check in 2.7 s, and a rekey in 1.1 s;
  - the check reports its problems in order, each by page, row, device and position, projection
    and stream, or effect, and stops after damaged pages or a malformed database;
  - the golden store of schema version 2, 94 KB, holds something of everything the store keeps.
- **Found and fixed during the build:**
  - SQLCipher hands SQLite a page that fails its authentication as zeros, and fails the reads
    after it. The damage property test found the last page of a long value read back with zeros
    in it and no error: events caught it by their hashes, but an effect's payload or a
    quarantined message wouldn't have. The store now proves every read with one more.
  - A connection that can write moves the WAL into the database and deletes it as it closes,
    even when its key was refused: a shell trying its current key, then the pending one, after
    a crash during a rotation, would have hidden the crash. The store now checks the key through
    a connection that can't write first, when there is a WAL.
  - Opening took any failure of its first read for a refused key, so a file cut short by a whole
    page was reported as `KeyRejected`; it is now `Damaged`.
  - Planted bugs the tests first missed:
    - the tests' keys were 32 equal bytes, so a key given in reverse was the same key. They now
      differ, and the golden store was made again;
    - no property test met damage first in a write: the damage property's reads now include
      writes that load an aggregate;
    - SQLCipher's log level is the process's, which the tests' own connections set too: the
      crash test's child, a process of its own, now reports the level its store left.
  - Two planted bugs changed nothing anyone could see, and were planted again: dropping the
    check that temporary storage is in memory, while still asking for it; and the check keeping
    one stream's rebuild, which the rollback of its transaction undid anyway.
  - The golden store's maker, a test ignored by default, would have run in CI, which runs
    ignored tests: it now runs only when an environment variable asks.

### Projections and the outbox: step 5, slice 2 (2026-09-29)

Commit `7a85745`. Reviewed 2026-09-29.

- **Built** ([ADR-0017](./adr/0017-projections-and-outbox.md)):
  - projections: a row for each order and each payment, folded with `keel-domain`'s folds from
    the stream's events in canonical order. Each write recomputes the rows of the streams it
    touched before it commits, so a late event needs nothing more; a projection whose version
    changed is dropped and rebuilt from the log when the store opens;
  - reads: an order or a payment by identifier, the orders in a state, an order's payments, and
    any aggregate loaded from its stream, inside a write or outside one. A write reports the
    streams it touched;
  - the outbox: effects with an idempotency key, a kind, a payload and the event that caused
    them, enqueued, started, finished, retried and failed inside writes; the due effects, and
    the running ones, which after a restart are in doubt;
  - schema version 2, which adds the outbox and the projections' versions, and migrates version
    1 stores; a fault point for rebuilds.
- **Verified:**
  - Known-answer tests: 18 more in `keel-store` (33 in all), among them an order and a payment
    projected as they fold, a late event folded in its place, rebuilds on a version change and
    on request, an interrupted rebuild, a version 1 store migrated, and every state of an
    effect, and every refusal. The golden rows pin each projection's rows for a fixed history.
  - A property test against a model of projections: writes that append events and receive two
    other devices' logs a few events at a time, commit or fail, with reopenings and rebuilds.
    After every step, each order's and payment's row is the model's fold of the stream's stored
    events in canonical order, as are the lists by state and by order, every aggregate loaded,
    and the streams each write reports it touched; a rebuild changes nothing.
  - A convergence property: two stores receive the same events, each in its own order and its
    own writes, some failing, and end with byte-identical projections, and the model's.
  - A property test against a model of the outbox: every change, and why a change is refused
    (an unknown key, another effect under the key, the wrong state, not yet due, a cause the
    store doesn't hold), inside writes that commit or fail, with reopenings.
  - The crash tests' workload now also rings up an order and moves effects along; after every
    crash, the projections equal a rebuild from the stored events, and the outbox holds exactly
    the effects of the stored writes.
  - Coverage probes, over 1,000 cases: an event arriving after others with later HLCs in its
    stream in 72%, and the arrival order folding differently from the canonical one in 62%;
    every order and payment state in 54 to 891 cases; closed checks in 90, tips in 75, a
    payment on an opened check in 453, a business date differing from the last event's in 442;
    every outcome of every outbox change, an effect started before it was due in 85, and due
    effects out of the order they were enqueued in 153.
  - Planted bugs, all caught: 94 in `keel-store`, 43 of them new, aimed at projections and the
    outbox, and 7 re-aimed at code this slice moved. The property and crash tests alone catch
    83; the other 11, settings, versions and invalid input, only the unit tests reach.
  - Every property test in `keel-store` passed 100,000 cases.
  - CI green.
- **Decisions:** [ADR-0017](./adr/0017-projections-and-outbox.md), accepted 2026-09-29 after
  review, with the details the build settled under "As built":
  - projections hold one row per stream, recomputed from the whole stream in each write that
    touches it, measured at about 2.3 µs an event on the development machine;
  - enqueuing a key the outbox holds is a no-op for the same effect, and refused for another;
  - an effect's cause must be an event the store holds.
- **Found and fixed during the build:**
  - The first planted-bug run hung: a bug that kept projections from being built made every
    write fail, and the convergence test, which lets writes fail on purpose, retried forever.
    It now fails on any error it didn't plan.
  - The projection property didn't check loading aggregates; it now compares every aggregate the
    store loads with the model's fold.
  - Two outbox refusals came up too rarely: starting an effect before it's due, in 5.6% of cases,
    and due effects out of the order they were enqueued, in 2.9%. Checking the due effects at
    later times too, and more retries, raised them to 8.5% and 15%.
  - The crash tests take 20 s instead of 4 s in a debug build, since each write folds the streams
    it touches from the start, and the workload's streams grow to hundreds of events.
  - One planted bug couldn't change what the code does: letting an effect's kind start with a
    capital, which the check on every letter refuses anyway. It was re-aimed at a kind starting
    with a digit.

### The event log store: step 5, slice 1 (2026-09-29)

Commit `f9e68d7`. Reviewed 2026-09-29.

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
- **Decisions:** [ADR-0016](./adr/0016-device-store.md), accepted 2026-09-29 after review, with
  the details the build settled under "As built (slice 1)":
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
- For step 7's slice 4: the reference all-in-one to buy (ADR-0023 suggests a 2 GB device with
  Android 11 and a built-in printer, such as iMin's Swan 1). This environment reaches
  `dl.google.com` now (a personal environment, POS-system, allows it), and the Android SDK's
  licenses were accepted on 2026-11-09; it has no KVM, so emulators run in CI.

## Known gaps

Work deliberately left for later, so it isn't forgotten:

- **`keel-events`:**
  - Enrollment and revocation records signed by the Device CA, and their distribution to replicas.
  - A way to re-admit genuine events quarantined by a revocation.
  - Hardware signers, which the platform apps provide through the `Signer` trait.
  - Negotiating schema versions between kernels, in `keel-sync`.
- **`keel-types`:** `Locale` for more locales, with the languages the product adds; units'
  names beside quantities, with the first items sold by weight.
- **Printing** (ADR-0024):
  - One printer per device, set by the shell, until routes and fallbacks; kitchen tickets with
    the kitchen.
  - Receipts in Windows-1252 only, until raster text; times as `2026-10-06 08:15` until `Locale`
    knows dates; labels in English and Spanish.
  - `reprint` and `print_jobs` read the whole outbox, and `next_print_job` every due effect: fine
    for a register's day, not for months of them, which retention will prune.
  - Drawer openings as events, which audits want, with cash management.
  - An order with so many lines that its receipt would pass 40 KB of lines prints the first and
    says how many more.
- **`keel-runtime`:**
  - The open orders view prices every open order again: 0.7 ms for eleven on the development
    machine. A location with hundreds open will need the projection's rows, or tickets kept.
  - Cash taken only for the whole of what is due: partial payments, cards and other tenders,
    splitting checks, voids, comps, discounts and an order's attributes come with the screens
    that need them.
  - PINs for signing in (security §2.2), and checking what a member may do (`keel-policy`).
  - A profile replaced while running: a new one takes a restart (ADR-0023, decision 3).
- **`keel-store`:**
  - Power-loss tests, which need keel-sim's simulated disk; until then the durability settings
    are checked as SQLite reports them.
  - Retention, snapshots and backups, after the sync engine.
  - Salvaging a damaged store: reading what still reads, above all the device's own events not
    yet sent, and starting again from its peers. A store that can't open, because a page opening
    reads is damaged, can't be checked yet either.
  - The platform shells make, wrap, store and rotate the store's key (steps 7 and 8), as ADR-0018
    describes. Rotation follows the protocol the store's crash tests check.
  - SQLCipher's HMAC-SHA512 may be slow on ARM cores without SHA-512 instructions: measure on the
    reference register (step 7), and switch to HMAC-SHA256 before the first deployment if so.
  - `rusqlite` bundles SQLCipher 4.14.0, on SQLite 3.51.3, while SQLCipher is at 4.19: follow its
    releases. Its rekey reports success when it fails, which the store works around (ADR-0018):
    check each upgrade against the store's tests.
  - Check balances in a projection: they need the location's pricing rules in the store.
  - Folding only what's new: each write folds the streams it touches from the start, about
    2.3 µs an event on the development machine. Long-lived streams, such as a gift card's, will
    need folded state kept between writes.
  - Telling the UI what changed: a write reports the streams it touched, for the shell to show
    again; subscriptions come with the shell.
  - Which replica enqueues an effect, by its executor tier, comes with the first effects:
    payments, printing and fiscal submissions.
  - Releasing or discarding quarantined messages: they are kept for a person to look at, with
    no tools yet.
  - Code that opens SQLite connections of its own, in a process with stores, must call
    `init_sqlite` before its first, or SQLCipher's initialization can race again: the shells
    (steps 7 and 8). SQLite now offers a hook that initializes under its lock
    (`SQLITE_EXTRA_INIT_MUTEXED`); SQLCipher could use it, which is worth reporting.
  - A statement cache: reading an event's number or a page of the feed spends most of its time
    preparing statements, as every read of the store does. `rusqlite`'s `cache` feature would
    help them all.
  - `confirmed()` reads every run of every record. The UI, which shows what is provisional, will
    need each device's confirmed start kept as records arrive.
- **`keel-sync`:**
  - A hub alone never settles its log, since `keel-sync` settles only once a peer has answered: a
    location of one device writes as an island after 3 s, and confirms nothing (ADR-0026, as
    built).
- **`keel-store`'s lock:** in a process that spawns others, a child holds a copy of a store's
  lock from its fork until its exec, so a store closed and opened again in that moment is
  briefly `Busy`. CI's crash tests met it, their children spawned side by side, and retry an open
  that is busy; a shell that spawns processes would have to as well.
- **`keel-net` and `keel-provision`:**
  - Certificates from a development CA, valid for a year, renewed only by provisioning again,
    where the Device CA's will live 24 to 72 hours (step 8); no revocation.
  - `rustls-webpki` doesn't enforce a trust anchor's path length: the CA's key, kept off the
    devices, is what keeps every certificate the location's.
  - Frames that find a queue full are dropped, which the replicator's rounds recover, 5 s later.
    A device catching up after a long time away may lose some that way, and take longer.
  - One hub address, provisioned: devices don't follow a standby that takes over, until
    discovery.
  - Every pair of candidates for the hub must be within two hops of each other, or two of them
    take the role from each other in turn (ADR-0022): the location's links, which discovery will
    make, must keep them so. Nothing checks it as the replicas run.
  - Without a quorum, a split store runs a hub on each side. When it heals, the losing side's
    records stop counting and its events are numbered again: the shells must show that numbers
    shown as confirmed there have changed (step 7).
  - Batching several writes' events into one record, if the records prove too many: when events
    arrive one at a time, the store holds about as many records as events.
  - Devices send the watermark back to the hub with each round: redundant, and harmless.
  - Pruning below the watermark, with retention.
  - Showing what is provisional and what isn't yet backed up, in the shells (step 7).
  - The transport (offline-and-sync §3.2–3.3): WebSocket over TLS, discovery, scopes, device
    certificates, protocol negotiation, priority lanes, compression and reconnection backoff.
  - A device restored from an older copy that must sell before any peer answers forks its log.
    The replicas report the fork, and keep disagreeing about that log until a person resolves
    it, with tools that don't exist yet. A forked candidate never claims, and a forked hub stops
    serving.
  - A peer refusing a replica's own log reads as a fork, whatever the peer's reason: frames don't
    say why a peer took none of a batch. Once enrollment records travel between replicas
    (`keel-events`' gap above), a peer that doesn't know a hub's device yet would make it stop
    serving until the peer takes its log. A `have` could say what its sender quarantined, and
    why.
  - A batch waiting for acknowledgement from a peer that has since restarted holds up the next
    for up to the acknowledgement timeout (2 s).
  - A clock set back less than a round delays the rounds by as much. Timers on a monotonic clock
    would need a second kind of time passed in, beside the clock the store stamps events with.
- **Ownership** (ADR-0021, decision 8):
  - The hub taking an order back from a device it hasn't heard from in a while: until then an
    order whose owner died in the middle of a payment can be taken only by override. Heartbeats
    now tell a device that it is an island, and the simulator's devices override only then.
  - A hub whose clock is more than a minute behind the devices can't give their new orders
    away: its grants sort before the orders' creation, where they don't apply. The hub's beat,
    its clock's reading, could tell the devices.
  - Who may override: a manager, checked with `keel-policy`.
  - The owner handing an order to another device without that device asking.
  - A payment started by a device that didn't own its order is flagged by the device's check
    and checkout's issues, not in the payment's own fold, which can't see the order.
  - Other leases: tables, store-wide order numbers, limited quantities (offline-and-sync §6.3).
  - The simulator's hub seldom refuses a request from the device that owns the order, once in
    5,000 seeds: the known answers and the property tests reach that refusal.
- **`keel-sim`:**
  - Disk faults, torn writes and lost fsyncs, wait for a simulated disk: a SQLite VFS, which
    needs `unsafe` code.
  - The invariants of later features: payments, the ledger and fiscal chains
    (offline-and-sync §12).
  - Its locations have two candidates for the hub, which every device links to: more candidates,
    and devices linked to only one, only the protocol property reaches.
- **Orders:**
  - Permissions and approvals aren't checked yet (`keel-policy`).
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
