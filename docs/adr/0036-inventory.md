# ADR-0036: Inventory: a movement ledger per item, sales read from orders, counts as of a time

- **Status:** Proposed
- **Date:** 2026-11-16

## Context

The MVP's eighth step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is the inventory's
movement ledger (K-01): stock as the sum of movements with reason codes, and the levels derived
from them: on hand, reserved, available. Returns left putting stock back for it (ADR-0032).

What is built shapes it:

- Inventory is **movement-sourced** (domain model §10): on hand is never a stored number, but
  the sum of immutable movements, so decrements from many devices offline merge with no
  coordination: consistency class A (offline and sync §6.1).
- **Sales are already events**: a closed order records each line's variant and quantity, a
  return each line and quantity returned (ADR-0015, ADR-0032). Writing a movement per line sold
  as well would say the same thing twice, and the two could disagree.
- An item's state kept per item, written by any device, its latest event deciding, is how
  availability works (ADR-0033): a stream per item, its identifier the item's.
- A line's quantity has a unit: each, or a measure for an item sold by weight.
- Roles gate what staff may do, and a format-4 profile already holds what card payments added
  (ADR-0029, ADR-0035).

## Decision

1. **Stock is an aggregate per variant**, stream kind `stock`, its identifier the variant's, so
   every device writes the same stream, with no owner. Its events, each v1:

   | Schema | Keys |
   |---|---|
   | `stock.moved` | 1 movement (0 received, 1 adjusted, 2 wasted, 3 reserved, 4 released), 2 quantity, 3 reason code (optional), 4 note (optional) |
   | `stock.counted` | 1 quantity, 2 as of (a time), 3 note (optional) |

   A movement's quantity is more than zero, but an adjustment's, which is any but zero; a count's
   is zero or more. The stream's unit is its first event's: one in another unit doesn't apply,
   and is reported. The fold is a sum, so events in any order give the same levels.

2. **Sales and returns are read from orders**, never written twice, from what the order's fold
   applied: each close takes what its live lines come to beyond what earlier closes took, as of
   that close, and gives back what they no longer come to; an order reopened keeps what it took
   until it closes again; a void or an abandonment gives back everything its closes took, as of
   it; a return puts back what it returned, as of the return. An order voided or abandoned
   before it closed takes nothing. A sale in another unit than its item's stock takes nothing,
   and is reported. *(As built: at first, an order took everything at its last close; see
   below.)*

