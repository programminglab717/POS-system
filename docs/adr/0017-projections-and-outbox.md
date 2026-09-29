# ADR-0017: Projections and the outbox: per-stream projections recomputed in each write, and a transactional effect outbox

- **Status:** Accepted (2026-09-29), with the details settled in building it, under "As built",
  accepted after review
- **Date:** 2026-09-29

## Context

[ADR-0016](./0016-device-store.md) keeps each device's events in SQLite, one transaction per
write. Its second slice adds what the store derives from events, and the effects events cause.
The design already fixes the principles:

- State is a **projection**, computed by total, deterministic folds; side effects go through a
  **transactional outbox**, with idempotency keys derived from event identifiers; events,
  projections and outbox are written in one transaction
  ([ADR-0002](./0002-event-sourced-signed-event-log.md)).
- An aggregate's state is the fold of its stream in **canonical order**. A late event whose
  position is earlier than events already applied means folding again; aggregates are small
  ([offline-and-sync §4](../architecture/offline-and-sync.md#4-ordering-and-deterministic-state)).
- Every effect, such as a card charge, a print job or a fiscal submission, has **one executor
  tier** and an idempotency mechanism, and an unknown outcome is resolved by a status query before
  any retry ([§10](../architecture/offline-and-sync.md#10-side-effects-the-outbox-and-effect-ownership)).
- The simulator checks, on every run, that **all replicas' projections are byte-identical after
  healing**, and that **no payment effect executes twice for one idempotency key**
  ([§12](../architecture/offline-and-sync.md#12-verification-deterministic-simulation-testing)).
- The command pipeline loads aggregates, decides events, then appends them, updates projections
  and enqueues effects in one transaction, and the UI renders projections
  ([architecture §5](../architecture/README.md#5-the-kernel)).

What the store holds today limits what it can derive. A check's balance needs the location's
pricing rules, which are reference data the store doesn't hold yet
([ADR-0015](./0015-checks-and-payments.md)). And no code produces effects yet: payments, printing
and fiscal submissions arrive with their crates.

## Decision

1. **A projection is one row per stream, computed from the stream's events alone.** The store
   folds the stream in canonical order with `keel-domain`'s fold, and maps the state to columns.
   Two projections to begin with:
   - `orders`: each order's status and stage, its attributes, how many live lines, open checks
     and closed checks it has, how many conflicts and unreadable events, its business date, and
     the HLCs of its first and last events;
   - `payments`: each payment's order and check, tender, amount, status, what its capture
     moved (amount and tip), its conflicts and unreadable events, its business date, and its
     first and last HLCs.

   A projection depends only on the set of events its stream holds, so replicas holding the same
   events hold byte-identical projections, whatever order the events arrived in.
2. **Each write recomputes the projections of the streams it touched, before it commits.** The
   events a write appends, and the received events it stores, mark their streams. Before the
   commit, the store reads each marked stream in canonical order, folds it, and replaces its row.
   Projections share the write's transaction, so they never disagree with the events they come
   from, and a write that fails leaves them as they were. Folding the whole stream again handles
   a late event without any incremental logic: an order is tens to hundreds of events. Keeping
   folded state between writes, and folding only what's new, are optimizations for when they're
   measured to be needed.
3. **Projections are versioned and rebuilt from the log.** Each projection has a version, and the
   store records the version it built. When a kernel's version differs, the store drops the
   projection's table and rebuilds it from every stored stream of its kind, when it opens, in one
   transaction. A projection's version changes whenever its columns change, or the fold it uses
   changes what it computes. A golden test pins each projection's rows for a fixed set of events,
   so such a change can't go unnoticed. The store can also rebuild on request, for support.
4. **Loading an aggregate** folds its stream in canonical order, inside a write or outside one:
   the first step of the command pipeline. A write also reports the streams it has touched, so
   the shell knows which projections to show again.
5. **The outbox holds effects, durably.** It isn't derived: an effect waiting to happen can't be
   recomputed from events. An effect has an idempotency key (unique in the store), a kind, a
   payload the store doesn't read, the event that caused it, a due time, a count of attempts, and
   a state:
   - **enqueue:** a new effect is pending and due at once. A key the outbox already holds changes
     nothing;
   - **start:** a pending effect that is due becomes running, and counts an attempt;
   - **finish:** a running effect is done;
   - **retry:** a running effect is pending again, due later;
   - **fail:** a running effect has failed for good, for a person to look at.

   Every change happens inside a write, so an effect is enqueued with the events that cause it,
   and an outcome recorded as events commits with the effect's finish. The executor reads only
   committed effects, and acts on an effect only after its start has committed. A running effect
   found after a restart is in doubt: the store reports it and never restarts it on its own. Its
   executor resolves it, by a status query for a payment, and then finishes or retries it. Which
   replica enqueues an effect, its executor tier, is decided by the code that produces it.
6. **Schema version 2** adds the outbox and the projections' versions. Projection tables are the
   projections' own, created and dropped with their versions. Opening a version 1 store migrates
   it and builds its projections.
7. **Verification**, as for the event log, plus:
   - property tests against models: after every write, rollback and reopening, each projection
     equals the model's fold of the stream's committed events; stores fed the same events in
     different orders, with different write boundaries, converge to byte-identical projections;
     a rebuild changes nothing; the outbox follows a model of its states;
   - crash tests: after every crash, the projections equal a rebuild from the stored events, and
     the outbox holds exactly the effects of the writes that committed.

## As built

Details settled in building it, for review with it:

- **What the rows hold.** `orders`: the order's state (`uncreated` until the store holds its
  creation, then `active`, `closed`, `voided` or `abandoned`), its stage, everything it was created
  with and its attributes since, the business date of the event that created it, its live lines,
  open and closed checks, conflicts and unreadable events, how many events it has, and its first
  and last HLCs. `payments`: the payment's state (`uninitiated` until the store holds its
  initiation), everything it was initiated with, the business date of that event, a capture's
  amount and tip, its conflicts and unreadable events, how many events, and its first and last
  HLCs. Channels, modes and tenders are stored as their payload codes, which never change.
- **Reads:** an order or a payment by identifier; the orders in a state, the earliest first; an
  order's payments, the earliest first; and an aggregate, folded from its stream, inside a write
  or outside one.
- **The cost of folding again**, measured on the development machine in a release build: about
  2.3 µs for each event of a stream a write touches, so 0.25 ms for an order of 100 events, on
  top of 0.2 ms for the write itself before it waits for the disk. A low-end device may be ten
  times slower: an order of a few hundred events still costs a few milliseconds.
- **When projections are rebuilt:** when the store opens, after it has checked it belongs to the
  device, if a projection's version isn't the one the store built; a new store builds them all.
  The rebuild has a fault point of its own, `Rebuilding`, and runs in one transaction.
- **The outbox:** an effect's kind is dotted lowercase words, at most 64 bytes, such as
  `print.receipt`; a key is 1 to 64 bytes; a payload at most 64 KiB. Enqueuing a key the outbox
  holds is a no-op when the effect is the same, and refused when it isn't, since two effects
  under one key would be deduplicated as one by their executor. An effect's cause must be an
  event the store holds, the write's own included. Due effects come earliest due first, then in
  the order they were enqueued.

## Consequences

**Positive**

- Projections can't disagree with the events they come from, on any replica, and converge by
  construction: the simulator's first invariant holds for the store.
- An effect is never acted on before the write that caused it is durable, and an effect whose
  outcome is unknown is never retried blind, which is what keeps a payment from executing twice.
- A projection's shape can change freely: it is dropped and rebuilt, never migrated.

**Negative**

- Each write folds the streams it touches from the start, so its cost grows with the streams'
  length. Measured in building it; long-lived streams, such as a gift card's, will need folded
  state kept between writes.
- Check balances wait for the pricing rules to reach the store.
- A change to a fold must bump the version of every projection that uses it, and so rebuild them
  on every replica. The golden tests catch a change, but the bump is a person's to make.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Folding incrementally, event by event, with snapshots to fold again from | Faster for long streams, but late events make it subtle, and orders are short. It can come later, checked against the full fold. |
| Projections kept by SQL triggers | Folds are Rust, total and tested; SQL would be a second implementation of them. |
| Projections migrated like the core schema | Derived data can always be rebuilt, which is simpler and can't go wrong in a way a migration can. |
| Effects derived from events by rules on every replica | Every replica would enqueue the same print job or charge; effects belong to one executor tier. |
| An outbox outside the write's transaction | An effect could be lost, or acted on for events that were never stored. |
| Restarting running effects after a crash | The first attempt may have happened; a card could be charged twice. |

## References

- [ADR-0002](./0002-event-sourced-signed-event-log.md) (event sourcing and the outbox),
  [ADR-0016](./0016-device-store.md) (the device store)
- [offline-and-sync.md §4, §10, §12](../architecture/offline-and-sync.md),
  [architecture §5](../architecture/README.md#5-the-kernel)
- Implementation: [`core/crates/keel-store`](../../core/crates/keel-store/)
