# ADR-0031: Cash: drawer sessions as single-writer aggregates, a blind close, and X and Z reports from local data

- **Status:** Accepted
- **Date:** 2026-11-12

## Context

The MVP's third step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is cash: drawer sessions
with a float, paid-in and paid-out with reasons, safe drops, an audited no-sale (N-01); a blind
close whose over or short past a role's threshold needs a manager (N-02); and X and Z reports,
from local data, offline (O-02).

What is built shapes it:

- The runtime takes cash: a payment is initiated and captured with what was tendered and the
  location's cash rounding, the drawer's kick and the receipt print, and the order or check
  closes, all in one write (ADR-0023, ADR-0030). No drawer records it.
- The domain model sets out drawer sessions (§13): one device, an opening float, cash in and
  out, paid-in and paid-out with reasons, drops, no-sale, a blind close count, over and short.
  They are **single-writer**: owned by the device the drawer is attached to, so they never
  conflict (offline and sync §6.2).
- Roles already name `drawer.no_sale`, `drawer.variance` (an amount limit) and
  `drawer.paid_out` (an amount limit) (ADR-0029); nothing asks for them yet.
- The cloud reports sales by business day from its durable copy (ADR-0028); a device holds the
  location's orders too, replicated, and reports nothing.
- The store's projections are versioned tables, each following a stream kind, rebuilt from the
  events whenever their version changes (ADR-0017): a new one is no new store schema.

## Decision

1. **A drawer session is an aggregate**, its stream kind `drawer`, its identifier minted when it
   opens. It is single-writer: the device that opened it alone appends to it, and only one
   session of a device is open at a time. Its events, each v1:

   | Schema | Keys |
   |---|---|
   | `drawer.opened` | 1 float: the cash put in to begin, zero or more |
   | `drawer.cash_taken` | 1 payment, 2 amount: what a cash payment left in the drawer, its amount, tip and rounding, more than zero |
   | `drawer.cash_moved` | 1 kind (0 paid in, 1 paid out, 2 safe drop), 2 amount, more than zero, 3 reason code, 4 note (optional) |
   | `drawer.no_sale` | 1 reason code, 2 note (optional) |
   | `drawer.closed` | 1 counted: the blind count, zero or more; 2 expected: what the device expected |

   What the drawer is expected to hold is its float, the cash taken and paid in, less what was
   paid out and dropped; the variance at close is the count less what was expected. Every
   amount is in the location's currency.

2. **The fold applies what it can and reports the rest**, as an order's does: an event from
   another device than the opener's applies and is reported (the opener alone writes it, so only
   a forged or misrouted event is one); an event after the close applies and is reported, so no
   cash goes uncounted; a payment taken twice, or an amount in another currency, doesn't apply
   and is reported. Commands refuse each of these.

3. **Cash goes into the drawer as it is taken.** With a drawer open on the device, a cash
   payment's write also appends `drawer.cash_taken` to it: the payment and the drawer agree, or
   neither is written. With none open, the sale goes on (a sale never waits for a drawer) and
   the cash is reported as taken with no drawer open, for a manager to look at. Requiring an
   open drawer to take cash waits for the profile's settings.

4. **Gated as the roles say** (ADR-0029): a paid-out as `drawer.paid_out`, for its amount; a
   no-sale as `drawer.no_sale`, which kicks the drawer open; a close with a variance as
   `drawer.variance`, for its size: zero needs nothing. A paid-in and a safe drop are not gated.
   The close is blind: the count is entered before the device says what it expected, and
   refusing it for want of a manager says no more than that.

5. **The store projects drawers**: a `drawers` table, one row per session (device, its state,
   business date, float, cash taken, moved in and out, no-sales, expected, counted), so the
   runtime finds its device's open session and reports find a day's sessions without reading
   every event; and a `drawer_payments` table, one row per payment a session took, so a report
   finds the cash no drawer took. It is a projection, rebuilt from the events: no new store
   schema.

6. **Reports from local data**, offline, for a business day, of every order the device holds:
   orders closed and their total, of those rung on the day; gross, discounts, comps and net
   before tax, and tax; each tender, of every payment taken on the day, whatever its order's
   state; orders voided; each drawer session opened on the day, with its expected, counted and
   variance once closed; and cash no drawer took.
   - **The drawer report** prints at the close.
   - **An X report** is the day so far, at any time, changing nothing.
   - **A Z report** is the day's end: printed once every drawer session on the device is
     closed, whatever day it opened, marked as the Z. A numbered, recorded close of the day, as fiscal rules want it,
     waits for the fiscal work.

7. **The shells**: the bindings take each intent and report; the Android register asks to open a
   drawer, with its float, once someone signs in and none is open; a drawer menu takes paid-in,
   paid-out and drops with reasons, a no-sale, the blind close on a keypad, and the X and Z
   reports, shown and printed.

8. **Built in slices**, each verified as it lands, then reviewed:
   1. *The drawer session*: the schemas, the fold, the commands, the store's projection.
   2. *Drawers at the register*: the runtime's intents, cash taken into the drawer, the gates,
      the drawer report, the bindings.
   3. *X and Z reports*: the runtime's reports and their printing, the bindings.
   4. *The shell*: the Android register's drawer.

