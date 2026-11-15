# ADR-0035: Card payments: a sale through a semi-integrated terminal, as an effect, resolved before any retry

- **Status:** Proposed
- **Date:** 2026-11-14

## Context

The MVP's seventh step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is card payments: a
processor-agnostic connector (I-01), semi-integrated terminals (I-02), tips (I-05), partial
approvals and split tender (I-07), and the card refunds that returns left for it (I-08,
ADR-0032). It is built against a simulated terminal until gate G2 chooses the first processor.
The MVP is counter service: a café or a quick-service counter, where a tip is chosen before the
card is presented, on the register's screen or on the terminal.

What is built shapes it:

- The **payment aggregate** already has a card tender, an authorization, and a capture with a tip
  and a processor's reference (ADR-0015). Its identifier is the idempotency key every request for
  it carries. Checkout starts a payment only on an open check with no unresolved payment, so an
  outcome no one knows blocks the check until it is found out.
- A card refund is "initiated and completed by its connector"; `refund.initiated` has a card
  tender, never written yet (ADR-0032).
- Anything that touches the outside world is an **effect** in the store's outbox, enqueued in the
  write that causes it; an executor starts it, and the start commits, before it acts; one found
  started after a restart is in doubt, and is resolved by a status query, where there is one,
  before any retry (ADR-0017; offline and sync §10). A card effect's executor is the device with
  the terminal. Print jobs work so (ADR-0024): the runtime hands a job to the shell, which does
  the I/O and reports what happened.
- The connector SPI (payments §3; ADR-0005) gives the operations, the outcomes (`Approved`,
  `PartiallyApproved`, `Declined`, `Pending`, `StoredOffline`, `Unknown`) and the capabilities.
  An `Unknown` outcome blocks any new attempt on the check until a status query resolves it.
- Ethical tipping (H-10): off by default for counter service; *No tip* and *Custom* as prominent
  as the presets; presets computed on the pre-tax subtotal, shown as a percentage and an amount.

## Decision

1. **The connector is a contract of requests and outcomes, executed by the shell.** The kernel
   decides what to ask a terminal and records what it answered; the shell's connector does the
   I/O, as a printer driver does, so the kernel stays free of I/O and of any processor. Each
   request carries its idempotency key: the payment's identifier for a sale.

   | Request | Fields |
   |---|---|
   | sale | key, amount, tip (optional: chosen on the register), presets (optional: the terminal asks for the tip), partial approval allowed |
   | status | key, and the sale it asks about |
   | refund | key, amount, the original payment's processor reference |

   | Outcome | Fields |
   |---|---|
   | approved | the amount charged or refunded, a tip included; less than asked for a partial approval; a reference (optional), the card (optional) |
   | declined | a reason code, a note (optional), a reference (optional) |
   | not found | to a status query: the terminal never took the request |
   | unknown | when to ask again (optional) |

   A connector states what it can do: ask for a tip, approve in part, refund by reference. The
   shell asks for no tip on the terminal, and allows no partial approval, from one that can't.
   Pairing a terminal, and which connector a register uses, are the device's settings, as its
   printer is. `Pending` and `StoredOffline` come with asynchronous rails and offline payments,
   not here.

2. **A card sale is an effect.** *Card* initiates the payment for what the check owes, or a part
   of it, and enqueues the sale, in one write. The runtime hands sales to the shell one at a
   time; a sale's start commits before the terminal is asked. Its outcome is recorded in one
   write:
   - **approved**: captured, with the reference and the card: what was approved pays the check
     first, up to what the sale asked, and the rest is the tip, wherever it was chosen; once the
     check's payments cover it, it closes, its receipt prints, and what of it is pending fires
     to the kitchen, as a cash payment does;
   - **approved in part**, for less than the check's amount: so captured, with no tip, and the
     rest stays due, for any tender (I-07); approved for less than the amount and the tip, but
     more than the amount, the check is paid and the tip is what is left;
   - **declined**: failed, for its reason; the check can be paid again;
   - **unknown**: nothing is recorded; the payment stays unresolved, so the check takes no other
     payment, and the sale becomes a status query, asked after the outcome's wait, or 5 s,
     doubling up to 60 s;
   - **not found**: failed, as `not_received`.

   A sale found started when the register opens, its outcome never recorded, is in doubt: it is
   asked about, never sent again. Staff can cancel a sale waiting on the terminal from the
   register; the terminal answers it as declined, `cancelled`, or as approved if the card was
   read first.

3. **A payment no terminal can answer for is resolved by hand**, on a manager's word: a new
   action, `payment.resolve` (ADR-0029). Whoever resolves it says whether the card was charged,
   from the terminal's own record: charged, it is captured for what was asked, with no
   reference and no card, and marked resolved by hand; not charged, it fails as `not_charged`.
   Its effect is done either way, so nothing asks about it again.
   A role that permits it needs profile format 4 (decision 5), and its approval is
   `approval.granted` version 2, with version 1's keys: version 1 never decoded the action, and
   still doesn't. An approval of an earlier action is written as version 1, as before.

4. **The card on the payment**: `payment.captured` version 2 adds key 6, the card: `{1 brand, 2
   last four, 3 entry: 0 tap, 1 chip, 2 swipe, 3 keyed, 4 authorization code (optional)}`. The
   brand is a name; the last four are four ASCII digits; an authorization code is 1 to 12 ASCII
   letters and digits. Only a card payment has one. A capture without a card is written as
   version 1, so a cash payment's bytes don't change, and a kernel that knows version 1 alone
   reads every cash payment. Keel never sees more of a card than this.

