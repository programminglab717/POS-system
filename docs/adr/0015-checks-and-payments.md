# ADR-0015: Checks and payments: line shares, per-check pricing, closing snapshots, and the payment aggregate

- **Status:** Accepted (2026-09-28)
- **Date:** 2026-09-28

## Context

An order is what was asked for; a check is what one party pays
([domain model §6.1](../architecture/domain-model.md#61-structure)). Quick service has one check
per order. Full service splits by seat, by item, evenly, or by a fraction of a line ("split the
wine three ways"), and re-splits after some checks are paid. The design already fixes some rules:

- a split never duplicates lines;
- money is allocated by largest remainder, so the parts always add up to the line to the cent;
- paid allocations are frozen, and the later of two concurrent splits wins for unpaid lines
  ([offline-and-sync §5.2](../architecture/offline-and-sync.md#52-conflict-rules)).

Payments are their own aggregate, driven by payment connectors, so a server adding a dessert never
conflicts with a terminal capturing a payment (domain model §7). And
[ADR-0014](./0014-pricing-engine-v0.md) requires each check to store its totals, rather than have
them recomputed later with other rules.

Four facts shape the design:

- **Each check is a sale.** A check gets its own receipt, and later its own fiscal document. Sales
  tax is computed per sale, so a check's tax must be computed on that check, not taken as a share
  of the whole order's tax.
- **Fractions must be exact and add up.** A third of a bottle isn't a quantity: millionths can't
  hold a third, and a third of 10.00 can't be 3.33 three times over.
- **Checks and lines change together.** The order's owning device performs splits and payments
  (offline-and-sync §5); any device may add lines. A split must stay consistent with the lines it
  splits, even when devices act concurrently.
- **Payments are facts.** Money that moved stays recorded, even when it conflicts with the order:
  a payment on a voided order, or two payments that together exceed the check.

The work is built in two slices: checks and splits (decisions 1 to 4), then payments and closing
(decisions 5 to 9).

## Decision

### Checks and splits

1. **Checks are part of the order's stream.** Every order has a *main check*, number 1, from its
   creation. Its identifier is the order's own, so a one-check order needs no check event.
   `order.check_opened` opens more checks, numbered in the order they are opened.
2. **Lines are allocated to checks in whole shares.** `order.lines_allocated` gives each line it
   lists a new allocation: the checks the line belongs to, and each one's number of shares.
   - Moving an item to a check is one share on that check. Splitting the wine three ways is one
     share on each of three checks. Two thirds and one third are two shares and one.
   - Each line's shares are in lowest terms, so every allocation has exactly one encoding.
   - A new line goes to the main check. Every live line belongs to at least one check, and only
     to checks that exist.
3. **Each check is priced as its own sale.** Its basket holds the lines allocated to it.
   - A line on one check is in that check's basket whole.
   - A shared line contributes its share. The line's gross (its unit price times its quantity,
     rounded once) is split among its checks by largest remainder, in the order of the checks'
     identifiers, and each check's basket holds its part. The parts always add up to the line,
     and each is within one minor unit of its exact share. Comps and discounts then apply to the
     part.
   - Tax is computed on each check.
   - `keel-pricing` gains a line's *share*: the weights of all the line's parts, and which one the
     basket holds. ADR-0014's allocation rounding point covers it.

   The checks' taxes can add up to a cent or so more or less than the tax on the whole order
   priced as one sale. That is correct: each check is a separate sale.
4. **Concurrent splits resolve in the fold**, identically on every replica:
   - for each line, the later allocation in canonical order wins;
   - an allocation naming a check that doesn't exist yet doesn't apply to that line, and leaves a
     conflict (`UnknownCheck`);
   - a check opened twice keeps the first opening (`DuplicateCheck`);
   - an allocation of a line that was removed or voided changes nothing.

### Payments and closing (built in the next slice)

5. **Closing a check records what was charged.** `order.check_closed` stores the check's snapshot:
   the version of the pricing rules used; each line part's gross, net and tax; each tax's taxable
   amount and tax; the total; and the payments that settled it. Receipts, the ledger and returns
   read the snapshot, never a recomputation.
6. **Closed checks are frozen.** Commands can't re-allocate, change, remove, void or comp a line
   with a part on a closed check: a manager reopens the order first, and refunds come with
   returns.
   - When such an event arrives anyway, from a device acting concurrently, the line follows it
     (the kitchen must know), the snapshot stands (the money moved), and the order reports a
     conflict.
   - A line allocated to a check that closed without it, because it was added concurrently,
     moves to the first open check, or to a new *post-close check*, with a conflict: unpaid
     additions.
7. **The order closes explicitly.** `order.closed` needs every check that holds a live line to be
   closed. `order.reopened`, with a reason, makes the order active again. Lines that a concurrent
   device adds to a closed order land in a new post-close check (`AddedToClosedOrder`).
8. **A payment is its own aggregate** (stream `payment`). Its identifier is the idempotency key
   the terminal or processor sees; it replaces the event identifier named in the sync design
   (offline-and-sync §10).
   - Version 1 has cash and card tenders and five events: `payment.initiated` (the order, check,
     tender and amount), `payment.authorized`, `payment.captured` (the amount applied to the
     check, a tip, and for cash the amount tendered and any cash rounding), `payment.failed` and
     `payment.voided`.
   - The payment's fold is a state machine: initiated, then authorized or captured, or failed or
     voided. An event out of turn still applies as recorded and leaves a conflict, because money
     may have moved.
9. **Checkout rules span the order and its payments.** A check's balance is its total (the
   snapshot once closed, the current pricing before) less its captured payments.
   - A payment can start only on an open check with no *unresolved* payment: one initiated or
     authorized without an outcome. An unknown outcome is resolved before any new attempt, so no
     check is charged twice ([payments.md §3](../architecture/payments.md#3-the-connector-spi)).
   - A check closes only when it has no unresolved payment and its captured payments cover its
     total.
   - Overpayment, a payment on a voided order, and a payment for a check the order doesn't have
     are reported, never hidden: the payments stand, and a manager decides.
10. **Deferred**, each needing a design of its own:
    - refunds, returns and disputes;
    - offline store-and-forward and asynchronous rails (the `StoredOffline` and `Pending` states);
    - tip adjustment and incremental authorization;
    - stored value, house accounts and external accounts;
    - surcharges, and discounts on a check;
    - check names, and putting a new line straight onto the check it is for;
    - each line's tax per tax in the snapshot, and re-pricing a snapshot to audit it;
    - re-splitting a line after part of it was paid.

## Consequences

**Positive**
- A one-check order, the counter-service case, needs no check or split events.
- Every check is right on its own receipt: its tax is computed on it.
- Splits are exact: shares are whole numbers, and every line's parts add up to it.
- Money that moved is never lost or hidden by a conflict.

**Negative**
- A split order's total can differ by a cent or so from the same order unsplit, which staff and
  guests may notice.
- Pricing one check needs the whole order: a shared line's part depends on every check sharing it.
- Until payments are built, checks can be split but not closed.
- While a line has a part on a closed check, its split is frozen, so the other checks sharing it
  can't re-split it.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Price the order once, and allocate its tax to the checks | A check's tax wouldn't be the tax on that check, so its receipt wouldn't add up |
| Split quantities by fractions (a third of a bottle) | Not exact: millionths can't hold a third, and the parts wouldn't add up to the line |
| A fraction per check, as a numerator and a denominator | Fractions must be checked to add up to one; whole shares add up by construction |
| Checks in their own stream | Splits must stay consistent with the lines they split, and the order's owner coordinates both |
| Payments in the order's stream | A terminal capturing a payment would conflict with a server adding a line; connectors drive payments, not the order's owner |
| An event for every check, the first included | Every order would need one more event; the main check comes with the order |
| New lines on the check they are for | Needs a new version of `order.line_added`; the owner moves them with one allocation instead |

## References
- [Domain model §6 and §7](../architecture/domain-model.md#6-orders--the-universal-transaction),
  [offline-and-sync §5 and §10](../architecture/offline-and-sync.md#5-check-ownership-and-conflict-semantics),
  [payments.md](../architecture/payments.md)
- [ADR-0005](./0005-processor-agnostic-payments.md) (processor-agnostic payments),
  [ADR-0013](./0013-event-payloads-and-schema-evolution.md) (payloads),
  [ADR-0014](./0014-pricing-engine-v0.md) (pricing)
- Implementation: [`core/crates/keel-domain`](../../core/crates/keel-domain/),
  [`core/crates/keel-pricing`](../../core/crates/keel-pricing/)
