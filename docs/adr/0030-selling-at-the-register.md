# ADR-0030: Selling at the register: discounts and price overrides as order events, split checks and partial payments, and orders recalled across devices

- **Status:** Accepted
- **Date:** 2026-11-11

## Context

The MVP's second step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is selling at the
register: discounts with reason codes and limits (B-06), open-price items (B-03), notes (B-14),
split tender (B-05), and park and recall across devices (B-04).

What is built shapes it:

- `keel-pricing` prices discounts already, on a line or the whole order, a percentage or an
  amount, each from what is left after those before it (ADR-0014). Orders record none: their
  baskets have none.
- An order splits into checks, each line allocated to them in whole shares, and each check
  prices as its own sale and closes when its payments cover it (ADR-0015). The runtime pays only
  in cash, the whole of a single check at once.
- A line records its notes (`order.line_added`, `order.line_changed`); the runtime takes none.
- Every order has an owning device, and only it changes the order. A device asks the hub for
  one it doesn't own, and the hub grants or refuses (ADR-0021); `keel-domain` and the store
  answer as the hub, but the runtime never asks.
- Staff actions are gated by roles and approvals (ADR-0029): `discount.apply` has amount and
  percentage limits, `price.override` an amount limit.

## Decision

1. **Discounts are order events.** `order.discount_applied` v1, `{1: discount, 2: line, 3:
   percent, 4: amount, 5: reason code, 6: note}`: its identifier; the line it is on, absent for
   the whole order; exactly one of a percentage, in basis points from 1 to 10,000, or an amount,
   more than zero; and the reason, the merchant's code with an optional note.
   `order.discount_removed` v1, `{1: discount}`. Each prices as `keel-pricing`'s discount, on its
   line or on the order, in the order they were applied. A discount on the whole order is a
   percentage: an amount on the whole of a split order has no one right share for each check. An
   amount off a line shared among checks is split among them as the line is, so each check takes
   its part. A discount's identifier names it for good: once applied, even if since taken off, it
   is never applied again. A discount goes on a live line (pending or fired: a meal is often
   discounted after it is served) that has no part on a closed check, or on the whole order while
   no check is closed; it comes off on the same terms, but always off a line since removed or
   voided. The fold applies what it can and reports the rest: a discount on a removed line or in
   another currency doesn't apply; one on a frozen line, or on the whole order once it is closed
   or a check is, applies and is reported, since the closed checks' snapshots stand; taking off a
   discount never applied is reported, and one already taken off, as two devices may, is not.

2. **A discount is gated as `discount.apply`**, asked for as what all the order's discounts
   would take together before tax, and the share its scope's discounts would take together, of
   the line, or of what the lines' own discounts leave of the order: percentages compound (two of
   10% are 19%), and an amount counts as its share of what is left when it applies, rounded up.
   So a role's "10% and $5" holds for the order's discounts however many there are. An amount
   more than is left of its line is refused. Raising a discounted line's quantity is asked for as
   a discount, for what all the discounts would then take. Taking one off is not gated. (As
   built: first designed per discount, found in review to let discounts be stacked.)

3. **A price override is an order event.** `order.line_repriced` v1, `{1: line, 2: unit price,
   3: reason code, 4: note}`: the line is priced at the new unit price, with its modifiers, from
   then on, in the order's currency, at least zero; the price it was rung at stays with it. The
   last price that applies is the line's. A live line, pending or fired, takes one unless it has
   a part on a closed check; a price that changes nothing is refused. The fold reports one for a
   removed line or in another currency, which don't apply, and one on a closed check, which
   does. It is gated as `price.override`, the amount being how far the new price is from the
   line's, for its whole quantity, after the order's own rules. An item rung at a price the
   catalog gives as zero is how an open-price item rings until the catalog marks one so.

4. **Notes**: the runtime sets a line's notes (`order.line_changed`), which the kitchen and the
   receipt show. An order's own note, and tags, wait for a version of `order.attributes_changed`.

5. **Split checks and partial payments**:
   - An order splits evenly into a number of checks, each line in as many shares, or a line moves
     to another check, new or open, whole.
   - A payment pays part of a check: cash takes the amount to apply and the amount tendered, which
     is at least it, and gives back the change. A check closes when its payments cover it; the
     order closes when every check with a live line has closed.
   - The ticket shows each check, what it owes, and its payments. Card payments come with their
     connector (step 7), through the same path.