5. **Tips** are the location's setting: profile format 4 adds key 14, tipping: `{1 presets, 2
   where: 0 the register's screen, 1 the terminal}`. The presets are one to four percentages,
   in basis points, ascending, each 1 to 10,000. With no tipping, there are no tips (H-10). A
   preset's amount is that percentage of the payment's share of its check's pre-tax subtotal:
   `amount paid × the check's net ÷ its total`, rounded half up, so a check paid at once tips on
   its net. The register offers *No tip*, each preset as a percentage and an amount, and
   *Custom*, alike. A tip is more than zero and no more than the payment it rides on: a guard
   against a slip. Where the
   terminal asks, the request carries the presets, and the tip is what the terminal says;
   where it can't ask, the register does. A profile with tipping is written as format 4; one
   without, byte for byte as before.

6. **Card refunds go back to the card, by reference.** A return's refund goes to one tender, as
   the cashier chooses among those that can take it all: cash, up to the cash the order took
   (ADR-0032); or one of its card payments, up to what it captured, its tip left out, less what
   refunds to it were initiated for. `refund.initiated` version 2 adds key 4, the payment a card
   refund goes back to; a cash refund is written as version 1. The refund is an effect, keyed
   by the refund and its attempt; approved, the refund completes; declined, a new schema, v1:

   | Schema | Keys |
   |---|---|
   | `refund.failed` | 1 reason code, 2 note (optional), 3 processor reference (optional) |

   records that the attempt moved nothing. The refund stays owed, shown and reported so, and
   is tried again on a manager's word as `refund.issue`; a completion after a failure applies,
   and a failure after a completion doesn't, and is reported. An unknown refund is asked about,
   as a sale is.

7. **What the register shows and prints**: the ticket shows each card payment as waiting for the
   terminal, being checked, approved, approved in part, or declined, and why. A receipt shows
   each card's brand, last four, entry and authorization code, and its tip; one resolved by
   hand says so. The day's report shows tips by tender and by the member who took them, and
   card refunds owed.

8. **The store** projects payments at version 2, adding who took each one, and refunds at
   version 2, adding the payment a card refund goes back to, and a refund's failure. Projections:
   no new store schema.

9. **Proven against a model**: a property test runs sales, refunds and status queries through a
   model terminal that keeps a ledger by key, loses requests and answers, answers unknown, and
   restarts the register between any two steps; once everything is resolved, the terminal
   charged each key at most once, and what the store recorded agrees with its ledger.

10. **Built in slices**, each verified as it lands, then reviewed:
    1. *The domain and the store*: `payment.captured` v2, `refund.initiated` v2 and
       `refund.failed`, their folds and commands; profile format 4 and its tipping;
       `payment.resolve`; the projections.
    2. *The register*: the contract, the sale and refund effects, outcomes, status queries and
       resolving by hand, tips, receipts and reports, the model test, the bindings.
    3. *The shell*: the Android register's card tender, tip prompt, waiting, partial and unknown
       states, card refunds, the terminal setting, and a simulated terminal.

11. **Out of scope**: a real connector, and pairing a real terminal (after G2); authorizing now
    and capturing later, tip adjust, incremental authorization and tabs (I-06, with full service
    in v1); offline store-and-forward and its risk envelope; SoftPOS; surcharging; voiding a
    captured sale (a return refunds it); unreferenced refunds, refunds at another location and
    to store credit; EMV receipt data beyond the card's brand, last four, entry and code;
    settlement and reconciliation; tip pooling and payroll; card payments in the simulator's
    workload.

## Consequences

**Positive**
- No processor is in the kernel: a connector is written for the shell, against one contract, and
  switching processors loses nothing Keel recorded (I-01).
- A card is never charged twice for a payment: every request carries the payment's key, an
  outcome no one knows is asked about before anything else is tried, and a sale in doubt is
  never sent again.
- A tip is offered fairly, on the pre-tax subtotal, with *No tip* as prominent as any preset.

**Negative**
- A terminal that never answers leaves its check blocked until a manager resolves it by hand.
- A card's sale and refund are effects on the device that took it: another register can't ask
  about them.
- Four new schema versions, a new schema and a profile format, which every replica's kernel
  must know before any device writes them.
- Counter service only: a tip on the slip, after the card, waits for v1.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| The connector as a Rust trait the runtime calls | The runtime would block on a terminal for as long as a customer takes, and do I/O; terminal SDKs ship for Android and iOS, so most connectors are the shell's anyway. |
| Authorize, then capture with the tip | A check stays open, its payment unresolved, until a tip is entered: full service's flow, which comes with tabs in v1. |
| Record an unknown outcome as an event | Every device would see a state that one device's terminal alone can resolve; the payment's being unresolved already blocks the check everywhere. |
| A partial approval voided, the customer asked for another card | Split tender pays the rest with any tender; voiding loses an approval the customer may want to keep. |
| A refund split across the order's tenders | One return names one refund; a cashier who wants two tenders returns in two parts. |

## References
- [ADR-0005](./0005-processor-agnostic-payments.md), [ADR-0013](./0013-event-payloads-and-schema-evolution.md),
  [ADR-0015](./0015-checks-and-payments.md), [ADR-0017](./0017-projections-and-outbox.md),
  [ADR-0024](./0024-receipts-and-printing.md), [ADR-0029](./0029-staff-roles-and-approvals.md),
  [ADR-0032](./0032-returns-and-refunds.md); payments §3, §5, §6; offline and sync §10;
  [feature catalog H-10, I-01, I-02, I-05, I-07, I-08](../product/feature-catalog.md)
