# ADR-0031: Cash: drawer sessions as single-writer aggregates, a blind close, and X and Z reports from local data

- **Status:** Proposed
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
   every event. It is a projection, rebuilt from the events: no new store schema.

6. **Reports from local data**, offline, for a business day, of every order the device holds:
   orders closed and their total; gross, discounts and net before tax, and tax; each tender;
   orders voided; each drawer session of the day, with its expected, counted and variance; and
   cash taken with no drawer open.
   - **The drawer report** prints at the close.
   - **An X report** is the day so far, at any time, changing nothing.
   - **A Z report** is the day's end: printed once every drawer session of the day on the device
     is closed, marked as the Z. A numbered, recorded close of the day, as fiscal rules want it,
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