6. **Park and recall**: an open order stays where it is, owned by its device, and every device
   lists the location's open orders. Recalling one another device owns asks the hub for it, and
   the device takes it once the hub grants it; a device that can't reach the hub takes it only on
   a manager's word, `order.ownership_overridden` (ADR-0021), gated as `order.void`'s approvers
   may. An order's label for the list is its table or its first lines, until orders can be named.

7. **The shells**: the bindings take each intent; the Android register gets a discount sheet
   (reasons, a percentage or amount, on a line or the order), a price override, line notes, split
   and pay-part on the tender, and an open-orders list across devices. A reason is a code the
   merchant chooses: the register offers a few to begin with, and the reports group by them.
   Reasons kept in the profile, which the back office edits, wait for the profile's settings.

8. **Built in slices**, each verified as it lands, then reviewed:
   1. *Discounts*: the schemas, the fold and the baskets, the closing snapshot, the gate.
   2. *Price overrides and notes*.
   3. *Split checks and partial payments*.
   4. *Park and recall across devices*, through the hub.

9. **Out of scope**: automatic discounts and promotions; amount discounts on a whole order;
   custom items not in the catalog; an order's note and tags; card payments (step 7); refunds
   (step 4).

## As built

Built in four slices on 2026-11-11 and 2026-11-12, each verified as it landed, then reviewed:
the kernel and the Android shell each by a reviewer who hadn't built them. What the review
found, and what was done:

- **Discounts stacked past a role's limits**, each within them alone: gated together now
  (decision 2). An amount larger than its line took more as the line grew: refused. A discount
  on a raised line took more with no approval: a raised line is asked for again.
- **A price set in steps** was asked for one step at a time: asked for by how far it is from the
  rung price, each extended as the order charges it; a raised line priced by hand is asked for
  again.
- **A change to a partly paid check** (a discount, a price, a line taken off or lowered) left the
  customer owed change no one gave them: refused while a payment toward the check is taken.
- **A split order emptied of its last open check** stayed open with nothing able to close it: it
  closes once every check holding something is. An order whose lines all moved off its main
  check listed no check and couldn't be paid: its check is listed, and `pay_cash` pays the one
  check holding something, refusing an order split among several. A split order's reprint gave
  its last check's receipt alone: every check's prints again.
- **On the register**: a double tap put on two discounts; a take-over's refusal spoke of dropping
  an order; Take over was offered with the hub up; orders parked before the day turned weren't
  listed; a check's Pay used the check as last drawn; a line's buttons were squeezed off a phone.
  Each is fixed.

Gaps that remain:

- A line both discounted and priced by hand, raised in quantity, is asked for as a price
  override only.
- An approval for a price names the order, not the line: one for $5.00 on one line could be spent
  on another line's $5.00 of the same order.
- What a discount on the whole order is asked for prices the order whole; a split order's checks
  are charged each on its own, which can differ by a cent.
- A partly paid check's lines are frozen until refunds come (step 4), even for a change that
  wouldn't take it below what was paid.
- In the fold, a discount refused for a removed line, another currency or an unknown line doesn't
  use its identifier, so a later event naming it could apply: only replicas' conflicts make one.
- A take-over records `hub_unreachable` always, as the register offers it only then.

## Consequences

**Positive**
- Every discount and override is in the log with its reason and, beyond a role, its approval, as
  loss prevention needs (security §6).
- The kernel prices discounts as it always has; only the basket gains them.
- Splitting and part payment are what card payments need: step 7 adds a tender, not a flow.

**Negative**
- Four new order schemas: every replica's kernel must know them before any device writes them.
- An open-price item is a zero price and an override until the catalog has a flag for it.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| A discount as a field of `order.line_added` | A discount comes after a line is rung, and can be taken off; one event each is its own audit. |
| An override as a new line at the new price | The line's identity, notes and allocation would be lost. |
| Recall by copying an order to the device that recalls it | Two devices would sell the same order: ADR-0021's ownership is what prevents it. |

## References
- [ADR-0014](./0014-pricing-engine-v0.md), [ADR-0015](./0015-checks-and-payments.md),
  [ADR-0021](./0021-ownership-leases.md), [ADR-0029](./0029-staff-roles-and-approvals.md);
  [feature catalog B-03…B-06, B-14](../product/feature-catalog.md)
