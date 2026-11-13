# ADR-0032: Returns and refunds: lines returned on the original order, refunds as their own aggregate, never beyond what was paid

- **Status:** Accepted
- **Date:** 2026-11-13

## Context

The MVP's fourth step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is returns and refunds,
linked to the original lines (B-09 basic, I-08): a customer brings back what they bought, and
is refunded what they paid for it, never more.

The domain model sketches a return as a new order with negative lines that reference the
original lines (§6.3), refunded at what each was allocated: its net price after discounts, and
its tax. Every return records the original sale, so a line is never refunded beyond what was
paid for it, across any number of partial returns (§6.4). The kernel has what that needs: each
closed check's snapshot records what each line was charged, its net and tax, and is what
returns read, never a recomputation (ADR-0015). The gate has `refund.issue`, limited by an
amount (ADR-0029). The drawer counts cash in and out (ADR-0031).

Two things shape the design. A return made on two registers at once, each offline, could each
refund the same line in full; the order's ownership lease (ADR-0021) already lets one device at
a time change an order. And a return's amounts come from the snapshot, never from pricing:
negative lines in a new order would take pricing, checkout, receipts and every view of an order
through quantities and prices below zero, for amounts that are already known.

## Decision

1. **Lines are returned on the original order**, by one new order event, v1:

   | Schema | Keys |
   |---|---|
   | `order.lines_returned` | 1 refund: the refund that pays it back; 2 lines: each `{1 line, 2 quantity, 3 net, 4 tax}`, in ascending order of line, never empty; 3 reason code, 4 note (optional) |

   A line's quantity returned is more than zero, in the line's unit; its net and tax are what is
   refunded for it, zero or more. It needs the order's ownership, as every change to an order
   does: a register returns an order another owns by recalling it through the hub (ADR-0030),
   or, with the hub out of reach, by taking it over on a manager's word. Two devices can't
   return the same line at once.

2. **What a line returned refunds** is its share of what its order was charged for it: the net
   and tax of every closed check's part of it, from their snapshots, for the quantity returned
   out of the line's quantity. Each share is taken of the quantity returned so far, less what was
   refunded for it so far, so partial returns add up: `refund = share(returned after) −
   refunded`, where `share(q) = amount × q / quantity`, rounded half up, and never below nothing
   nor beyond what is left. The last return of a line refunds exactly what is left of its net
   and tax, whatever was returned before. A comped line refunds nothing; a discounted line
   refunds what it was sold for.

3. **The fold applies a return and reports what doesn't fit**: a return of a line the order
   doesn't have doesn't apply; one on an order that isn't closed, or that takes a line beyond
   its quantity, or its net or tax beyond what was charged, applies (the money has moved) and is
   reported. Commands refuse each, and refuse reopening an order with a return.

4. **A refund is its own aggregate**, stream kind `refund`, its identifier the one the return
   names. It is single-writer, the device that made the return. Its events, each v1:

   | Schema | Keys |
   |---|---|
   | `refund.initiated` | 1 order, 2 tender (0 cash, 1 card), 3 amount: more than zero |
   | `refund.completed` | 1 amount: what was refunded; 2 rounding (optional): what cash rounding added to what was handed back |

   A cash refund is initiated and completed in one write, with the return. A card refund, to
   the card that paid, is initiated and completed by its connector, and arrives with card
   payments (step 7).

5. **Refunded to the original tender.** An order paid in cash alone is refunded in cash; one
   with a card payment is refused until card refunds arrive. Cash goes out of the device's open
   drawer, by one new drawer event, v1: `drawer.cash_refunded` (1 refund, 2 amount: what was
   handed back, more than zero), which what the drawer should hold leaves out. With no drawer
   open the refund goes on, reported as cash with no drawer, as a sale's is.

6. **Gated as the roles say**: a return as `refund.issue`, for what the order's refunds refund
   with it (ADR-0029), so a return split in parts is asked for as all of them. Its reason is the
   merchant's reason code. Cash handed back is rounded over all the order's refunds, never more
   than the cash the order took. A return that refunds nothing has no refund events.

7. **The store projects refunds**: a `refunds` table, one row per refund (its order, tender,
   amount, state, business date, and what was refunded), found by order and by business date.
   The `drawers` projection adds what each session refunded (its version 3).

