# ADR-0020: Hub sequencing: signed sequencing records in the hub's log, confirmation by hash, and durability watermarks

- **Status:** Accepted (2026-10-01), with the details settled in building it, under "As built",
  accepted after review
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

## As built

Details settled in building it, for review with it:

- **The record** is as decision 1 describes, pinned byte for byte in two payloads that Python's
  `cbor2` encoded from the key table. Every rule is judged in one place, when a record is made
  and when one is decoded, so both name the same rule: `epoch`, `first`, `runs` (none, or more
  than 1,024), `run` (a position outside 1 to 2^63 − 1, or a run ending before it starts),
  `runs of one device side by side`, `runs that don't follow on`, and `last number`. A first
  number past the largest takes the last number past it too, so those two rules refuse the same
  records.
- **What the hub numbers:** for each device, the events after the highest position any record
  the hub holds covers, except records, in the order the hub received them.
  - This holds if the hub's own records come back from its peers after a restore, which "after
    the arrival of the hub's latest record" wouldn't.
  - The hub's own log is read only after its latest record, which numbered everything in its log
    before it. Each run's row keeps its record's position in its author's log for that, since
    records, which nothing numbers, would otherwise be read again at every write.
  - A stretch below a device's highest covered position that no record covers is never
    numbered. Only a record written by another device could leave one; slice 4 restricts
    records to the elected hub.
  - Within one record, each device's runs follow on: a record ends where a device's next event
    doesn't follow on (around a record in that device's own log), and at 1,024 runs.
  - A record's stream identifier is drawn from the store's own entropy through its log writer
    (`LogWriter::generate_id`, new in `keel-events`), so simulated runs stay deterministic. Its
    business date is the latest among the events it numbers.
  - `Store::sequence` checks the epoch first. Numbers start at 1 in each epoch and follow on
    from the store's own last record of the epoch; another device's records in the epoch don't
    move them.
- **Confirmation** is worked out as the store is read, never kept, since it depends on the
  events the store holds. The projection keeps a row per run, never per event: a record may
  claim a run of any length, and a row per event would let one record fill every replica's
  disk. Each run's length is indexed, so a search for the runs covering an event looks no
  further than its device's longest run.
  - `store_seq`: the first number, by epoch and number, among the confirming runs covering the
    event.
  - `confirmed()`: for each device, the longest start of its log in which every event is
    confirmed or a record. Records aren't numbered, and count as confirmed.
  - `sequenced(epoch, after, limit)`: the confirming runs of the epoch in number order, each
    event under its first number only.
  - A record's stream holds one event. The projection reads its first event, in canonical order;
    a record this kernel can't read numbers nothing.
- **Projections and the check:** a projection names the columns that order a stream's rows, and
  the full check compares every row of a stream in that order.
- **`keel-sync`:**
  - A replicator starts with its roles, `Roles { sequencer, durable }`.
  - The hub numbers after every write that stores events it received, after the device's own
    writes, and when its log settles; never before. The records go out like any new events.
    `Replica::sequence` is the store's `sequence`.
  - `store_durable()` is the most any peer has said it holds of the device's own log.
  - The watermark is the durable peer's `have`, raised device by device, never lowered. When it
    rises, a replica passes it to every peer but the one it came from; it also sends it with
    every round and in answer to a `have` that asks. It never sends it to its own durable peer.
  - Devices send the watermark back to the hub with each round, 6% of all `durable` frames:
    redundant, and harmless.
- **`keel-sim`:** the hub sequences in epoch 1 and names the cloud its durable peer.
  - Checked as a run goes: every `durable` frame against what the cloud holds, and that every
    replicator's watermark only rises.
  - Checked at the end:
    - every event but the records is numbered once, gapless, and each device's log in order;
    - every replica confirms each event as the hub numbered it, a forked device's aside, and
      holds each log confirmed to its end;
    - each replica's feed gives its events in number order;
    - no event a device was told was store-durable was lost to a rollback.
  - The replicas agree only once every watermark but the cloud's reaches what the cloud holds.
  - The first durability check marked positions, and seed 1 showed why that was wrong: a device
    rolled back writes its lost positions again, and a later mark made an old, lost event look
    store-durable. Marks now go to the event the device held at each position when it was told.
- **Verification**, beyond decision 9: in the store's property test, the other replica, once it
  holds everything, then sequences in the next epoch, and the hub takes in its records and the
  rest of the devices' logs. That checks what a sequencer numbers where another device's records
  cover part of each log, and confirmation across epochs and versions of a log. It was added
  when a planted bug that numbers records went unnoticed: a hub on its own never reads its own
  records again.
- **Costs**, measured on the development machine in a release build, on a RAM disk. A hub took
  two devices' events ten at a time, 20,000 in all, and sequenced after each write:

  | Operation | Cost |
  |---|---|
  | Sequencing a write's ten events: the first hundred writes | 0.9 ms |
  | The same, the last hundred, at 20,000 events | 1.6 ms |
  | `store_seq` of one event | 0.3 ms |
  | A page of 256 events from the middle of the feed | 2.5 ms |
  | `confirmed()`, with 2,000 runs | 37 ms |
  | The full check of that store | 0.75 s |

  - The first version of sequencing read every event, and the hub's records again, at every
    write: 37 ms a write at 20,000 events. It now reads only each device's events after its last
    covered position.
  - `store_seq` and the feed spend most of their time preparing statements, as every read of
    the store does, so a statement cache would help them all. `confirmed()` reads every run;
    the UI will need a confirmed start kept for each device instead.
  - Over 5,000 simulator seeds, the hub wrote 367,065 records, numbering 597,945 events: 1.6
    events a record, as expected when events arrive a few at a time.

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
