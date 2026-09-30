# ADR-0019: Replication and deterministic simulation: anti-entropy over version vectors, folds in HLC order, and a seeded single-process simulator

- **Status:** Accepted (2026-09-30), with the details settled in building slice 1, under "As
  built", accepted after review
- **Date:** 2026-09-30

## Context

Step 6 of Phase 0 is `keel-sim` and `keel-sync` v0: "two devices, one hub and one cloud in
simulation. Replication, hub sequencing, ownership leases, failover. The invariant suite runs in
CI" ([roadmap](../roadmap.md#8-first-engineering-milestones-the-next-build-steps)). The design
already fixes a great deal:

- Every device appends only to its own signed, hash-chained log. Replicas exchange logs by
  version vectors, and the same exchange runs device ↔ hub, hub ↔ standby, hub ↔ cloud and
  device ↔ cloud ([ADR-0002](./0002-event-sourced-signed-event-log.md),
  [ADR-0006](./0006-own-sync-protocol.md),
  [offline-and-sync.md §3](../architecture/offline-and-sync.md#3-replication-protocol)).
- Receivers verify every event, quarantine what fails, keep the first of two events at one
  position, and deduplicate, so replays and duplicates are harmless (§3.2).
- The hub sequences the store's events, grants ownership leases, and fails over to a hot standby
  in under 5 s with epoch fencing (§4, §5, §6.3, §7).
- Sync is simulation-tested: the kernel's clock, network, disk and randomness sit behind
  interfaces, and a simulator runs devices, hubs and a cloud replica in one process under a
  seeded scheduler, injecting faults and checking invariants on every run (§12).

What is built already shapes the rest:

- `keel-store` receives events one at a time and says what became of each: stored, a duplicate,
  a gap (with the position to resend from), or quarantined, with a reason. It reads each device's
  log from any position, and the version vector ([ADR-0016](./0016-device-store.md)). Receiving
  the device's own events from another replica moves its log writer on to them.
- Projections fold each stream in **canonical order, by HLC, then device, then sequence number**,
  so a projection depends only on the set of events its stream holds, whatever order they arrived
  in ([ADR-0017](./0017-projections-and-outbox.md)). The order fold's rules for concurrent
  changes are written for that order: a close that left out a line added concurrently moves the
  line to a new post-close check, for instance
  ([domain-model.md §6.5](../architecture/domain-model.md#65-as-built-order-events-v1)).
- The kernel never reads the clock or a random number generator: time arrives as a `Timestamp`,
  identifiers come from an `Entropy`, and the store's fault hooks can stop a write at any point.

That leaves open how the protocol works in detail, where it lives, how the simulator is built
and what it checks, and how step 6 is split. It also leaves one conflict. offline-and-sync.md §4
defines the canonical order as the hub's order for confirmed events, `(epoch, store_seq)`, then
HLC order for provisional ones; the store folds in HLC order.

## Decision

1. **Two new crates.**
   - `keel-sync`: the replication protocol, and later sequencing, leases and election. It does no
     I/O itself: the caller owns connections, timers and the clock, hands it each message
     received and each tick, and sends the messages it returns. It works through a `Replica`
     trait: its version vector, a device's log after a position, and receiving a batch of events.
     `keel-sync` implements the trait for `keel-store`, so it is a **platform crate** like the
     store, and a model replica in memory lets its property tests run fast.
   - `keel-sim`: the simulator and its invariant suite, for tests only, never shipped. It depends
     on `keel-sync`, `keel-store` and `keel-domain`, and on nothing else outside the kernel.
2. **Folds stay in HLC order; the hub's order confirms.** Every replica folds a stream by HLC,
   device and sequence number, so the same events make the same state everywhere, with or
   without a hub, in island mode and in a split brain. The hub's `store_seq` (slice 2) records
   the order in which the hub received events: it marks events confirmed, gives the store a single
   gapless feed, and orders the hub's own decisions, such as leases. It never moves an event in a
   fold. This amends offline-and-sync.md §4, for these reasons:
   - folding in the hub's order would append an island's events after everything the hub
     sequenced while it was away: an island's stale change (a guest count set at 12:00) would
     override a newer one made in the store (at 12:30) when the island rejoins at 13:00;
   - confirming an event would move it, and change state that staff have already seen;
   - every replica would need the hub's sequencing records before it could fold, and a failover
     that sequences events again would reorder history.

   The cost: a device whose clock runs ahead wins "last writer" conflicts it shouldn't. Ownership
   leases prevent most such conflicts, the hub gives devices its time, and a remote HLC more than
   `max_forward_drift` ahead doesn't advance a device's clock.
3. **The protocol v0: anti-entropy by version vector, over any transport.** It assumes nothing
   of delivery: frames may be lost, duplicated, reordered or delayed, and it still converges.
   - Two frames, each a canonical CBOR array beginning with the protocol version: `have`, a
     replica's location and version vector; and `events`, a batch of signed events as their
     stored bytes, each device's in order. A frame over 1 MiB, or one that doesn't decode, is
     dropped and counted, never fatal.
   - A replica sends `have` to each peer when it starts, after storing a batch (the
     acknowledgement), and every few seconds (a round). On a peer's `have`, it records the peer's
     vector and sends one batch of what the peer lacks: for each device, from the position after
     the peer's.
   - One batch at a time per peer. The peer's next `have` acknowledges it; a batch not
     acknowledged within a timeout is forgotten, and the next batch starts from what the peer
     last said it holds. A batch that made no progress isn't sent again before the next round,
     so an event a peer refuses costs one batch a round, not a loop.
   - New events, appended or received, are pushed at once to every peer whose vector is behind
     and that has no batch waiting. Events received from a peer count as held by that peer, so
     they are never echoed back.
   - Receiving is the store's: a duplicate changes nothing, an event after a gap waits to be sent
     again, and an event that fails verification is quarantined and never acknowledged. The
     replicator keeps its own version vector in memory, from the store's when it starts, and
     moves it on as events are stored, so it never scans the store per frame.
   - A `have` for another location or protocol version is refused. Negotiating versions comes
     with mixed-version fleets.
4. **A device settles its own log before it writes.** A store can't tell when it has lost its
   latest writes ([ADR-0018](./0018-encryption-at-rest-and-integrity-checks.md)), and writing
   again at those positions would fork its log. So after every start, the replicator reports the
   device's own log as settled only once a peer's `have` shows it holds no more of that log than
   the store does, after receiving whatever the peer held. The shell waits for that before
   writing, up to a few seconds: a device that can reach no peer is an island, and must sell.
5. **The simulator.** One process, one thread, one seeded generator, and virtual time:
   - **Nodes:** devices, a hub and a cloud replica, each with a real `keel-store` on a RAM disk,
     its own key and signer, and a clock offset from virtual time. In slice 1 the hub and the
     cloud are replicas that relay, in a star: devices to the hub, the hub to the cloud.
   - **The network** carries frames between linked nodes, each after a random delay, so frames
     arrive out of order; it loses and duplicates frames at configured rates, and partitions
     cut links for a while.
   - **Faults:** partitions; frames lost, duplicated and reordered; clocks offset and jumping
     back and forth within the drift the kernel tolerates; nodes crashing, between writes or in
     the middle of one through the store's fault hooks, and restarting; and a device's store
     restored from an older copy of itself, as a backup or a damaged WAL would.
   - **Workload:** devices ring orders through `keel-domain`'s commands, each decided against the
     device's own view of the order: creating orders, adding, changing, firing, voiding and
     removing lines, opening checks and allocating lines to them, closing and reopening, and
     changing attributes, on their own orders and each other's. A command the device's view
     refuses is skipped, as a register would refuse it.
   - **A run** works for a set time under faults, then heals everything, restarts every node and
     runs until the replicas agree, within a bound. Every action feeds a running hash, the
     run's trace.
6. **The invariants**, checked after every run:
   - **convergence:** every replica holds exactly the events every device committed, device by
     device, and projections byte-identical to those of a fresh store given all of them;
   - **no loss:** every event a device committed is on every replica, unless it was on that
     device alone when its store was restored from an older copy;
   - **causality:** each event's HLC is later than every event its device held when it was made;
   - **no forks and no quarantine:** devices never write two events at one position, and no
     replica quarantines anything. The one exception is a device restored from an older copy
     that wrote before any peer answered, as an island must. Its log forks: each replica keeps
     the version it received first, and quarantines the other when it meets it, never merging
     them. The simulator checks that forks happen only then, and holds every other log to the
     invariants above;
   - **every store checks clean:** each replica's full check finds no problem;
   - **liveness:** after healing, the replicas agree within a bound; and without faults, every
     event reaches every replica within a bound;
   - **determinism:** a seed run twice gives the same trace.

   Later slices add their own: one confirmation per event, gapless per epoch; one lease holder
   per order; one hub per epoch.
7. **Seeds.** A test runs a range of seeds on every commit, each with its faults, workload and
   network drawn from the seed; a soak runs many more. A failing seed prints the command that
   runs it alone, and becomes a named regression test. The protocol's property tests, against the
   model replica, run under proptest like every other property test.
8. **Step 6 comes in four slices, each ending with a review:**
   1. replication and the simulator: decisions 1 to 7;
   2. hub sequencing: `store_seq` per epoch, recorded in the hub's own log as signed events,
      confirmed and provisional events, the cloud's durable-ack watermark, and store durability
      once two replicas hold an event;
   3. ownership leases: the hub grants and transfers orders' ownership; structural and money
      commands need it, commutative ones don't; island mode and the manager's override;
   4. hub election and failover: priorities, heartbeats, the hot standby, epochs and fencing,
      and a split brain healing.
9. **Verification**, for slice 1:
   - known answers: frames pinned byte for byte, batches, the peer's states, and settling a
     device's own log;
   - property tests of the protocol against the model replica: replicas exchanging frames over a
     network that loses, duplicates and reorders them, with partitions, converge to the union of
     their events, and never send a peer what it has acknowledged;
   - the simulator's seeds, with every invariant above;
   - planted bugs in `keel-sync`, caught by the property tests and the simulator alone;
   - coverage probes of the faults and workloads each run meets.

## As built

Slice 1's details, settled in building it, for review with it:

- **Frames.** `have` is `[1, 0, location, [[device, position], ...], acked, asks]`, and `events`
  is `[1, 1, batch, [event, ...]]`, both pinned byte for byte. A `have` lists devices by their 16
  bytes in ascending order, each once, with positions from 1. Decoding refuses anything else, and
  any frame over 1 MiB.
- **Events are at most 256 KiB** (`MAX_EVENT_BYTES`, in `keel-events`). The log writer refuses
  to make a larger one, and the registry refuses to verify one, which the store quarantines as
  `rejected`. A device's log replicates in order, so one event too large to send would hold back
  every later event of its device. With the limit, a batch's first event always fits in a frame.
- **A replica asks for a `have` until it hears from the peer.** Its `have` asks for the peer's
  in return until it has heard from that peer since it started, and the peer answers at once.
  So a replica that starts learns what its peers hold in one round trip, and its device's log
  settles then, not at a peer's next round. This is decision 3's exchange with one more field,
  and the first coverage probe showed why it's needed. Without it, a device restored from an older
  copy waited up to a round (5 s) for its peer's `have`, longer than its settle wait (3 s), and
  wrote as an island: 294 seeds in 1,000 forked a device's log. With it, and with batches that
  carry the peer's own log first (below), 88 do. Each one examined was an island by force: its
  link to the hub was cut through its settle wait.
- **Rounds.** A round comes every 5 s. Until a replica has heard from a peer, it repeats its `have`
  every acknowledgement timeout (2 s), so a `have` lost at startup costs 2 s, within the settle
  wait: seed 13 found a device that forked when its one `have` was lost. A round keeps its own
  clock, and acknowledgements don't count as rounds. Otherwise a peer that keeps sending
  batches, a forked device's refused ones or a busy device's sales, holds off the round that ends
  a stall: seed 162 found a hub stalled towards a device for good that way.
- **Batches.**
  - One batch at a time per peer. Batches are numbered from the replicator's start time, in
    microseconds, so unless the clock went back, no acknowledgement from before a restart
    acknowledges a batch after it.
  - A batch is acknowledged by the next `have` whose `acked` is its number, or taken as lost
    after 2 s.
  - A batch holds up to 256 events and 256 KiB, and always holds its first event, whatever its
    size. It carries the peer's own log first, as much of it as fits, since the peer's device
    can't write until it has its own log back. Then each other lagging device gets an equal share
    of the room left. The probe found this too: a device restored from an older copy got its own
    events back at half speed, sharing each batch with another device's, and forked when a few
    batches were lost.
  - The peer's own log loses its place at the front if the peer took none of it in the last
    batch that carried some. The peer then holds another version of that log, having forked it,
    so its own log takes a share like any other's. Seed 1645 found the need: a forked device's
    own log filled every batch it was sent, it took none of each, and it never got another
    device's events.
  - A batch the peer took none of stalls the replica towards the peer until its next round.
    "None" means the peer's `have` doesn't hold the first event of any device in the batch.
- **What a peer holds** is what it last said it holds, raised by the events it has sent that the
  replica stored. What it said before isn't merged in, since a peer restored from an older copy
  holds less than it did.
- **The replicator's version vector** is the store's when it starts, and moves on as events are
  stored or appended. If what it is told doesn't follow what it knew, something wrote to the
  store behind its back, and it reads the store's again.
- **Ticks.** `next_tick` says when the replicator next needs a tick: its next round or
  acknowledgement timeout. A clock set back counts as time passed, so correcting a clock
  backwards never stops the rounds. A clock set back less than a round delays them by as much.
- **The simulator** runs two devices, or three in 3 seeds in 10, with the hub and the cloud, for
  30 virtual seconds of work, then heals and runs until the replicas agree, within 60 s. A seed
  draws everything else:
  - **Clocks:** each off virtual time by up to 20 s, jumping by up to 10 s, and never more than
    25 s off.
  - **Frames:** delays of 1 ms to up to 120 ms; up to 25% lost and 15% duplicated, none in a
    quarter of seeds; up to three links cut, for 1 to 15 s each.
  - **Crashes:** up to three, half of them in the middle of a write, at one of its fault points,
    down for 0.2 to 8 s.
  - **Rollbacks:** up to two. A device's store is copied between writes; 0.5 to 10 s later
    the device goes down, and comes back 0.5 to 5 s later with the copy, down longer than any
    frame takes.
  - **Work:** a move every 0.1 s to up to 1.5 s, of seventeen kinds. Seven in ten carry a sale
    forward: creating an order, adding lines, firing, taking cash for each check, closing checks
    and the order. The rest are any move on any order: attributes, changes, removals, voids,
    comps, checks and allocations, reopening, voiding and abandoning. A device waits for its log
    to settle for up to 3 s after starting, then works as an island.
  - **Timers:** each node's replicator ticks when `next_tick` asks, on a timer that runs in
    virtual time, as a device's monotonic timer would, whatever its clock does.
- **Rules checked as the run goes.** Every batch a replica sends is checked against a model of
  what its replicator knows of the peer:
  - no batch while another waits, unless its timeout has passed;
  - no batch to a peer that took none of the last one, until a round;
  - a tick a round or more after the stall began is that round.

  The protocol's property test, whose replicas at times decline a batch so that its sender
  stalls, checks every frame against the same rules and more:
  - no event the peer said it holds, or sent;
  - batch sizes;
  - fresh batch numbers, even across restarts;
  - acknowledgements and answers at once;
  - `asks` exactly until the replica has heard from the peer;
  - no tick doing anything before `next_tick` said.

  A replica that sends a peer the batch it refused again as soon as the peer acknowledges it
  meets every final invariant, so only rules checked as the run goes can catch it.
- **A forked device** is one that appended while its log wasn't settled, after a rollback. Its
  log is left out of convergence, no loss and causality, and the streams it wrote to out of the
  projections' comparison. The other replicas may quarantine its events only as a fork or a
  broken link.
- **Costs:** a seed takes about 0.4 s in a release build and 0.75 s in a test build. The seed
  test runs 32 seeds, and CI runs it twice, on different seeds. The protocol property's 4,096
  cases take about a minute.

## Consequences

**Positive**

- Replication is one small mechanism, the same between any two replicas, and correct over any
  transport: a lost, duplicated or reordered frame costs time, never data.
- Projections depend only on the events a replica holds, so the simulator can check convergence
  exactly, and a device folds the same with or without a hub.
- Sync bugs meet their failing seed before they meet a merchant, and every seed replays exactly.
- The simulator runs the real store, so the invariants cover the store's receiving, folding and
  crash recovery together with the protocol.

**Negative**

- The simulator runs the real store, with SQLCipher, so a seed takes about a second: tens of
  seeds per commit and thousands in a soak, not the millions offline-and-sync §12 hopes for
  nightly. The model replica makes the protocol's own property tests fast.
- Disk faults the store can't show without a simulated disk, torn writes and lost fsyncs, wait
  for one: a SQLite VFS, which needs `unsafe` code.
- HLC order lets a clock that runs ahead win conflicts; see decision 2.
- A peer that keeps sending events the replica refuses costs one batch per round per peer.
- A device restored from an older copy that must sell before any peer answers forks its log.
  The replicas report the fork, and keep disagreeing about that log until a person resolves it,
  with tools that don't exist yet.
- The protocol has no priority lanes, compression or reconnection backoff yet: payments first
  and storms of reconnecting fleets come with the transport.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Folding confirmed events in the hub's order, as offline-and-sync §4 had it | An island's stale changes would override newer ones, confirmations would change state staff had seen, and a failover that sequences again would reorder history. See decision 2. |
| Relying on the transport's ordered, reliable delivery | WebSocket over TCP delivers in order within a connection, but connections drop and reconnect. A protocol correct over any delivery can be tested against every fault, and needs no special case for reconnection. |
| Exchanging Merkle trees or ranges of hashes instead of version vectors | Each log is gapless and delivered in order, so one number per device says exactly what a replica holds. |
| An existing simulation framework (`turmoil`, `madsim`) | They simulate async runtimes and networks for code written against them. `keel-sync` has no I/O and no async, so a plain scheduler does the job with no dependency, and every choice it makes is in the seed. |
| Simulating against in-memory replicas only | Fast, but it would test a model of the store, not the store. The model replica tests the protocol; the simulator tests the system. |
| Testing sync with real processes and networks (Jepsen-style) | Not deterministic: a failure found once may never recur. Kept for the shells' integration tests later. |

## References

- [ADR-0002](./0002-event-sourced-signed-event-log.md), [ADR-0006](./0006-own-sync-protocol.md),
  [ADR-0016](./0016-device-store.md), [ADR-0017](./0017-projections-and-outbox.md),
  [ADR-0018](./0018-encryption-at-rest-and-integrity-checks.md)
- [offline-and-sync.md §3–§7, §12](../architecture/offline-and-sync.md)
- FoundationDB's and TigerBeetle's deterministic simulation testing, which §12 follows
