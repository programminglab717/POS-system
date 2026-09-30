# ADR-0020: Hub sequencing: signed sequencing records in the hub's log, confirmation by hash, and durability watermarks

- **Status:** Proposed
- **Date:** 2026-09-30

## Context

Slice 2 of step 6 is hub sequencing
([ADR-0019](./0019-replication-and-deterministic-simulation.md), decision 8): "`store_seq` per
epoch, recorded in the hub's own log as signed events, confirmed and provisional events, the
cloud's durable-ack watermark, and store durability once two replicas hold an event".

The design fixes the purpose:

- The hub numbers the store's events in the order it receives them, gapless within its epoch.
  An event with a number is **confirmed**; one without is **provisional**, and the UI says so
  where it matters ([offline-and-sync.md §4](../architecture/offline-and-sync.md#4-ordering-and-deterministic-state)).
- The numbers never move an event in a fold: every replica folds in HLC order (ADR-0019,
  decision 2). They give the store one gapless feed, mark what the store has confirmed, and
  order the hub's own decisions.
- Payment and close events are **store-durable** once two replicas hold them, and a device shows
  "not yet backed up" until then (§7). The cloud's version vector is the **durable-ack
  watermark**: devices show "all sales backed up" and prune only below it (§3.2, §9).

What slice 1 built shapes the rest:

- Every store records the order it received its events in (`events.arrival`).
- Projections are per stream, recomputed in each write that touches the stream, own their
  tables and versions, and are checked against a rebuild ([ADR-0017](./0017-projections-and-outbox.md)).
- The replicator knows what each peer last said it holds, and a replica settles its own log
  before it writes (ADR-0019, decision 4).

Election, epochs changing hands and fencing are slice 4's. This slice has one hub, in epoch 1.

## Decision

1. **Sequencing records are events in the hub's own log.** To sequence, the hub appends a
   `sequence.assigned` event, signed like any other, which replicates like any other:
   - its payload names the epoch, the store sequence number of its first event, and the events
     in order, as **runs**: a device, the first and last positions of a stretch of its log, and
     the hash of the stretch's last event;
   - each record is a stream of its own, of kind `sequence`, so its projection reads that record
     alone;
   - its actor is the system component `sequencer`, and its business date the latest among the
     events it sequences;
   - it holds at most 1,024 runs, far under the event size limit; the hub writes several records
     when it has more to sequence.

   The payload is an integer-keyed canonical map (ADR-0013), decoded strictly: at least one run;
   no two neighbouring runs of one device; each device's runs within a record contiguous; every
   number below 2^63, so that it fits the store; and the numbers the record assigns not
   overflowing. It is pinned byte for byte, and checked by an independent decoder.
2. **What the hub sequences, and when.**
   - The hub sequences every event it holds that no sequencing record it holds covers, except
     sequencing records themselves, in the order it received them. Each device's log arrives in
     order, so each is sequenced in order, whole.
   - It sequences after every write that stores events, and after its own log settles. It
     never sequences before its log is settled, however long that takes: sequencing isn't on the
     sell path, and a hub that sequenced after losing its latest records would fork its log, and
     with it the store's confirmations.
   - Sequencing is a write of its own, after the write that stored the events. A crash between
     the two leaves events unsequenced, and the next sequencing covers them.
   - The epoch comes from the hub's configuration: 1, until slice 4 elects hubs. Numbers start at
     1 in each epoch, and follow on from the hub's last record of the epoch.
3. **Every replica confirms from the records it holds.**
   - An event is confirmed when the replica holds a record with a run covering it, and the hash
     of the run's last event matches the replica's own copy of that event. The hash chain then
     pins every event of the run.
   - A replica that holds another version of a device's log, because the device forked it, finds
     the hashes don't match: the hub's numbers aren't for its version, and it never takes them
     as confirming it.
   - The store keeps the runs in a projection, `sequence`, one row per run, and answers:
     - `confirmed()`: for each device, how far into its log the replica holds confirmed events;
     - `store_seq(device, position)`: an event's epoch and number, once confirmed;
     - `sequenced(epoch, after, limit)`: the confirmed events of an epoch in number order, the
       store's feed.
   - Records arrive in any order relative to the events they cover. Confirmation depends only on
     what the replica holds, so it is the same everywhere once replicas hold the same events.
   - Folds don't change: an order's state is the same confirmed or not. The UI shows an order as
     provisional while any of its events is.
4. **Trust, for now.** A record from any device enrolled at the location counts, as any event
   does; slice 4's epochs and fencing restrict records to the hub elected for their epoch. If
   two records ever cover one event, its first number by epoch and number counts. The simulator
   checks this never happens.
5. **Store durability, factor 2.** A device's replicator reports how far into the device's own
   log a peer has said it holds (`store_durable`). The device shows "not yet backed up" past it.
   Peers say what they hold only once it is committed, so this needs no new frame.
6. **The cloud's watermark.**
   - A replica may name a peer as its **durable** peer: the hub names the cloud, and a
     single-device merchant's device would too. That peer's `have` is the watermark.
   - A new frame, `durable`, `[1, 2, location, [[device, position], ...]]`, relays it:
     - a replica sends it to its other peers when its watermark advances;
     - it sends it at each round, so a lost frame costs a round;
     - it sends it in answer to a `have` that asks, so a device that starts learns it at once.
   - A replica keeps the pointwise maximum of what it hears, so the watermark only rises, and
     reports it (`durable`). A kernel that doesn't know the frame drops it, as any frame it can't
     decode.
   - Pruning below the watermark comes with retention, after step 6.
7. **No new schema version.** The `sequence` projection owns its table, version 1, and a store
   builds it from its events when it opens, as for any new projection; the golden stores keep
   opening as they are. The full check compares a projection's rows for each stream, which may
   now be several.
8. **Out of scope until slice 4:** a hub restored from an older copy of its store. A hub
   settles its own log against whichever peer answers first; with several peers, one that
   holds more of the hub's log may not be first. Until elections, the simulator restores
   devices' stores only. Also later: batching several writes' events into one record, if the
   records prove too many, and records restricted to the elected hub.
9. **Verification**, for slice 2:
   - known answers:
     - the payload pinned byte for byte, checked with Python's `cbor2`, and each way it is
       refused;
     - the store's sequencing: runs in arrival order, excluding its own records, several records
       when there are many runs, and nothing when nothing is new;
     - confirmation by hash, a forked version never confirmed, and the feed;
     - the `durable` frame pinned, relayed and kept at its highest;
   - property tests:
     - the payload against the codec's model, as for every schema;
     - the store's sequencing and confirmation against a model, over events arriving at a hub
       and at another replica in different orders, with interrupted writes and forked logs;
     - the protocol property with one replica sequencing and one durable, checking frame by
       frame that watermarks only rise and never claim what the durable replica doesn't hold;
   - the simulator, with new invariants:
     - each epoch's numbers are gapless, each assigned once;
     - every event but a forked device's other version is confirmed once, and each device's log
       is numbered in order;
     - every replica confirms the same;
     - the hub's log never forks;
     - a device's store-durable events are never lost, and every watermark only rises, never
       claims what the cloud doesn't hold, and reaches everything after healing;
   - planted bugs in `keel-domain`, `keel-store` and `keel-sync`, and coverage probes.

## Consequences

**Positive**

- Confirmations are facts in the log: signed, replicated, auditable, and the same on every
  replica, with no protocol of their own.
- A replica can tell its own copy of a log from the version the hub confirmed, so a fork shows
  as events that never confirm.
- The store gets a single, gapless feed for the cloud and the reports that need one.
- Devices can say what is backed up, in the store and in the cloud, from what their peers
  already tell them.

**Negative**

- A record for every write the hub makes: when events arrive one at a time, the store holds
  about as many records as events, some 300 bytes each. The simulator measures it; batching
  writes into one record can come later.
- A device's events stay provisional while the hub is unreachable, or unsettled after a restart.
- Until slice 4, any enrolled device could write records, and a hub restored from an older copy
  could fork its log.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Numbers in frames, not in the log | They would be lost with the hub, couldn't be verified, and would need their own replication. As events, they are signed and replicate with everything else. |
| One record per event | Twice the events, for nothing: a run names a stretch of a log in one entry. |
| One stream per epoch, holding every record | Each write would fold every record of the epoch again. A stream per record keeps each write's projection work to its own record. |
| Confirming by position alone | A replica holding a forked version of a log would take the hub's numbers as confirming its own events. The hash of each run's last event pins the version. |
| Sequencing in the same write that stores events | It would bind receiving to the hub role, and a hub not yet settled can't sequence anyway. A separate write is simpler, and a crash between the two only delays numbers. |
| The cloud's watermark as events in the hub's log | A record every time the cloud acknowledges something, forever. A watermark is state to relay, not a fact to keep. |

## References

- [ADR-0017](./0017-projections-and-outbox.md), [ADR-0019](./0019-replication-and-deterministic-simulation.md),
  [ADR-0006](./0006-own-sync-protocol.md), [ADR-0013](./0013-event-payloads-and-schema-evolution.md)
- [offline-and-sync.md §3.2, §4, §7, §9](../architecture/offline-and-sync.md)