8. **What the register does**: finds a closed order from its receipt, by the short code it
   prints; shows each line with what is left to return; returns lines, whole or in part, for a
   reason; refunds the cash, kicks the drawer, and prints a refund receipt listing the lines
   returned and what each refunded. Reports list the day's refunds by tender, by the day each
   was made, and what was returned; a day's sales stay as they were rung.

9. **Built in slices**, each verified as it lands, then reviewed:
   1. *The domain*: the three schemas, the fold and commands of returns, refunds and the drawer.
   2. *The register*: the projections, the runtime's lookup, return and refund, the receipt, the
      reports, the bindings.
   3. *The shell*: the Android register's returns.

10. **Out of scope**: exchanges; returns without a receipt, and return policies (windows,
    restocking fees, final sale); store credit; card refunds (step 7); returns at another
    location; putting stock back (step 8).

## Consequences

**Positive**
- A line is never refunded beyond what was paid: its returns are on its order, under the order's
  lease, and add up exactly.
- Pricing, checkout and the order's lines keep their rules: quantities and prices are never
  below zero.
- Every refund has its approval beyond a role, its reason, and its cash in the drawer.

**Negative**
- Four new schemas, which every replica's kernel must know before any device writes them.
- A return on another register needs the hub, or a manager's take-over, to have the order.
- An order with a card payment can't be refunded until card refunds arrive.

## As built

Built in three slices on 2026-11-13, each verified as it landed, then reviewed: the kernel and the
Android shell each by a reviewer who hadn't built them. What the review found, and what was done:

- **Refunds in parts handed back more cash than was taken**: each rounded to five cents on its
  own, three espressos paid as $10.60 went back as $10.65. Cash handed back is rounded over the
  order's refunds, and capped at the cash its payments took.
- **A share was taken of what was returned, not of what was refunded**: two devices' returns,
  worked out against one charge each, left the last a cent short. A return refunds its share
  less what was refunded (decision 2).
- **A role's limit could be passed in parts**: each return was asked for on its own. It is asked
  for as all the order's refunds (decision 6).
- **A payment not yet here passed for cash**: an order counts as paid in cash only if every
  payment its closed checks name is here, captured, in cash.
- **The order was decided outside its write**: it is decided again inside, and an order changed
  meanwhile is refused.
- **A refund that handed back nothing** counted as cash with no drawer: it isn't counted.
- **The bindings** took a count of a line sold by measure as that many kilograms, and called a
  line with nothing left a quantity of nothing: the first is refused, the second is "nothing
  left".
- **On the register**: a second tap on Refund refunded again; a code shorter than six characters
  opened whatever order it ended, and the kernel took one too; two orders with one code opened
  the latest; every return was "damaged" unless changed; a refused refund kept picks of lines
  since returned elsewhere; a failed refresh after a refund left its picks for a second tap;
  Return was offered on an order another register owns; errors showed twice. Each is fixed, with
  a model test; the fake till now finds, refuses and measures as the kernel does.

Gaps that remain:

- **Card refunds** wait for card payments (step 7); the refusal of a card-paid order can't be
  tested until the runtime takes cards.
- **A return on another register** needs the order: the register doesn't recall it through the
  hub for a return yet, and hides Return on such an order.
- **Reports** list refunds by tender, not the lines returned; a refund in another currency is
  left out of them.
- **The decision made again in the write**, a refund that hands back nothing, and a payment
  unknown here are guarded but untested: no test makes them happen.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| A return as a new order with negative lines (the domain model's sketch) | Pricing, checkout, receipts and every view of an order would meet quantities and prices below zero, for amounts the snapshot already holds; and two returns of one line, on two devices, would be two orders, which no lease keeps apart. An exchange, when it comes, can still be a return and a sale in one visit. |
| Refund events on the original payment | A payment is written under its order's ownership by the device that took it; a refund is its own movement of money, with its own tender and outcome, possibly on another device and against several payments. |
| A return's amounts priced again | Rules and prices change: a refund is of what was paid, which only the snapshot knows. |

## References
- [ADR-0015](./0015-checks-and-payments.md), [ADR-0021](./0021-ownership-leases.md),
  [ADR-0029](./0029-staff-roles-and-approvals.md), [ADR-0030](./0030-selling-at-the-register.md),
  [ADR-0031](./0031-cash-drawers-and-day-reports.md); domain model §6.3, §6.4, §7;
  payments §6; [feature catalog B-09, I-08](../product/feature-catalog.md)
