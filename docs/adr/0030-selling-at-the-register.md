# ADR-0030: Selling at the register: discounts and price overrides as order events, split checks and partial payments, and orders recalled across devices

- **Status:** Proposed
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

2. **A discount is gated as `discount.apply`**, with its percentage and the amount it takes off,
   before tax, as the kernel prices it: "10% and $5" means a 10% discount that takes off $5 at
   most. Taking one off is not gated.

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