3. **The levels** of a variant: *on hand* is its latest count, by as-of time, plus every movement,
   sale and return after that time; with no count, the sum since its first stock event, so an
   item's stock starts with its first movement or count of any kind, and earlier sales don't
   count. Received and returned add; sold and wasted take; adjusted adds or takes. *Reserved* is
   what was reserved less what was released; *available* is on hand less reserved. A count is
   taken as of a time, at most when it is recorded, so a store counts while it sells: what sold
   after that time still comes off (K-03's beginning). On hand below zero is allowed, and shown
   so: a sale is never refused for stock (K-02 waits for v1).

4. **What the register records**: received, adjusted, wasted, counted, reserved and released,
   each for a reason code, with a note. Adjusting, wasting and counting are `stock.adjust`, a new
   action, which the member's role permits or a manager approves; receiving, reserving and
   releasing need no more than being signed in. A role that permits it needs profile format 5,
   and its approval is `approval.granted` version 3, with version 1's keys: versions 1 and 2
   never decoded it, and still don't. A return puts its lines back in stock unless the cashier
   says they don't go back, which records them wasted, for the return's reason, in its write.

5. **The store projects** what each stock stream moved, one row per applied movement and count,
   and what each order sold and returned, one row per variant each close, void or return moved;
   a level is a query over both. A variant is tracked once it has a stock event. Projections: no
   new store schema.

6. **What the register shows**: the tracked items with their levels, the ones below zero first;
   an item's movements, sales and returns since its latest count; and the day's report adds what
   was wasted, and each count's variance from what was expected at its time.

7. **Built in slices**, each verified as it lands, then reviewed:
   1. *The domain and the store*: the stock aggregate and its two schemas, `stock.adjust`,
      profile format 5 and `approval.granted` v3, the projections and the levels' query.
   2. *The register*: the intents, returns that don't go back to stock, the views and the
      report, the bindings.
   3. *The shell*: the Android register's stock screen and the return's choice.

8. **Out of scope**: recipes and modifier depletion (K-07); transfers and in transit (K-05);
   purchase orders and on order (K-04); receiving with costs, and cost methods (K-06, K-10);
   negative stock policy (K-02); count sessions and sheets (K-03); places within a store, lots and
   serials; limited-quantity escrow; editing stock from the back office (step 9).

## As built

Built in three slices on 2026-11-15 and 16, each verified as it landed, then reviewed: the
kernel and the Android shell each by a reviewer who hadn't built them. A property test replays
any movements, counts and flows, in any unit and up to overflow, against an exact model.
What the review found, and what was done:

- **An order reopened and closed again took everything a second time** after a count between
  its closes, as the whole order was timed at its last close: each close now takes what it adds
  since the last (decision 2, amended). The flows were read from the order's events, not from
  what its fold applied, so a second return with one refund put stock back twice: they are read
  from the fold. An order reopened after a return lost the return until it closed again: a
  return counts as of the return, whatever comes after.
- **Nothing reported what didn't count**, as decisions 1 and 2 say: each stock stream's
  conflicts and the sales in another unit are kept, and the register shows them on the item.
- **One quantity scanned from a barcode could overflow an item's levels for good**, and with
  them every item's list, the day's report, and returns of that item: a movement or count of
  more than a billion is refused, and an item that overflows is left out of the list and the
  report instead of failing them.
- **A count's default time came from the device's clock**, sales' from the log's: a device
  behind the log took a sale twice. It is the log's. Two counts as of one time were ordered
  differently by the report than by the levels: they share one order.
- **On the register**: a refresh that failed after a movement was recorded left it to record
  again; a second tap while an approval was asked recorded it twice; an item already tracked,
  chosen again, showed no history; the screen didn't scroll, its keypad squeezed Record on a
  narrow pane, the error showed twice, and nothing showed when; a return's choice not to restock
  stayed for the next return. Each is fixed, most with a model test. A note given without a
  reason is refused.
- **Tests the review asked for**: the stock projections' rows are pinned; the bindings take
  waste, adjustments down and a return kept out of stock, and the day's report shows them.

Gaps that remain:

- **Every item starts in `each`** on the register: the catalog has no unit for a variant, and
  the register rings whole counts. An item first tracked by measure elsewhere is typed in
  thousandths of its unit.
- **A count is as of now** on the register, and a movement's note isn't offered there: the
  kernel and bindings take both.
- **A return kept out of stock wastes without `stock.adjust`**: the return is gated as a refund
  is, and its waste is recorded with its reason and who made it. A return of nothing to refund
  isn't gated at all, as before.
- **A level is computed at each read**, from all of an item's rows since its first event or
  latest count: fine for a café's items, to watch for a large catalog.
- **The Android register's instrumented test** waits for an emulator, as steps 3 to 7's do.

## Consequences

**Positive**
- Every device decrements stock as it sells, offline, and the levels agree once the events do:
  no coordination, no conflicts but units.
- A sale's stock is the sale: nothing more to write, nothing that can disagree with it.
- A count fixes a level without stopping the store.

**Negative**
- A level is a query over two tables, not a row: fine for a café's items, to watch for a large
  catalog.
- Two new schemas, a new schema version, a profile format and an action, which every replica's
  kernel must know before any device writes them.
- Two counts with the same as-of time, from two devices, are ordered by their events, not by
  what was counted.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| A movement event per line sold, in the sale's write | The order already says it; two records can disagree, and every sale writes twice. |
| On hand kept as a number, updated by each sale | Devices offline would overwrite each other's decrements; the domain model rules it out. |
| A count replaces on hand from when it is recorded | A store would stop selling to count; a count as of a time keeps what sold after. |
| Stock per catalog item rather than per variant | A large and a small latte are different cups. |

## References
- [ADR-0015](./0015-checks-and-payments.md), [ADR-0029](./0029-staff-roles-and-approvals.md),
  [ADR-0032](./0032-returns-and-refunds.md), [ADR-0033](./0033-availability.md),
  [ADR-0035](./0035-card-payments.md); domain model §10; offline and sync §6.1;
  [feature catalog K-01](../product/feature-catalog.md)
