# ADR-0014: Pricing engine v0: snapshot prices, five rounding points, allocated discounts, US sales tax

- **Status:** Proposed
- **Date:** 2026-09-28

## Context

The pricing engine turns an order into money: line amounts, discounts, taxes and totals
([domain model §8](../architecture/domain-model.md#8-pricing-and-tax-calculation)). It must give
the same result, to the minor unit, on every device, hub and cloud node, because a total that
differs between the register and the back office is a classic point-of-sale defect. Every amount
must be explainable to a cashier, a customer and an auditor. Returns need each line's share of
every discount and tax, recorded when the sale is made.

Version 0 serves the first pilot: counter service at one US location. Three facts shape it:

- **Lines are snapshots.** An order line records the item's unit price, tax category and modifier
  prices as the catalog priced them when the line was rung up, with the catalog version
  ([ADR-0013](./0013-event-payloads-and-schema-evolution.md), domain model §5.5). Resolving a
  price from price lists, or a free topping from a modifier group's rules, happens then, and
  needs the catalog, which doesn't exist yet.
- **Rounding is a legal rule.** Jurisdictions say whether tax is rounded per line or per
  document, and how. Some amounts need rounding that the domain model's "rounding happens in
  exactly three places" didn't list: an item sold by weight ($12.99 per kg × 0.453 kg is
  $5.88447) and a percentage discount (20% of $9.99 is $1.998).
- **US sales tax** is added to the price, and depends on the item's tax category and, for
  prepared food in some states, on whether it is eaten on the premises. Discounts the seller gives
  reduce the taxable amount. Customers with exemption certificates don't pay some taxes.

## Decision

1. **Pricing is a pure function**: `price(basket, rules) → totals`, in the `keel-pricing` crate,
   which depends only on `keel-types`.
   - A **basket** is what to price, in one currency: lines, each with its unit price, modifiers
     (each with a unit price, a quantity and nested modifiers), quantity, tax category, whether it
     is comped, and its line discounts; the order's discounts; whether the order is eaten on the
     premises or taken away; and the taxes the customer is exempt from. `keel-domain` builds a
     basket from an order.
   - The **rules** are the location's taxes and rounding modes.
   - The same basket and rules give the same totals, on every platform. Lines, discounts and
     taxes are identified by their position in the basket and the rules.
2. **The pipeline**, in fixed order. Rounding happens at five points, each with an explicit
   rounding mode, and nowhere else:
   1. **Extension.** A line's unit price is the item's price plus its modifiers'. A modifier
      counts its quantity times the sum of its own price and its nested modifiers'. The line's
      *gross* is its unit price times its quantity, rounded with the extension mode. Only a
      fractional quantity (a weighed or measured item) needs rounding.
   2. **Comps and line discounts.** A comp takes the whole line. Then each line discount, in
      order, takes from what is left of the line: a percentage takes that percentage of what is
      left, rounded with the discount mode; an amount takes itself, but never more than what is
      left.
   3. **Order discounts**, in order, each taking from what is left of the order: a percentage
      takes that percentage of the order's remaining subtotal, rounded once with the discount
      mode; an amount takes itself, but never more than the remaining subtotal. Each discount is
      then **allocated** to the lines in proportion to what is left of each, by largest
      remainder: every line gets its exact share rounded toward zero, and the minor units left
      over go one each to the lines with the largest remainders, ties to the earlier line. The
      shares add up to the discount exactly. What is left of a line after all discounts is its
      *net*.
   4. **Tax.** Each tax has a rate of 0 or more (some excise taxes exceed 100%) and applies to
      lines in its tax categories, optionally only when the order is eaten on the premises, or
      only when it is taken away. Taxes are added to the price (exclusive) and computed on each
      line's net, so discounts reduce the taxable amount. Taxes are independent of each other:
      there is no tax on tax. The customer's exemptions remove taxes from the whole basket.
      - **Per line:** each line's tax is its net times the rate, rounded with the tax mode.
      - **Per document:** each tax's total is the sum of the taxable nets times the rate, rounded
        once with the tax mode, then allocated to the taxed lines in proportion to their nets, by
        largest remainder as above.
   5. **Cash rounding**, such as to the nearest 5 cents, applies to the amount due in cash when it
      is paid, not to the order's totals. The engine provides it for the tender to call.

   Default modes: half away from zero everywhere, the usual commercial rule. The domain model's
   "three places" becomes these five.
3. **The totals** give, for each line: its gross, comp, each line discount, its share of each order
   discount, its net, and for each tax that applies, the taxable amount and the tax. For each order
   discount, the amount taken; for each tax, the taxable total and the tax. For the order: gross,
   discounts, net, tax and total. Every set of parts adds up to its total exactly, no amount is
   negative, and each allocated share is within one minor unit of its exact share.
4. **The trace** lists every step above, in order, with its inputs (quantities, rates, modes) and its
   result, including any discount limited to what was left. It explains every amount in the totals,
   and a test checks that replaying it reproduces them.
5. **Invalid input is refused**, never guessed at: another currency, a negative price or discount,
   a quantity that isn't positive, a percentage discount above 100%, modifiers nested more than 32
   deep, a negative tax rate, two taxes with the same identifier, or an amount that overflows.
6. **Deferred** to later versions, each needing a design of its own:
   - price-list resolution, and modifier pricing from the catalog's rules (free selections, half
     pricing): these happen when a line is rung up, and need the catalog;
   - promotions: conditions, stacking and exclusivity, mix-and-match and BOGO;
   - service charges, fees and surcharges, and tips;
   - tax-inclusive (VAT) prices, compound taxes, per-item thresholds (such as clothing under a price
     cap), tax holidays (a change of rules by date), taxes on the SNAP-paid portion, discounts that
     don't reduce the taxable amount (manufacturer coupons), and destination-based tax;
   - returns and negative lines.

## Consequences

**Positive**
- One function prices every basket the same way everywhere, with no floating point and one
  rounding per rounding point.
- Every line carries its share of every discount and tax, so returns can refund exactly what was
  paid.
- Per-line and per-document tax rounding are both supported, as jurisdictions require.
- The trace answers "why is this the price" at the counter and in an audit.

**Negative**
- Line and discount positions identify things in the totals, so callers must keep the basket's order
  when they map results back.
- A change of pricing rules changes totals for baskets priced afterwards. Totals must be stored with
  each check (slice 3), not recomputed from old orders with new rules.
- Much of the pipeline (price lists, promotions, fees, VAT) is still to come. The types leave room for
  it: the pipeline's order is fixed, and new steps slot in without changing the existing ones.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Resolving prices from the catalog whenever totals are computed | Totals would change when the catalog changes. The line's snapshot is what was rung up; its catalog version explains it. |
| Spreading a percentage order discount by taking the percentage of each line | The discount would no longer be the percentage of the subtotal: the lines' roundings add up to a different amount. |
| Adding line discounts up against the gross | Two discounts could take more than the line; applying them in turn to what is left can't. |
| One combined tax rate only | Can't express a local tax that applies to fewer categories than the state's. The rules allow either. |
| Rounding tax only per line | Many US states compute tax on the whole invoice; per-document rounding is required. |
| Floating point, or rounding intermediate results | Totals would differ between platforms and from the law's arithmetic. |

## References
- [Domain model §4 and §8](../architecture/domain-model.md#8-pricing-and-tax-calculation),
  [compliance.md §3](../architecture/compliance.md#3-tax-engine)
- [Research: tax engine requirements](../research/03-payments-compliance.md#6-tax-engine-requirements)
- Implementation: [`core/crates/keel-pricing`](../../core/crates/keel-pricing/)