9. **Out of scope**: counting by denomination; more than one drawer on a register, or a drawer
   shared by registers; mid-shift counts; safes and bank deposits; paying tips out; cash in
   another currency; a recorded, numbered close of the day.

## Consequences

**Positive**
- Every movement of cash is an event with its reason and, beyond a role, its approval: the
  audit loss prevention wants (security §6), on the device, offline.
- A drawer never conflicts: one device writes it.
- The reports need no network: a register closes its day in island mode.

**Negative**
- Five new schemas, which every replica's kernel must know before any device writes them.
- Cash taken with no drawer open is only reported, until a profile can require a drawer.
- A device's report covers the orders it holds: one that hasn't heard from another register
  since a sale leaves that sale out until it does.

## As built

Built in four slices on 2026-11-12, each verified as it landed, then reviewed: the kernel and the
Android shell each by a reviewer who hadn't built them. What the review found, and what was done:

- **The blind close wasn't blind**: the open drawer showed its float and every sum, from which
  what it should hold is arithmetic. An open session's view, on the register and on a report,
  now has its float and no-sales alone; what moved through it shows once it is counted.
- **A drawer left open past the day's end** held the next day's cash, yet that day's Z passed,
  calling the cash drawerless: a Z waits for every open session of the device, and which cash a
  drawer took is projected per payment (`drawer_payments`, the projection's version 2).
- **Payments went by their order's day, of closed orders only**: an order rung one day and paid
  the next, or cash toward an order still open, was misreported or left out. Tenders are of
  every payment taken on the day; sales are still by the day an order was rung. The cash
  tender left out cash rounding, which the drawer counts: it is what the customer paid.
- **A drawer's refusal refused the sale**, as one opened before the location's currency changed
  would every cash sale, and its own close: such a session is left open for a person to look
  at, the register opens another, and a refusal of the cash never refuses the sale.
- **An order in another currency failed the report**: it is left out before it is priced.
- **Amounts that overflow** were recorded, reported by the fold as another currency, and left a
  session unable to close: the commands refuse them, and the fold reports an overflow.
- **On the register**: a drop of nothing reached the kernel's refusal, with the keypad gone; a
  drop nobody touched was recorded as change; a no-sale had no reason to choose; the count was
  read at the tap, before the digits tapped before it were in, so a fast close could close
  short; a report read earlier stayed on the page; the Z was offered with the drawer open, and
  refused in the fake till's words, not the kernel's; comps and each drawer were missing from
  the report on screen; a float cancelled stopped the register asking for one. The fake till
  now gates a no-sale and a close as the kernel does. Each is fixed, with a test.
- **CI's emulator tests had failed since step 2**: on a phone, the ticket pane didn't scroll, and
  a line's buttons, Split and Drop order were below the screen, where a tap hit nothing. The
  pane scrolls, and the tests scroll to what they tap.

Verified here as CI would: formatting, both lint passes, the docs, every workspace test with the
exhaustive sweeps, the wasm build, the planted bugs (every one aimed at the drawer and the
reports caught), the Kotlin bindings' tests, the register's JVM tests and the app's build. The
app's instrumented tests are built but not yet run on an emulator: CI's minutes for the month
are spent. They run when CI does.

Gaps that remain:

- **An X report shows the day's cash tender** while a drawer is open: with one drawer and every
  sale in it, what it should hold is a sum away. Reports aren't gated by role yet.
- **The approval prompt hints at the count**: a close asks for a manager only when it is over or
  short beyond the role, and the approvers listed depend on its size. A cashier can cancel and
  count again until no prompt shows; nothing records the attempt.
- **Safe drops aren't gated**: a shortage can be recorded as a drop, which a safe count would
  find, and safes are out of scope.
- **Sales go by the day an order was rung**: an order rung before the day turned and closed after
  changes a day already reported.
- **Another register's cash** shows as taken with no drawer until its drawer's events arrive.
- **The store check** compares the `drawers` table with a rebuild, not `drawer_payments`.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| The drawer's cash derived from payments at close, with no `drawer.cash_taken` | A payment doesn't name a drawer: which session took it would be guessed from timing, and a new payment schema version would be needed to say. |
| Refusing cash with no drawer open | A register that can't sell for want of a drawer session fails the merchant at the worst time; reported, it is fixed after. |
| The Z report as an event, numbered | A recorded day close is fiscal work, with its own rules per jurisdiction: the report is what the MVP needs. |

## References
- [ADR-0017](./0017-projections-and-outbox.md), [ADR-0023](./0023-register-shell.md),
  [ADR-0028](./0028-cloud-cell-v0.md), [ADR-0029](./0029-staff-roles-and-approvals.md),
  [ADR-0030](./0030-selling-at-the-register.md); domain model §13; offline and sync §6.2;
  [feature catalog N-01, N-02, O-02](../product/feature-catalog.md)
