# ADR-0021: Ownership leases: an order's owning device in its own events, requests answered by the hub, and a manager's override when the hub can't be reached

- **Status:** Proposed
- **Date:** 2026-10-01

## Context

Slice 3 of step 6 is ownership leases
([ADR-0019](./0019-replication-and-deterministic-simulation.md), decision 8): "the hub grants
and transfers orders' ownership; structural and money commands need it, commutative ones don't;
island mode and the manager's override".

The design fixes the purpose ([offline-and-sync.md §5](../architecture/offline-and-sync.md#5-check-ownership-and-conflict-semantics),
[ADR-0006](./0006-own-sync-protocol.md), decision 4, after Oracle Simphony's CAPS):

- Each open order has exactly one **owning device**: by default the device that opened it.
  Ownership moves through the hub, in under 100 ms.
- **Structural and money operations need ownership**: payments, splits, discounts and comps,
  voids, closing and reopening. **Commutative operations don't**: adding lines, firing and
  notes, from any device.
- Another device asks for an order with one tap. With the hub reachable, it gets it at once,
  unless the owner is in the middle of a payment.
- An **island**, a device that can reach neither hub nor cloud, keeps working on the orders it
  owns. It takes over another device's order only through a **manager's override**, which is
  logged, flagged for reconciliation, and fenced by the owner's lease.
- Events are facts: nothing is ever rejected at merge. What a device did without owning the
  order stands, and is flagged.

What slices 1 and 2 built shapes the rest:

- Folds see each event's device, and run in canonical order, by HLC
  ([ADR-0019](./0019-replication-and-deterministic-simulation.md), decision 2). An event a
  device made after it heard of another always folds after it.
- The hub's decisions can be events in its own log, signed and replicated like any other, and
  made in a write of their own after the write that stored what they answer, as sequencing is
  ([ADR-0020](./0020-hub-sequencing.md)).
- Order commands are checked on the device against its own view; their code already says the
  owning device's lease is checked "by other parts of the kernel".
- The store keeps each payment's order and state in a projection
  ([ADR-0017](./0017-projections-and-outbox.md)), so the hub can see a payment in progress.

Time is left out of this slice: the hub taking an order back from a device it hasn't heard from
needs heartbeats, which come with election in slice 4.

## Decision

1. **Ownership is part of the order, decided by its own events.** The order's fold keeps its
   **ownership**: a device and a **lease**, a number that counts the order's changes of owner.
   - The device that records `order.created` owns the order, under lease 0. Every order already
     recorded has an owner, with no migration.
   - Each change of owner names the lease it replaces, and takes the next. It applies only if
     that lease is still the order's at that point in canonical order. So two changes made from
     the same lease never both apply, wherever they were made, and every replica, folding the
     same events, finds the same owner.
2. **Four new order events**, each a new schema at version 1, integer-keyed and strict
   ([ADR-0013](./0013-event-payloads-and-schema-evolution.md)), pinned byte for byte:

   | Event | Recorded by | Payload |
   |---|---|---|
   | `order.ownership_requested` | a device that wants the order | 1 the lease it saw |
   | `order.ownership_granted` | the hub | 1 the request (an event identifier), 2 the device, 3 the new lease, 4 the hub's epoch |
   | `order.ownership_refused` | the hub | 1 the request, 2 why: 0 the lease had moved on, 1 the device already owned the order, 2 a payment was in progress |
   | `order.ownership_overridden` | a device, on a manager's word | 1 the lease it overrides, 2 the reason's code, 3 its note (optional) |

   A grant names its device and lease, so a replica that holds it before the request still
   knows the owner. It carries the hub's epoch, so that slice 4 can fence a deposed hub's grants
   without a new schema version. A request is always for the device that records it: handing
   an order to another device is the other device's request.
3. **What the fold does with them**, in canonical order:
   - A request joins the order's **pending requests**, until a grant or refusal answers it.
   - A grant from the current lease makes its device the owner under the new lease. A grant from
     a lease that has moved on doesn't apply: a **stale grant**, flagged. Only an override the
     hub hadn't heard of, folding before the grant, can make one.
   - A refusal changes nothing but the pending requests.
   - An override from the current lease makes its device the owner under the next lease, and is
     always flagged, for reconciliation. An override of a lease that has moved on doesn't apply:
     a **stale override**, flagged.
   - An event that only the owner may record, recorded by another device, applies as it always
     has, and is flagged **not the owner's**, naming its device.
   - An answer to a request the fold doesn't know is only that: the request hasn't arrived yet,
     and folds before its answer once it does.
4. **Which commands need ownership.** On the device, against its own view, before any event
   exists:
   - **Owner only:** changing, removing, voiding and comping lines; opening checks and
     allocating lines to them; closing, reopening, voiding and abandoning the order; and
     starting a payment, in checkout.
   - **Anyone:** creating the order, adding and firing lines, and changing its attributes, which
     resolve by last writer as before (offline-and-sync §5.2).
   - A payment's outcome is recorded by the device that started it, whoever owns the order by
     then: money that moved is a fact.
   - Requesting needs an active or closed order the device doesn't own, and no request of its
     own still pending; closed, since reopening needs ownership. Overriding needs an order the device
     doesn't own, and a reason.
   - `Order::decide` takes the device as well as its location, and refuses an owner-only command
     from any other device (`NotOwner`), naming the owner.
5. **The hub answers requests.** After each write that stores events, once its log is settled,
   the hub answers every pending request it holds, order by order, each order's in canonical
   order, in one write, before it sequences:
   - **refused** if the request's lease is no longer the order's (the lease moved on), or its
     device already owns the order;
   - **refused** if a payment of the order is in progress: initiated, with no outcome yet. An
     authorized payment, such as a tab's, doesn't hold the order;
   - **granted** otherwise, under the next lease. A grant moves the lease, so the next request
     for the order from the same lease is refused.

   A request for an order whose creation the hub doesn't hold yet waits for it. The answers are
   a pure function of what the hub holds (`Order::answers`), so they are tested as a function,
   and every request has exactly one answer from a hub that holds it.
6. **Island mode** is these same rules where the hub can't be reached: a device works on what it
   owns, and its requests wait. A manager's override is the one way to take an order without the
   hub, online or not. When the island rejoins, canonical order decides who owns each order, and
   the flags show what was done without ownership. Telling that the device is an island, from
   the hub's heartbeats, comes with slice 4; until then the simulator, which knows its
   partitions, decides when a device overrides.
7. **The store and the replicator.**
   - The orders projection, version 2, keeps each order's owning device, lease and pending
     requests, indexed by orders with requests pending; `OrderSummary` reports them. The store
     builds version 2 from its events when it opens.
   - `Store::answer_requests(epoch, now)` answers, as the hub, every pending request the store
     holds, in a write of its own, and returns the answers. `Replica::answer_requests` is the
     store's, and a replicator with the sequencer role calls it before it sequences.
8. **Out of scope until later:**
   - the hub granting an order whose owner it hasn't heard from in a while, and telling a device
     it is an island: heartbeats, slice 4;
   - grants only from the hub elected for their epoch: slice 4, as for sequencing records;
   - checking who may override, a manager, with `keel-policy`;
   - the owner handing an order to another device without that device asking;
   - flagging a payment started by a device that didn't own its order, in the payment's own
     fold, which can't see the order: the device's check and checkout's issues cover it;
   - other leases: tables, store-wide order numbers, limited quantities (offline-and-sync §6.3).
9. **Verification**, for slice 3:
   - known answers:
     - the four payloads pinned byte for byte, checked with Python's `cbor2`, and each way each
       is refused;
     - the fold: the creator's ownership, a grant, a stale grant, a refusal, an override and a
       stale override, every owner-only event recorded by another device, pending requests, and
       an answer before its request;
     - each command's ownership check, and each of the hub's answers;
     - the store answering as the hub, the orders projection's new columns, and the replicator
       answering once settled;
   - property tests:
     - the payloads against the codec's model, as for every schema;
     - the fold against a model of ownership, over events of several devices in any canonical
       order, overrides and stale answers among them;
     - the hub's answers against a model, over requests, overrides and payments reaching a hub
       and a replica in different orders, with the replica agreeing with the hub;
   - the simulator: devices request orders they don't own and wait for the hub, and override
     when a partition cuts them off from it. New invariants:
     - every request the hub holds has exactly one answer, from the hub, after it;
     - the hub's grants for each order name each lease once, in increasing order in its log;
     - a stale grant only ever follows an override;
     - every replica agrees on each order's owner, lease and pending requests;
   - planted bugs in `keel-domain`, `keel-store` and `keel-sync`, and coverage probes.

## As built

Details settled in building it, for review with it:

- **The payloads** are as decision 2 describes, pinned byte for byte in payloads Python's
  `cbor2` encoded from the key tables. Refusals are coded from 0, as every code in the payloads
  is: 0 the lease had moved on, 1 the device already owned the order, 2 a payment was in
  progress; the design's table first said 1 to 3. An override's reason is a code and an
  optional note, as for voids and reopenings: 2 the code, 3 the note. A lease is from 0 to
  2^63 − 1, so that it fits the store; a grant's lease and its epoch are from 1.
- **The fold** keeps the owner, the lease, and the requests that wait, in canonical order.
  - An event only the owner may record (`OrderEvent::needs_ownership`), recorded by another
    device, is flagged before it applies, naming the device and the owner at that point:
    `NotOwner`. It then applies as it always has.
  - The four other conflicts: `StaleGrant`; `Overridden`, naming the device the override took
    the order from; `StaleOverride`; and nothing for a refusal.
  - **Answered requests are remembered.** An answer names its request wherever it folds, so a
    request whose answer folded first never waits. The design assumed a request always folds
    before its answer, but a store keeps an event from a device whose clock is too far ahead
    without moving its own clock past it, so the hub's answer can sort before the request it
    answers, and, with the hub's clock far enough behind, before the order's creation too, where
    the answer itself doesn't apply. Without the memory, such a request would wait for ever,
    and the hub would answer it again at every write. A hub more than a minute behind the
    devices can't give their new orders away, then: its grants sort before the orders exist.
    Slice 4's heartbeats can tell such a hub.
  - No lease follows the largest: an override of it is stale, and the hub refuses a request
    from it as if the lease had moved on.
- **The commands:** `Order::decide(location, device, command)`. `OrderCommand::needs_ownership`
  lists the commands of decision 4. A command on that list from another device is refused,
  `NotOwner`, naming the owner, once the order is found active at the device's location and
  before the command's own rules. A closed order takes three commands: reopening, from its
  owner; a request; and an override. Both a request and an override from the owner are refused,
  `AlreadyOwner`; a request from a device whose request still waits is refused,
  `RequestPending`, and an override doesn't wait for it. Checkout's `start_payment` and
  `close_check` take the device and check ownership the same way.
- **The hub's answers:** `Order::answers(paying, epoch)` checks the lease first, then the
  device, then the payment, each request in canonical order, as if the answers before it had
  applied.
- **`keel-store`:**
  - The orders projection, version 2, adds `owning_device`, `lease` and `requests`, and a
    partial index of the orders with requests waiting. `OrderSummary` gains `ownership` and
    `requests`, the number waiting.
  - `Store::answer_requests(epoch, now)` refuses an epoch out of range first. Then, in one
    write, for each order with requests waiting, by identifier, it folds the order and answers
    with the order's rules, a payment of the order in progress when the store holds one
    initiated with no outcome. It skips an order whose creation it doesn't hold. Each answer is
    an event on the order's stream, from the hub as `System("hub")`, caused by the request it
    answers, under the order's own business date.
- **`keel-sync`:** `Replica::answer_requests` is the store's. A replicator with the sequencer
  role answers whenever it would sequence (after a write that stores events it received, after
  its own appends, and when its log settles; never before) and then numbers what it wrote,
  answers included, in the same pass. Both go out like any new events.
- **`keel-sim`:**
  - A move that needs an order another device owns becomes a request, which waits for the hub;
    or, from a device that can't reach the hub (its link is cut, or the hub is down), a
    manager's override. A device also asks for an order it sees another device owns in 3 of
    every 100 moves it makes on any order, rather than on the sale it is working on.
  - Half the moves on any order go to the two latest active orders, so that devices contend
    for the same orders, and half the runs have three devices rather than three in ten: a grant
    made stale takes a third device, asking the hub for an order an island has taken.
  - Checked at the end, beyond decision 9's invariants: each answer is on its request's order,
    later in canonical order; a grant gives the requesting device the lease after the one it
    saw, in epoch 1; and no request waits on any replica, the orders a forked device wrote to
    aside.
  - A stale grant follows an override of the very lease it replaced. The hub grants each lease
    of an order once, so when a grant doesn't apply, only an override it hadn't heard of can
    have taken that lease first; the check is that strong.
  - The hub never refuses a request because its device already owns the order. In the
    simulator's star, a device hears of other devices' events only through the hub, so the
    hub's view of an order holds the device's, and a device's waiting request holds back
    another until its answer arrives. Only the known answers and the property tests reach it.
- **Costs**, measured on the development machine in a release build, on a RAM disk, with other
  tests running, at a hub holding 200 orders of about 20 events:

  | Operation | Cost |
  |---|---|
  | Answering, with nothing waiting: what each of the hub's writes pays | 7 µs |
  | Storing a request, a write like any other | 0.5 to 0.65 ms |
  | Answering it, a write of its own | 0.6 to 0.75 ms |
  | Answering ten requests on ten orders at once | 4 to 6 ms |

  A transfer costs the hub under 2 ms of the 100 ms the design allows it (see Context); the rest
  is the network's.
- **Verification**, beyond decision 9, much of it added when planted bugs went unnoticed:
  - `keel-store`'s property uses the order's own rules, `Order::answers`, as its model of what
    the hub writes, to check what the store feeds them: which orders, which payments are in
    progress, and how each answer is recorded. So `keel-domain`'s ownership property checks
    `Order::answers` against a model of its own: the grants form a chain from the owner, each
    to the first request after the last that saw the chain's lease. It also folds the answers
    after everything, and finds no request left waiting and the order with the last grant's
    device. Its steps sometimes fold the order's creation after the first few.
  - A payload property of its own for the four schemas, as payments and sequencing records
    have, and a sweep of every lease and epoch across the ends of its range: among every order
    schema, a grant's lease or epoch of 0 came up about once in 3,000 cases.
  - Commands, against the command model: in merged orders, every other racing device asks for
    the order before it overrides, so requests wait; commands, those anyone may make included,
    are tried from the owner, from each device whose request waits, and from one with none;
    the device carrying on asks twice, then overrides, and the requests it leaves must be the
    model's, leases included. Checkout's decisions are checked for a device that doesn't own
    the order.
  - `keel-store`'s property draws the hub's epoch, and dates its two orders a day apart.
    `keel-sync`'s known answer runs its hub in epoch 2.

## Consequences

**Positive**

- Who owns an order, and how it changed hands, is in the order's own history: signed,
  replicated, auditable, and the same on every replica, with no lease table to rebuild after a
  failover.
- A stale request or override can't take an order: each names the lease it replaces.
- Commands that need ownership are refused on the device, and anything done without ownership,
  by a concurrent transfer or an island, still stands and is flagged for a person.
- The hub's answers are a pure function of its store, testable as such, like sequencing.

**Negative**

- A transfer takes the hub two writes: storing the request, and answering it.
- The old owner may act in the moment before it hears of a grant. What it does stands, flagged
  "not the owner's"; the design accepts this for transfers in under 100 ms, as the conflict
  rules of offline-and-sync §5.2 do.
- An order whose owner died in the middle of a payment can be taken only by override until
  slice 4 brings heartbeats.
- The orders projection's version changes, so every store rebuilds it once.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Leases as frames between devices and the hub | Lost with the hub, unverifiable, and with no history; a failover would need the table passed on. As events, they replicate with everything else. |
| A stream of lease events per order, beside the order's | The order's fold couldn't tell which events were recorded without ownership, and the UI would have to read two streams to show one order. |
| Transfers only once the owner lets go, as a classic lease | Every transfer would wait for the owner, and one that can't be reached would hold its orders until its lease ran out. The design wants transfers at once, and flags the rare overlap. |
| The last claim wins, without leases | A request or override made long ago, or from a stale view, would take an order from whoever had it since. Naming the lease replaced makes every change compare-and-swap. |
| Refusing transfers while any payment is unresolved | A tab's authorized card would hold the order for the whole evening. Only a payment in flight, initiated with no outcome, holds it. |

## References

- [ADR-0006](./0006-own-sync-protocol.md), [ADR-0013](./0013-event-payloads-and-schema-evolution.md),
  [ADR-0017](./0017-projections-and-outbox.md), [ADR-0019](./0019-replication-and-deterministic-simulation.md),
  [ADR-0020](./0020-hub-sequencing.md)
- [offline-and-sync.md §5, §6.3, §7](../architecture/offline-and-sync.md),
  [domain-model.md §6](../architecture/domain-model.md)
