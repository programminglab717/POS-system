# Build progress

> A living record of the build: where it stands, what each step delivered and how it was
> verified, and what is waiting on a decision. Updated as each piece of work lands.
> Last updated: 2026-09-30.

## Where we are

**Phase 0 (Foundations), step 6 of 8:** `keel-sim` and `keel-sync` v0, the deterministic
simulator and the sync engine, in four slices
([ADR-0019](./adr/0019-replication-and-deterministic-simulation.md)). Slice 1, replication and
the simulator, is built and reviewed. Slice 2, hub sequencing, is built and verified, and
awaits review ([ADR-0020](./adr/0020-hub-sequencing.md), proposed). Step 5, `keel-store`, is
done: built in three slices, each reviewed.

## Phase 0 milestones

From the [roadmap](./roadmap.md#8-first-engineering-milestones-the-next-build-steps).

| # | Milestone | Status | Where |
|---|---|---|---|
| 1 | Monorepo scaffold | Rust workspace and CI done. The Android, web and schema directories arrive with their first code. | [`Cargo.toml`](../Cargo.toml), [CI](../.github/workflows/ci.yml) |
| 2 | `keel-types`: value types | Done, 2026-09-27 | [`core/crates/keel-types`](../core/crates/keel-types/) |
| 3 | `keel-events`: the signed, hash-chained event log | Done, 2026-09-27. Its schema registry was built with the first domain events, in `keel-domain`. | [`core/crates/keel-events`](../core/crates/keel-events/), [ADR-0012](./adr/0012-event-wire-format.md) |
| 4 | `keel-domain` (order, check, payment) and `keel-pricing` v0 | Done, 2026-09-29: built in four slices, each reviewed | [`core/crates/keel-domain`](../core/crates/keel-domain/), [`core/crates/keel-pricing`](../core/crates/keel-pricing/), [ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md), [ADR-0014](./adr/0014-pricing-engine-v0.md), [ADR-0015](./adr/0015-checks-and-payments.md) |
| 5 | `keel-store`: SQLite events, projections and outbox | Done, 2026-09-30: built in three slices, each reviewed | [`core/crates/keel-store`](../core/crates/keel-store/), [ADR-0016](./adr/0016-device-store.md), [ADR-0017](./adr/0017-projections-and-outbox.md), [ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md) |
| 6 | `keel-sim` and `keel-sync` v0 | In progress: slice 1 of 4 built and reviewed; slice 2 built, awaiting review | [`core/crates/keel-sync`](../core/crates/keel-sync/), [`core/crates/keel-sim`](../core/crates/keel-sim/), [ADR-0019](./adr/0019-replication-and-deterministic-simulation.md) |
| 7 | Android register shell | Not started | |
| 8 | Cloud cell v0 | Not started | |

Step 4 was built in four slices, each ending with a review. It was planned as three; the third,
checks and payments, was split in two, so that the payment design was reviewed before it was
built:

1. **Foundations and the order's lines** (built and reviewed 2026-09-28): payload codecs
   and schema registry; the aggregate framework; order events, fold and commands for creating an
   order, adding, changing, removing, firing, voiding and comping lines, changing attributes, and
   voiding or abandoning the order.
2. **Pricing v0** (built and reviewed 2026-09-28): modifier pricing, discounts, US sales tax,
   rounding and allocation, the calculation trace, and the golden-basket suite.
3. **Checks and splits** (built and reviewed 2026-09-28): every order's main check,
   opening checks, allocating lines to checks in whole shares, and pricing each check as its own
   sale.
4. **Payments and closing** (built 2026-09-28, reviewed 2026-09-29): the payment aggregate,
   balances, closing checks with their totals, and closing and reopening orders.

Step 5 was built in three slices, each ending with a review
([ADR-0016](./adr/0016-device-store.md), decision 8):

1. **The event log store** (built and reviewed 2026-09-29): the device's own events and
   those it receives from other replicas, in one SQLite database; the quarantine and the version
   vector; one transaction per write; crash tests at every point of a write.
2. **Projections and the outbox** (built and reviewed 2026-09-29): order and payment
   projections recomputed in each write's transaction, rebuilt from the log when their version
   changes; and an outbox of effects that commit with the events that cause them.
3. **Encryption at rest and integrity checks** (built and reviewed 2026-09-30): SQLCipher
   encryption with a key the platform protects, a store that fails closed when damaged, cheap
   checks when it opens, and a full check on request.

Retention, snapshots and backups follow the sync engine (step 6).

Step 6 is built in four slices, each ending with a review
([ADR-0019](./adr/0019-replication-and-deterministic-simulation.md), decision 8):

1. **Replication and the simulator** (built and reviewed 2026-09-30): `keel-sync`'s protocol v0, anti-entropy
   by version vector over any transport, and `keel-sim`, devices, the hub and the cloud with
   real stores under seeded faults, checking the protocol's rules and the invariants.
2. **Hub sequencing** (built 2026-09-30, [ADR-0020](./adr/0020-hub-sequencing.md)):
   `store_seq` per epoch, confirmed and provisional events, store durability, and the cloud's
   durable-ack watermark.
3. **Ownership leases:** the hub grants and transfers orders' ownership; island mode and the
   manager's override.
4. **Hub election and failover:** priorities, heartbeats, the hot standby, epochs and fencing,
   and a split brain healing.

## Current slice

Step 6, slice 2, hub sequencing, is built and verified (see "Completed"), as
[ADR-0020](./adr/0020-hub-sequencing.md), proposed, describes:

- The hub numbers the store's events in the order it receives them, gapless in its epoch, in
  signed `sequence.assigned` records in its own log. Each record names stretches of devices'
  logs, each pinned by the hash of its last event.
- Every replica works out from the records it holds which events are confirmed, and which
  provisional: a replica holding a forked version of a log never takes the hub's numbers for
  its own. The store keeps the numbers in a projection, and gives a gapless feed.
- The hub sequences only once its own log is settled.
- A device knows its events are store-durable once the hub says it holds them, and learns the
  cloud's durable-ack watermark from a new `durable` frame the hub relays.

| Piece | Status |
|---|---|
| Design, in ADR-0020 | Done |
| `keel-domain`: the `sequence.assigned` payload | Done |
| `keel-store`: sequencing, the `sequence` projection, confirmation and the feed | Done |
| `keel-sync`: the hub's sequencing, store durability, and the `durable` frame | Done |
| `keel-sim`: the hub sequences, the cloud is durable, and the new invariants | Done |
| Known answers, property tests, the simulator's seeds, planted bugs and probes | Done |
| Soak, CI-equivalent run, docs | Done |
| Review, and accepting ADR-0020 | Next |

## Completed

### Hub sequencing: step 6, slice 2 (2026-09-30)

Built and verified; awaiting review. Its commit and CI run are recorded once it is pushed.

- **Built** ([ADR-0020](./adr/0020-hub-sequencing.md)):
  - `keel-domain`: `sequence.assigned` v1, the hub's sequencing record. It holds an epoch, the
    number of its first event, and 1 to 1,024 runs, each a device, a stretch of its log, and the
    hash of the stretch's last event. Every rule is judged in one place, for records made and
    records decoded alike.
  - `keel-events`: `LogWriter::generate_id`, a new aggregate's identifier drawn from the log
    writer's entropy, so that simulated runs stay deterministic.
  - `keel-store`:
    - `Store::sequence`: for each device, the store numbers the events after the last position
      any record covers, records aside, in the order it received them, from where its own last
      record of the epoch left off. It writes several records where a device's runs wouldn't
      follow on, or a record is full.
    - The `sequence` projection, a row per run, and confirmation by hash: `confirmed()`,
      `store_seq()`, and `sequenced()`, the feed. Indexes on each run's length bound every
      search.
    - The full check compares every row of a stream, now that a stream may have several.
  - `keel-sync`:
    - A replicator's roles: the Store Hub numbers after every write that stores events it
      received, after its own appends, and when its log settles; never before.
    - `store_durable()`: how far into the device's own log a peer has said it holds.
    - The `durable` frame, and the watermark: the durable peer's `have`, kept at its highest and
      relayed to the other peers.
  - `keel-sim`: the hub sequences in epoch 1 and names the cloud its durable peer, and the
    simulator checks the new invariants, some as a run goes and the rest at its end.
- **Verified:**
  - Known answers:
    - `keel-domain`, 9 tests: the payload pinned byte for byte in two payloads Python's `cbor2`
      encoded; each rule a record is refused by, made or decoded; strict reading; the registry.
    - `keel-events`: identifiers drawn between events are fresh and in order.
    - `keel-store`, 11 tests:
      - numbering in the order received, and records ending where a device doesn't follow on
        or a record is full;
      - confirmation by hash, a forked version never confirmed, and the feed;
      - records that disagree, where an event's first number counts;
      - the confirmed start of a log;
      - an interrupted sequencing leaving nothing, and each epoch numbering from 1;
      - the check finding a run that isn't what its record says.
    - `keel-sync`: the `durable` frame pinned byte for byte, and 5 ways it is refused. Six
      replicator tests, frame by frame against the model replica:
      - the hub numbering what it stores, once its log is settled, and a replica that isn't the
        hub numbering nothing;
      - store durability, as far as a peer holds the device's log;
      - the durable peer's `have` as the watermark, and a `durable` frame raising it, each
        passed on;
      - the watermark with every round, and in answer to a `have` that asks.
  - Property tests:
    - `keel-domain`: records against the codec's model, as for every schema; spoiled records
      refused exactly when the model refuses them; the numbers a record gives, counted run by
      run; and records around the 1,024-run limit.
    - `keel-store`, against a model:
      - a hub takes in two devices' logs and appends events of its own, sequencing now and
        then, sometimes interrupted;
      - another replica takes in the hub's log and the devices' logs in any interleaving, one of
        them perhaps forked;
      - that replica then sequences in the next epoch, and the hub takes in its records.

      After every step, each store's numbers, confirmed starts, and the feed of each epoch and a
      page of it, must be the model's.
    - `keel-sync`: the protocol property, with one replica the hub and one durable. All along,
      every `durable` frame claims no more than the durable replica holds and never goes to it,
      and every watermark only rises. At the end, every replica's watermark reaches what the
      durable replica holds.
    - 100,000 cases each passed, in release builds: `keel-domain`'s 18 payload properties in
      80 s, the protocol property in 13 minutes, and the store's in 54.
  - The simulator: 5,000 seeds passed in a release build, in 43 minutes, alongside other runs.
    - The hub wrote 367,065 records, numbering 597,945 events, and replicas sent 961,128
      `durable` frames.
    - The runs met what slice 1's did: 613,260 events appended; 4.6 million frames, of which
      240,000 were lost, 256,000 duplicated, 116,000 cut off and 64,000 sent to a node that was
      down; 3,524 crashes between writes, 3,410 in the middle of one, 3,627 rollbacks, 7,526
      clock jumps and 7,575 cuts; 5,317 events lost to rollbacks, held by no other replica, and
      535 devices forked.
    - The replicas agreed after healing within 1.4 s at the median, 2.1 s at the 90th
      percentile, 5.1 s at the 99th, and 10.9 s at most, now that agreeing includes every
      watermark reaching what the cloud holds.
  - A coverage probe over 1,000 seeds, all passing:
    - the hub wrote 72,983 records, numbering 118,617 events, 1.6 a record;
    - 57,239 runs reached a device before the events they cover, in 998 seeds;
    - the hub was interrupted in the middle of a replicator's write 166 times, in 158 seeds;
    - devices were told 118,788 events were store-durable. Rollbacks then took 5,443 of them
      from their devices, in 525 seeds, and none was lost: the hub gave them back;
    - 1,653 of forked devices' own events stayed unconfirmed where the devices held them, in
      87 seeds, as they should;
    - of 194,241 `durable` frames, 12,326 (6%) went back to the hub from its devices, and none
      to the cloud.
  - Planted bugs, all caught: 22 in `keel-domain`, 27 in `keel-store`, 14 in `keel-sync`, and 1
    in `keel-events`, caught by its unit test.
    - The property tests alone catch 46 of the 63 in the first three: `keel-domain`'s 20,
      `keel-store`'s 17, and `keel-sync`'s 9. The protocol property catches all 9 of those, and
      the simulator 5 of them.
    - Only the unit tests reach the other 17:
      - 6 need records that only a second sequencer or a misbehaving device writes: two
        devices' records in one epoch, records that disagree about an event, or a device that
        isn't the hub writing a record between its own events;
      - 2 are refused by another rule as well, and only the error's rule name tells them apart;
      - a record of 1,024 runs, an epoch out of range, a record's business date, and a
        projection row changed behind the store's back;
      - the hub numbering before its log is settled, which can't fork the numbers while the
        simulator never rolls the hub back;
      - store durability, where the simulator's devices each have one peer;
      - a watermark held back until the next round, twice;
      - a watermark from another location, where every replica is at one.
  - A CI-equivalent run passed every step, in 28 minutes: formatting, both lint runs, the tests
    with the exhaustive sweeps and 4,096 cases a property, the tests without default features,
    the docs, the `wasm32` build, the currency table and the golden baskets.
- **Decisions:** [ADR-0020](./adr/0020-hub-sequencing.md), proposed, with what the build
  settled under "As built":
  - what the hub numbers, for each device: the events after the last position any record
    covers, which holds even when the hub's own records come back from its peers;
  - a record ends where a device's runs wouldn't follow on;
  - confirmation worked out as the store is read, from a row per run, never per event;
  - where records disagree, an event's first number counts;
  - the watermark relayed on every rise, with every round, and in answer to a `have` that asks.
- **Found and fixed during the build:**
  - Seed 1: the simulator marked store-durable events by position. A device rolled back wrote
    its lost positions again, and a later mark made an old, lost event look store-durable. Marks
    now go to the event the device held at each position when it was told.
  - The first sequencing read every event, and the hub's own records again, at every write:
    37 ms a write at 20,000 events. It now reads only each device's events after its last
    covered position, and the hub's own log after its latest record: 1.6 ms.
  - The first run of the planted bugs found `keel-domain`'s test records too regular: the
    property tests missed 7 of 22 bugs, and the unit tests one, a decoder keeping only the first
    byte of each run's hash, since every hash they used was one byte repeated. Records now have
    up to three devices taking turns, runs near the largest position as well as the first, and
    hashes whose bytes all differ; spoiled runs reach each end of the range; and one property
    builds records around the 1,024-run limit, which random records almost never reach.
  - After the store's queries were rewritten for speed, a planted bug that numbers records went
    unnoticed: a hub on its own never reads its own records again. The store's property now
    has another replica sequence in the next epoch, reading the hub's records. That also leaves
    gaps in what a replica holding a forked log has confirmed, so the property now catches a
    confirmed start that passes a gap, which only a unit test caught before.
  - The CI failure that the SQLCipher race caused, below.

### A race opening a process's first stores (2026-09-30)

Found when CI failed on `28aed7a`, the commit accepting ADR-0019. Fixed in commit `0118401`, with
CI green on it (19.8 minutes), on ADR-0019's acceptance and ADR-0020's design with it.

- **What failed:** 1 ms into the `keel-store` unit tests, as their threads opened their first
  stores at once, one store didn't open. SQLCipher refused its key: "An error occurred with
  PRAGMA key or rekey". The same code had passed CI twice.
- **Why:** SQLite runs SQLCipher's initialization only after it has marked itself initialized
  and let other threads on. A connection keyed on another thread meanwhile finds SQLCipher not
  ready, and its key is refused. SQLCipher, up to 4.19, still initializes this way.
- **Fixed:** the store opens the process's first connection alone, under a `Once`, before every
  connection it opens (`keel_store::init_sqlite`); other threads wait for it. The tests' own
  connections call it too, and so must any code that opens SQLite connections in a process with
  stores. ADR-0018's "As built" records it.
- **Verified:** a new test opens 32 stores at once on 32 threads, as its process's first use of
  SQLite. Run 2,000 times, four processes at a time, alternating with the same test built
  without the fix: without it, 44 runs failed; with it, none. Of 1,000 more runs without it, 19
  failed, each with SQLCipher refusing the key of 1 to 23 of the 32 stores, as in CI. The race
  needs the thread initializing SQLite to stop within a few instructions, so no test can force
  it: the test catches it only as often as it happens, about 1 run in 50 on a loaded machine and
  1 in 200 on an idle one. So no planted bug stands for it: the fix closes it by construction.

### Replication and the simulator: step 6, slice 1 (2026-09-30)

Commit `c5240e1`. Reviewed 2026-09-30.

- **Built** ([ADR-0019](./adr/0019-replication-and-deterministic-simulation.md)):
  - `keel-sync`, replication v0: anti-entropy by version vector, correct over any transport,
    with no I/O of its own. The caller hands it each frame received and a tick when
    `next_tick` asks, and sends the frames it returns.
  - Two frames, `have` and `events`, in canonical CBOR. A replica tells each peer what it holds
    when it starts, after each batch it receives, and every round.
  - It sends each peer one batch at a time of what the peer lacks, the peer's own log first
    unless the peer refused it. It pushes new events at once, never echoes them back, and after a
    batch the peer took none of, waits for its next round.
  - Until it hears from a peer, its `have` asks for the peer's in return, which comes at once.
    So a device settles its own log, holding what its peers hold of it, before it writes.
  - It works through a `Replica` trait, implemented for `keel-store`; a model replica in memory
    serves the property tests.
  - `keel-sim`, the deterministic simulator: two or three devices, the hub and the cloud, each
    with a real encrypted store on a RAM disk, in one thread and virtual time, with every choice
    drawn from one seed.
    - Faults: frames lost, duplicated and reordered; partitions; crashes between writes and in
      the middle of one; stores restored from older copies; clocks offset and jumping.
    - Workload: devices ring and settle orders through `keel-domain`'s commands.
    - Checks: each batch against the protocol's rules as it is sent. After healing, the
      replicas must agree within a minute; then convergence, no loss, causality, forks and
      quarantine, and every store's full check.
    - A failing seed prints the command that replays it, and `KEEL_SIM_LOG` prints every
      action.
  - `keel-events`: an event is at most 256 KiB. The log writer won't make a larger one, and the
    registry refuses one, which the store quarantines.
  - The planted-bug runner can run other crates' tests with a crate's, and leave its known
    answers out of `--props`.
- **Verified:**
  - Known-answer tests:
    - frames pinned byte for byte, and 22 ways a frame is refused;
    - 17 replicator tests, frame by frame against the model replica: starting, asking and
      answering, batches and their acknowledgement, shares and the peer's own log first, stalls
      and the rounds that end them, timeouts, echoes, settling, a replicator out of step with
      its store, a clock set back, and the regressions of seeds 162 and 1645;
    - two against real stores, one interrupted in the middle of a write;
    - the event size limit, at the writer and the registry, to the byte.
  - The protocol property. Cases: 2 to 4 replicas as a star, a line or a mesh, over a network
    that delays every frame, loses, duplicates and cuts, with restarts, replicas declining
    batches, and batches of 1 to 6 events, some with too few bytes for two.
    - At the end, every replica holds exactly the events appended, has refused none, and has
      its log settled.
    - All along, every frame keeps the protocol's rules: no event the peer has said it holds
      or sent; batch sizes; one batch at a time; nothing more to a peer that took none of the
      last batch until the next round, which comes when due; fresh batch numbers, even across
      restarts; acknowledgements and answers at once; asking exactly until the peer is heard
      from; no tick that does anything before `next_tick` said.
    - 100,000 cases passed.
  - The simulator: 5,000 seeds passed in a release build, in 34 minutes. What they met:
    - 612,799 events appended, by 17 kinds of move;
    - 2.4 million frames, of which 161,000 were lost, 136,000 duplicated, 54,000 cut off and
      25,000 sent to a node that was down;
    - 3,525 crashes between writes, 3,408 in the middle of a write (in 2,504 seeds), 3,629
      rollbacks (in 2,880 seeds), 7,526 clock jumps and 7,575 cuts;
    - 13,593 events received after a gap, 177,201 duplicates, 119,518 timeouts, and 1,078
      stalls, in 497 seeds;
    - 5,112 events lost to rollbacks, held by no other replica, in 1,056 seeds; 528 devices
      forked, in 509 seeds; 24,961 writes held back until a log settled;
    - agreement after healing within 1.2 s at the median, 2.0 s at the 90th percentile, 5.0 s
      at the 99th, and 8.5 s at most.
  - Planted bugs: 50 in `keel-sync`, all caught, and 4 in `keel-events` at the size limit,
    caught by its unit tests.
    - The property tests alone catch 33 of `keel-sync`'s: the protocol property 29, 10 of them
      alone, and the simulator 23, 4 of them alone. Those 4 need a real store or a rollback:
      forgetting that a restored peer holds less, a log settled from the start or when a peer
      holds more, and the store adapter skipping an event.
    - Only the unit tests reach the other 17:
      - 8 frames or inputs that no replica or test network makes;
      - 3 bugs of a replicator out of step with its store;
      - 2 in how a batch is shared out, which is fairness, not correctness;
      - the pace of `have` repeats before a peer is heard from;
      - a clock set back further than the simulator's clocks go;
      - a batch half stored;
      - seed 1645's bug, which the simulator meets in about 1 seed in 1,000.
  - CI green.
- **Decisions:** [ADR-0019](./adr/0019-replication-and-deterministic-simulation.md), accepted
  2026-09-30 after review, with the details the build settled under "As built". Among them:
  - the fold order: every replica folds in HLC order, and the hub's order will confirm, never
    reorder, amending offline-and-sync §4;
  - a `have` asks for one in return until its sender has heard from the peer;
  - batches carry the peer's own log first, unless the peer refused it;
  - rounds keep their own clock;
  - events are at most 256 KiB;
  - the simulator's faults, its workload, and the rules it checks as runs go.
- **Found and fixed during the build:**
  - Seed 13: a device restored from an older copy, whose one `have` at startup was lost, waited
    a round (5 s) for its peer, longer than its settle wait (3 s), and forked its log. A replica
    now repeats its `have` every 2 s until it hears from a peer.
  - Seed 162: a hub stalled towards a device for good. The device kept sending batches the hub
    refused, and each acknowledgement counted as the round that ends a stall, so the round never
    came. Rounds now keep their own clock, and a named regression test pins it.
  - The first coverage probe showed 294 seeds in 1,000 forking a device's log. A replica that
    started learned what its peers held only at their next round: now its `have` asks for
    theirs, which cuts forks to 98. Seed 17 then showed a restored device getting its own
    events back at half speed, sharing each batch with another device's: batches now carry the
    peer's own log first, and 88 seeds fork. The three examined were islands by force, their
    links to the hub cut through the settle wait.
  - Seed 1645, in the soak: putting a peer's own log first starved a device that had forked
    its log. The hub's version of that log filled every batch, the device took none of each,
    and it never got another device's events. A peer's own log that it refused now takes a
    share like any other's, and a named regression test pins it.
  - A replica that sends a peer its refused batch again as soon as the peer acknowledges it met
    every final invariant. So the simulator now checks each batch against the protocol's rules
    as it is sent, and the protocol property checks every frame.
  - The stall rules were met only in the simulator, whose 32 seeds missed seed 162's bug: it
    fails 12 seeds in 500, the first of them seed 84. The protocol property now has replicas
    decline batches, so senders stall, and checks the stall rules too; it catches the bug, and
    every other planted stall bug.
  - A planted bug that took any later position of a device's log for its next went unnoticed:
    the known answer for a replicator out of step with its store had only one device out of
    step. It now has two.
  - The store adapter storing each event of a batch in a write of its own went unnoticed too:
    the known answer interrupted the write at the first event, where one write and many roll
    back alike. It now interrupts at the second.
  - `next_tick` was tested only by known answers. The simulator now ticks each node when it
    asks, and the protocol property checks that no tick does anything sooner.

### Encryption at rest and integrity checks: step 5, slice 3 (2026-09-30)

Commit `fcef53f`. Reviewed 2026-09-30.

- **Built** ([ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md)):
  - encryption at rest: SQLCipher 4.14, compiled in with OpenSSL 3.6 built from source, the same
    on every platform. Every page, the WAL's too, is encrypted with AES-256 and authenticated
    with HMAC-SHA512, with SQLCipher 4's settings pinned, and temporary storage stays in memory.
    The store opens only with its key: 32 bytes the platform keeps safe, zeroed when dropped;
  - changing the key, in one transaction, proved by opening the file again: SQLCipher's rekey
    reports success when it fails, so the store never trusts it;
  - a damaged store fails closed: a page that fails its authentication, or a file SQLite finds
    malformed, makes the store refuse everything until it is reopened, and it never returns
    what it read there;
  - checks: opening checks what costs the same whatever the store holds, and says whether the
    store was closed cleanly last time; a full check, for when the device is idle, reports every
    problem with the file's pages, SQLite's structure, the events and each device's log, the
    store's identity and clock, the projections against a rebuild, the outbox and the
    quarantine;
  - fault points for rekeys. The error for a signer that didn't sign the device's events is now
    `WrongSigner`, beside the errors about the store's key.
- **Verified:**
  - Known-answer tests: 20 more in `keel-store` (53 in all). Among them: nothing in the clear in
    the database or its WAL; only the store's key opens it, and neither an unencrypted database
    nor a file of garbage opens; the pinned settings; rekeys, interrupted and failed ones
    included; a journal left behind; a damaged page failing the store closed, and one that
    reads as zeros; a damaged first page; files cut short and added to; each change behind the
    store's back the check finds, an event from another location, and a forged event whose
    clock runs backward; a malformed database, which stops the check; a check another reader
    holds up.
  - The golden store: made once with a fixed key, kept in the repository, and read back as it
    was made, against pinned heads and a store stocked afresh.
  - Property tests against models:
    - keys: only the store's key opens it, through writes, rekeys, interrupted and refused
      rekeys, and reopenings with four keys, and it holds what was written;
    - damage to the file: bits flipped anywhere, bytes added or cut. Every read, and every
      write that loads an aggregate, gives what the sound store gave or reports the damage, and
      after it, so does every read and write; a damaged first page refuses the key; the check
      reports exactly the damaged pages;
    - changes behind the store's back, with the key: events changed, refiled, deleted or
      forged, streams dropped, projection rows changed, added or deleted, effects' causes and
      attempts, the clock, quarantined digests and the store's identity. The check reports
      exactly the problems a model predicts, and the same again.
  - Every earlier test now runs on encrypted stores. The crash tests change the key halfway
    through their workload. After every crash, exactly one key opens the store, it knows it
    wasn't closed cleanly, and its full check finds nothing. Kills spread over a rekey of about
    4 ms left the old key in 3 cases and the new one in 13.
  - Coverage probes, over 1,000 cases:
    - damage to the file: none in 157, bits flipped in 765, bytes added in 164 and cut in 159
      (a whole page in 81); several pages in 603, overflow pages in 265, the last page of a long
      value, which reads as zeros, in 107, and the first page in 52. The key refused in 52, the
      damage met by opening in 439, by a read in 250 (a write, first, in 43), and by the check
      alone in 102;
    - changes: every kind of problem, in 79 to 396 cases each; several problems at once in 456;
      a forgery breaking the link after it in 108; a changed row in a stream with an unreadable
      event, which can't be judged, in 23; a stream that lost some of its events in 153, and all
      of them, with its row, in 75;
    - keys: two rekeys or more in 646, back to an earlier key in 364, to the same key in 478; a
      reopening right after a rekey in 392, after an interrupted one in 165, trying another key
      first in 676; a write right after a rekey in 538.
  - Planted bugs, all caught: 139 in `keel-store`, 45 of them new, aimed at the key, encryption,
    damage and the check, and 9 re-aimed at code this slice changed. The property and crash
    tests alone catch 120, two of which only the unit tests reached before: a journal that isn't
    the WAL, which the crash tests now see in a crashed store that looks closed cleanly, and a
    stored event's hash left unchecked, which the full check now meets. The other 19 only the
    unit tests reach: settings and versions nothing else can see, invalid input, and failures
    only a unit test provokes, such as a rekey SQLCipher can't do.
  - Every property test in `keel-store` passed 100,000 cases.
  - CI green.
- **Decisions:** [ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md), accepted
  2026-09-30 after review, with the details the build settled under "As built":
  - opening tells a key that doesn't open the store from a file SQLite finds malformed. When a
    journal was left behind, the store checks the key through a connection that can't write
    first, since one that can would tidy the journal away, the only sign of a crash;
  - after each read, one more, from the cache, proves the connection met no damage, at a cost
    of about 2 µs; a damaged store refuses everything until it is reopened, and its check opens
    the file afresh;
  - costs, against the same store unencrypted on the development machine: a write of one event
    90 to 210 µs more, the more pages it touches; for a store of 100,000 events, 72 MB, opening
    in 0.8 ms, the full check in 2.7 s, and a rekey in 1.1 s;
  - the check reports its problems in order, each by page, row, device and position, projection
    and stream, or effect, and stops after damaged pages or a malformed database;
  - the golden store of schema version 2, 94 KB, holds something of everything the store keeps.
- **Found and fixed during the build:**
  - SQLCipher hands SQLite a page that fails its authentication as zeros, and fails the reads
    after it. The damage property test found the last page of a long value read back with zeros
    in it and no error: events caught it by their hashes, but an effect's payload or a
    quarantined message wouldn't have. The store now proves every read with one more.
  - A connection that can write moves the WAL into the database and deletes it as it closes,
    even when its key was refused: a shell trying its current key, then the pending one, after
    a crash during a rotation, would have hidden the crash. The store now checks the key through
    a connection that can't write first, when there is a WAL.
  - Opening took any failure of its first read for a refused key, so a file cut short by a whole
    page was reported as `KeyRejected`; it is now `Damaged`.
  - Planted bugs the tests first missed:
    - the tests' keys were 32 equal bytes, so a key given in reverse was the same key. They now
      differ, and the golden store was made again;
    - no property test met damage first in a write: the damage property's reads now include
      writes that load an aggregate;
    - SQLCipher's log level is the process's, which the tests' own connections set too: the
      crash test's child, a process of its own, now reports the level its store left.
  - Two planted bugs changed nothing anyone could see, and were planted again: dropping the
    check that temporary storage is in memory, while still asking for it; and the check keeping
    one stream's rebuild, which the rollback of its transaction undid anyway.
  - The golden store's maker, a test ignored by default, would have run in CI, which runs
    ignored tests: it now runs only when an environment variable asks.

### Projections and the outbox: step 5, slice 2 (2026-09-29)

Commit `7a85745`. Reviewed 2026-09-29.

- **Built** ([ADR-0017](./adr/0017-projections-and-outbox.md)):
  - projections: a row for each order and each payment, folded with `keel-domain`'s folds from
    the stream's events in canonical order. Each write recomputes the rows of the streams it
    touched before it commits, so a late event needs nothing more; a projection whose version
    changed is dropped and rebuilt from the log when the store opens;
  - reads: an order or a payment by identifier, the orders in a state, an order's payments, and
    any aggregate loaded from its stream, inside a write or outside one. A write reports the
    streams it touched;
  - the outbox: effects with an idempotency key, a kind, a payload and the event that caused
    them, enqueued, started, finished, retried and failed inside writes; the due effects, and
    the running ones, which after a restart are in doubt;
  - schema version 2, which adds the outbox and the projections' versions, and migrates version
    1 stores; a fault point for rebuilds.
- **Verified:**
  - Known-answer tests: 18 more in `keel-store` (33 in all), among them an order and a payment
    projected as they fold, a late event folded in its place, rebuilds on a version change and
    on request, an interrupted rebuild, a version 1 store migrated, and every state of an
    effect, and every refusal. The golden rows pin each projection's rows for a fixed history.
  - A property test against a model of projections: writes that append events and receive two
    other devices' logs a few events at a time, commit or fail, with reopenings and rebuilds.
    After every step, each order's and payment's row is the model's fold of the stream's stored
    events in canonical order, as are the lists by state and by order, every aggregate loaded,
    and the streams each write reports it touched; a rebuild changes nothing.
  - A convergence property: two stores receive the same events, each in its own order and its
    own writes, some failing, and end with byte-identical projections, and the model's.
  - A property test against a model of the outbox: every change, and why a change is refused
    (an unknown key, another effect under the key, the wrong state, not yet due, a cause the
    store doesn't hold), inside writes that commit or fail, with reopenings.
  - The crash tests' workload now also rings up an order and moves effects along; after every
    crash, the projections equal a rebuild from the stored events, and the outbox holds exactly
    the effects of the stored writes.
  - Coverage probes, over 1,000 cases: an event arriving after others with later HLCs in its
    stream in 72%, and the arrival order folding differently from the canonical one in 62%;
    every order and payment state in 54 to 891 cases; closed checks in 90, tips in 75, a
    payment on an opened check in 453, a business date differing from the last event's in 442;
    every outcome of every outbox change, an effect started before it was due in 85, and due
    effects out of the order they were enqueued in 153.
  - Planted bugs, all caught: 94 in `keel-store`, 43 of them new, aimed at projections and the
    outbox, and 7 re-aimed at code this slice moved. The property and crash tests alone catch
    83; the other 11, settings, versions and invalid input, only the unit tests reach.
  - Every property test in `keel-store` passed 100,000 cases.
  - CI green.
- **Decisions:** [ADR-0017](./adr/0017-projections-and-outbox.md), accepted 2026-09-29 after
  review, with the details the build settled under "As built":
  - projections hold one row per stream, recomputed from the whole stream in each write that
    touches it, measured at about 2.3 µs an event on the development machine;
  - enqueuing a key the outbox holds is a no-op for the same effect, and refused for another;
  - an effect's cause must be an event the store holds.
- **Found and fixed during the build:**
  - The first planted-bug run hung: a bug that kept projections from being built made every
    write fail, and the convergence test, which lets writes fail on purpose, retried forever.
    It now fails on any error it didn't plan.
  - The projection property didn't check loading aggregates; it now compares every aggregate the
    store loads with the model's fold.
  - Two outbox refusals came up too rarely: starting an effect before it's due, in 5.6% of cases,
    and due effects out of the order they were enqueued, in 2.9%. Checking the due effects at
    later times too, and more retries, raised them to 8.5% and 15%.
  - The crash tests take 20 s instead of 4 s in a debug build, since each write folds the streams
    it touches from the start, and the workload's streams grow to hundreds of events.
  - One planted bug couldn't change what the code does: letting an effect's kind start with a
    capital, which the check on every letter refuses anyway. It was re-aimed at a kind starting
    with a digit.

### The event log store: step 5, slice 1 (2026-09-29)

Commit `f9e68d7`. Reviewed 2026-09-29.

- **Built** ([ADR-0016](./adr/0016-device-store.md), decisions 1 to 7):
  - `keel-store`, the kernel's first platform crate: each device's events in one SQLite
    database, through `rusqlite` with SQLite compiled in, in WAL mode, with every commit on disk
    before it returns, and a versioned schema;
  - writes: `Store::write` runs a closure whose appended and received events commit together or
    not at all. Appended events leave the store only once their write has committed, and a
    failed write puts the device's log writer back to its log and clock as stored;
  - received events: verified with the device registry, and for the store's location. The next
    event of its device's log is stored, and the device's clock observes it within the drift
    limit; a duplicate changes nothing; after a gap, the caller learns where the log ends; and
    anything else goes to the quarantine, once, with one of 13 reasons. The device's own events
    from another replica move its writer and clock forward;
  - reads: a device's log from a position, each device's head, the version vector, an event by
    identifier, a stream's events in canonical order, and the quarantine;
  - opening refuses another device or location, a key that didn't sign the device's events,
    and a database a newer kernel wrote;
  - fault points (`Migrating`, `Began`, `Stored`, `Committing`, `Committed`) for crash tests and
    the simulator;
  - in `keel-events`: `LogWriter::restore` and `LogWriter::latest_hlc`, `LogHead::from_parts`,
    and `SignedEvent::from_stored`, behind a `stored` feature that only the store enables.
- **Verified:**
  - Known-answer tests: 15 in `keel-store`, each rule worked through with a few devices,
    among them the durability settings as SQLite reports them, each refusal quarantined once
    with the first event kept, a row changed on disk read back as corrupt, and the failure the
    property test found; 3 in `keel-events`.
  - A property test against a model of the store's rules. A case is up to nine writes and
    reopenings. Each write appends events and receives messages of 16 kinds: the next event of
    a log (sometimes beyond the drift limit), one received before, a fork, one after a gap,
    one that doesn't link or whose clock went back, one signed with another key, from an
    unknown, revoked or foreign device, naming another location, with a used identifier, at the
    last position a store holds or beyond, garbage, a quarantined message again, and the
    device's own events from elsewhere. Steps may share a physical time, so that devices' HLCs
    tie. A write commits, fails, or is interrupted at a point; some reopenings first open the
    store as another device, at another location, or with another key. After every step,
    everything the store reads back is checked against the model, and so is the device's clock:
    after everything it must follow, no later than it had to be, and given back by a failed
    write. The model decides each message's fate from how the test made it, never by calling
    the verifier.
  - A new property in `keel-events`: a writer restored to a stored head and clock carries on
    exactly as a new writer resumed from them, back after a failed write, on past the device's
    own events from elsewhere, or anywhere with any clock.
  - Crash tests, in a child process: an abort at every point of a six-write workload in turn,
    37 crashes; and 32 kills, each a moment after a chosen write's acknowledgement. After each,
    the store holds every acknowledged write and nothing half-written, every log verifies, the
    clock came back, the device's writer carries on, and a crash in a migration left nothing of
    it.
  - Coverage probes, over 1,000 cases: every fate of a received message in 140 to 890 cases
    (but the two catch-all reasons, which nothing reaches today);
    every ending of a write in 137 to 874; a quarantined message whose fate has changed since,
    received again, in 75; ties between devices' HLCs in a stream in 55; the last position a
    store holds and the first beyond it in 80 and 67; and each kind of stranger, with and
    without the device's own events, in 65 to 91. In the restored-writer property, 40% of
    cases restore behind the head and half behind the writer's clock, and a quarter of the
    observations are beyond the drift limit. The kills landed up to four writes after the
    chosen acknowledgement, two of 32 between a commit and its acknowledgement.
  - Planted bugs, all caught: 51 in `keel-store`, 46 of them by the property and crash tests
    alone. The other 5, three settings, a newer kernel's database and a row changed on disk,
    only the unit tests can reach. In `keel-events`, 6 new ones, in restoring a writer, caught
    by its property tests alone. The runner now skips, under `--props`, a bug marked `"unit"`,
    which only unit tests can catch; `keel-events`' five known-answer-only bugs (byte-level
    checks of COSE messages and keys) are marked so.
  - The store's property test, and every log property in `keel-events`, the restored writer's
    among them, each passed 100,000 cases. A larger crash run, not kept, passed too: an abort at
    every point of a 24-write workload (145 crashes), and 300 kills.
  - CI green.
- **Decisions:** [ADR-0016](./adr/0016-device-store.md), accepted 2026-09-29 after review, with
  the details the build settled under "As built (slice 1)":
  - the checks on a received event run in a fixed order, each refusal with a stable code;
  - the quarantine keeps a message's first reason, even when its fate has changed since;
  - a failed write gives back the clock as well as the log;
  - reading an event back checks its stored hash, not its signature.
- **Found and fixed during the build:**
  - The property test found that an interrupted write could leave the device's clock an hour
    ahead: the write had received the device's own event from elsewhere, stamped an hour
    ahead, and the rollback put back the log but not the clock. `LogWriter::restore` now takes
    both, the store restores both after any failed write, and the test checks the clock after
    every write.
  - A unit test that reopened a store with the same entropy at the same millisecond minted an
    identifier the store already held, and its write failed. The tests now reopen with new
    entropy; the simulator must give each restart its own (see "Known gaps").
  - The first kill test slept fixed delays, 5 to 405 ms, against a workload that takes 330 ms
    in a debug build, so where its kills landed depended on the machine. It now kills a moment
    after a chosen write's acknowledgement.
  - The child's first acknowledgement shared a line with the test harness's own output, and was
    missed; the parent now looks for acknowledgements anywhere in a line.
  - A quarantined message whose fate had changed came up in 2.5% of cases; aiming the
    generator at them raised it to 7.5%.
  - The first run of the planted bugs found one that no test caught: a migration committing
    before its fault point. The unit test only checked that the store opened afterwards, which
    an empty store also does. It and the crash test now check that nothing of the migration was
    kept. Three checks on opening were first unit-only; the property test now opens stores as
    strangers, so they are its to catch.

### Payments and closing: step 4, slice 4 (2026-09-28)

Commit `c78bc15`. Reviewed 2026-09-29.

- **Built** ([ADR-0015](./adr/0015-checks-and-payments.md), decisions 5 to 9):
  - closing a check: `order.check_closed` records what pricing charged the check, line by line
    and tax by tax, the version of the pricing rules, and the payments that settled it;
  - the fold's rules for closed checks: the snapshot stands; a closed check freezes what its
    lines cost and where they are paid; a line a close left off moves to an open check, or to a
    post-close check opened for it; and a conflict for each surprise;
  - closing and reopening the order: `order.closed` and `order.reopened`, and commands that
    refuse to change a frozen line, or to void or abandon an order with a closed check;
  - the payment aggregate, whose identifier is the idempotency key: `payment.initiated`,
    `payment.authorized`, `payment.captured`, `payment.failed` and `payment.voided`, for cash
    and card; a state machine that keeps money that may have moved when events arrive out of
    turn; and commands that record a payment's outcome;
  - checkout, across an order and its payments: each check's balance, starting a payment,
    closing a check, and the issues it reports.
- **Verified:**
  - Known-answer tests, with the arithmetic worked out: 21 more in `keel-domain` (49 in all),
    among them a table paying check by check (a tip, a split tender, change from a note, and
    each check's tax shared among its lines by largest remainder); a closed check freezing its
    lines; lines a close left off; post-close checks; and the payment state machine.
  - The eight new schemas' pinned payloads were encoded independently by Python's `cbor2`, from
    the documented key tables, and the kernel's encoding matches them byte for byte.
  - The payload model covers the eight new schemas. Three new properties aim at processor
    references at their length limits; snapshots with one rule broken and the others kept (a
    gross below its net, a net, tax or taxable amount at or below zero with the sums still
    adding up, lines or taxes out of order or repeated, no lines, an amount in another
    currency, a sum a unit or two off); and cash a unit or two short of what is paid.
  - The fold model follows the checks and the lines' allocations in one pass over the events,
    and queries the rest; a new property aims at closing, forged check identifiers included.
    The command model covers closing checks through checkout, with real payments, closing and
    reopening orders, and every refusal the freeze adds, and now says why a command is refused,
    not only whether. Commands are also checked against it on merged orders, and on every state
    a merge passes through, from the order's location and another: a new property races devices
    closing checks against others removing, firing or moving lines, and then a device carries
    on with the merged order.
  - The payment fold against a model written as queries (the first capture, and runs of
    outcomes before it); the payment commands against a model of their rules, which also says
    why a command is refused, with commands at the edges of the rules (a unit over the
    authorization, say) tried at every state; devices acting concurrently on one payment.
  - Checkout against a model of its rules: starting payments and closing checks refused exactly
    when the model refuses them, and for the same reason; every balance and issue after every
    action, with payments given twice and another order's payment left out; each close
    recording exactly what pricing charged and who paid; and the issues of devices working a
    table concurrently, merged and in each device's partial view.
  - Coverage probes, over 1,000 or 2,000 cases each: every new conflict in the closing
    property in 170 to 850 cases; all eight out-of-turn payment outcomes in 170 to 550; checks
    closed about 1,800 times in 1,000 runs of checkout; every issue, a payment after a close in
    240 of 1,000 concurrent tables, an underpaid check in 440 partial views; and in the race, a
    closed check whose lines were all removed in 6% of cases, and an order abandoned after its
    only line was fired in 2%.
  - Planted bugs, all caught, and all caught by the property tests alone: 220 in
    `keel-domain`, 126 of them new, aimed at closing, freezing, payments and checkout.
  - Every property test in `keel-domain` also passed 100,000 cases.
  - CI green.
- **Decisions:** [ADR-0015](./adr/0015-checks-and-payments.md), accepted 2026-09-28, with the
  details the build settled recorded under "As built", accepted after review on 2026-09-29:
  - **A closed check freezes what its lines cost and where they are paid.** Their seat, course
    and notes can change, and they are still fired, so an order paid first is still prepared.
  - **An allocation doesn't move a frozen line**, even from a device acting concurrently, since
    that would charge its paid part twice; every other change to it applies, and is reported.
  - **Lines a close left off** move to the first open check that doesn't already hold part of
    them; a new line goes to the first open check. Otherwise a post-close check opens, named
    after the event that opened it, so every replica opens the same one.
  - **Reopening** reopens every closed check with the order; a voided or abandoned order is
    final.
  - **A capture always stands.** After it, nothing changes the payment; an authorization or a
    capture after a failure or a void applies, since the card may be held or charged.
  - **Checkout also reports** a payment after its check closed, and a closed check its payments
    no longer cover.
  - **Voiding or abandoning an order with a closed check** is refused: reopen it first.
- **Found and fixed during the build:**
  - The first checkout generators closed a check only 82 times in 1,000 runs, so a payment
    after a close came up 8 times in 1,000 concurrent tables. A "settle" action, which pays a
    check in full and closes it, each step still checked against the model, raised that to about
    1,800 closes and 240 cases.
  - Two rules were out of the property tests' reach: forged check identifiers, which only a
    hostile device writes, never met the new-check fallback, and checkout never saw a payment
    given twice or another order's payment. A forgery aimed at the next event that opens a
    check, and noisy payment lists, now reach both.
  - A payment a cent short of its check came up too rarely to test the close rule's boundary;
    the generators now pay a cent short on purpose.
  - The first full run of the planted bugs found 20 that the property tests alone missed, and
    the rerun a 21st that the first had caught only by luck. The tests were strengthened until
    all were caught:
    - eight of the snapshot's rules. The snapshot property only moved an amount, which broke a
      sum as well, and never reordered or emptied a list; the property that changes any payload
      rarely reached a snapshot. The snapshot property now breaks one rule at a time, and moves
      the sums to match;
    - five refusals that a second rule backs up, such as a payment in another currency, which
      also fails the limit it can't be compared with, and closing an empty check, whose snapshot
      also fails its schema. The models of order, payment and checkout commands now say why a
      command is refused, not only whether;
    - a capture for more than was authorized came up too rarely: the payment tests now try
      commands at the edges of the rules at every state;
    - sorting a check's lines for its snapshot never mattered, because new lines always had
      rising identifiers: the checkout tests now hand them out in no particular order;
    - reopening a closed order from another location came up too rarely: commands on merged
      orders are now tried from the order's location and another;
    - abandoning an order with a closed check, and abandoning one whose only line was fired
      and then removed, happen only when devices race. The second came up often enough until
      this slice's events joined the generators. The new race property reaches both. Only a
      concurrent removal can empty a closed check, so no single device's commands reach the
      first, and a known-answer test covers it too;
    - three planted bugs couldn't change what the code does. Two removed checks that another
      rule backs up with the same result (a line's amount, and a cash rounding, in another
      currency), and are no longer listed; one targeted code that moved into `lines_on`, and
      was re-aimed.
  - 17 planted bugs no longer matched the code they target, rewritten for closing; their
    anchors were refreshed.

### Checks and splits: step 4, slice 3 (2026-09-28)

Commit `fdd6d6f`. Reviewed 2026-09-28.

- **Built** ([ADR-0015](./adr/0015-checks-and-payments.md)):
  - every order's main check, whose identifier is the order's own, and `order.check_opened` for
    more checks, numbered in the order they are opened;
  - `order.lines_allocated`, which moves or splits lines among checks in whole shares, kept in
    lowest terms so each split has one encoding;
  - the fold's rules for concurrent splits: for each line, the later allocation wins; one naming
    a check that doesn't exist yet doesn't apply, and is reported; a check opened twice keeps
    its first opening; allocating a removed or voided line changes nothing;
  - commands to open checks and allocate lines, checked against the device's view;
  - each check's basket, which prices the check as its own sale;
  - in `keel-pricing`, a line's *share*: a basket holds its part of a line split among several,
    by largest remainder, and the trace records the split.
- **Verified:**
  - Known-answer tests, with the arithmetic worked out: 4 more in `keel-domain` (28 in all),
    among them a table split by seat with the wine shared three ways and each check taxed on its
    own; 2 more in `keel-pricing` (18 in all).
  - The two new schemas' pinned payloads were encoded independently by Python's `cbor2`, from
    the documented key tables, and the kernel's encoding matches them byte for byte.
  - The payload model covers allocations: out of order, repeated, not in lowest terms, empty, or
    with a share of zero, each accepted exactly when valid. A new property checks that
    allocations have one form, whatever order they are given in.
  - The fold model, written as queries over the events' positions, covers checks and splits,
    and a new property aims at splits. The command model covers opening checks and allocating
    lines, faulty commands included. Invariants check that every live line belongs to existing
    checks, in lowest terms; that each check's basket holds its part of each line allocated to
    it; and that the checks' parts of a line add up to the line.
  - In `keel-pricing`, the exact oracle and the independent Python oracle price shares, with 31
    new golden baskets (140 in all). A new property checks that a shared line's parts add up to
    it, each within one minor unit of its exact share, and invalid shares are refused.
  - Planted bugs, all caught, and all caught by the property tests alone: 94 in `keel-domain`,
    22 of them aimed at checks and splits, and 51 in `keel-pricing`, 6 of them aimed at shares.
  - Every crate's property tests also passed 100,000 cases each.
  - CI green.
- **Decisions:** [ADR-0015](./adr/0015-checks-and-payments.md), accepted 2026-09-28.
  - **Checks are part of the order's stream**, and every order has a main check with the order's
    identifier, so a one-check order needs no check events.
  - **Lines are split among checks in whole shares**, in lowest terms: exact, and the parts add
    up by construction.
  - **Each check is priced as its own sale**, so its tax is computed on it. A split order's
    checks can pay a cent or so more or less tax than the order would unsplit.
  - **Concurrent splits:** the later allocation of a line wins.
  - **Payments and closing**, designed now and built in slice 4: a snapshot of what each check
    was charged when it closes; closed checks frozen; post-close checks for lines added
    concurrently; the payment aggregate, with five events for cash and card; and checkout rules
    that keep a check from being charged twice.
- **Found and fixed during the build:**
  - The first generators rarely split a line among checks: a probe of 2,000 cases found only 20
    orders ending with a line shared by several checks. A new property with generators aimed
    at splits reaches that in nearly a quarter of its cases, with over a thousand allocations
    applied per 2,000 cases.
  - An old planted bug, reason codes of 33 bytes, escaped the property tests: with two more kinds
    of event, the wide generators reached a 33-byte code too rarely. A new property tries reason
    codes at every length around the limit.

### `keel-pricing` v0: step 4, slice 2 (2026-09-28)

Commit `7638563`. Reviewed 2026-09-28.

- **Built:** a pure pricing function from a basket, the order's lines as rung up, and the
  location's rules to the totals ([ADR-0014](./adr/0014-pricing-engine-v0.md)):
  - line extension: the item's price plus its modifiers', nested and counted per unit, times the
    quantity, rounded for weighed and measured items;
  - comps, line discounts and order discounts, each taking from what is left, a percentage
    rounded and an amount limited to what is left; order discounts allocated to the lines by
    largest remainder;
  - US sales tax: taxes by category, optionally only on the premises or only to go, on each line's
    net, rounded per line or once per document and then allocated; exemptions by tax;
  - totals for every line and for the order, and a step-by-step trace that explains them;
  - cash rounding, for the tender to call;
  - validation that refuses invalid input rather than guessing;
  - `Order::basket` in `keel-domain`, which turns an order's live lines into a basket, so an
    order can be priced.
- **Verified:**
  - 16 known-answer tests and a doctest, each with its arithmetic worked out, and 7 property
    tests, which also passed 100,000 cases each. A known-answer test prices a real order, and the
    order property tests check every folded order's basket against its live lines.
  - An exact oracle in arbitrary-precision integers, written from the ADR with its own rounding
    and allocation: the engine agrees on every amount, and reports an overflow exactly when an
    amount doesn't fit. Generators aim at ties at every rounding point, allocation ties
    (repeated lines), exact-amount discounts and tax rates above 100%; a probe of 20,000 cases
    confirmed each happens more than a hundred times.
  - 109 golden baskets (café, restaurant, grocery with weighed items, exemptions, yen and dinar,
    edge cases, a tie under each rounding mode), with totals from an independent Python oracle
    that uses exact fractions. CI regenerates them and fails on any change.
  - The trace replays into the totals and accounts for every tax; the parts add up; reordering
    the lines changes neither the gross, the discounts nor the net, nor, without order discounts,
    the tax.
  - 45 planted bugs in `keel-pricing` and 5 in the order's basket, all caught; the property tests
    alone catch every one.
  - CI green.
- **Decisions:** [ADR-0014](./adr/0014-pricing-engine-v0.md), accepted 2026-09-28.
  - **Snapshot prices:** pricing uses the prices recorded when a line was rung up; price lists and
    the catalog's modifier rules apply then, and come with the catalog.
  - **Five rounding points**, each with an explicit mode (half away from zero by default):
    extension, percentage discounts, allocation, tax, and cash. The domain model said three.
  - **Discounts take in turn from what is left**, so they can never take more than the line or
    the order; an amount discount larger than what is left is limited, and the trace says so.
  - **A percentage order discount is rounded once** on the order, then allocated, so it is exactly
    that percentage of the order.
  - **Discounts reduce the taxable amount; taxes don't compound.** Per-line and per-document tax
    rounding are both supported, as jurisdictions require.
  - **Lines, discounts and taxes are identified by position** in the basket and the rules.
- **Found and fixed during the build:**
  - The domain model's "rounding happens in exactly three places" missed weighed items and
    percentage discounts; it now lists five.
  - Planted bugs found three gaps in the first tests: no test at all covered an amount discount of
    exactly what is left, and the property tests alone missed the deepest allowed modifiers and
    cash rounding. Each now has a known-answer test and a property.
  - A final review found two more gaps, each confirmed by a planted bug that the property tests
    missed: the engine accepts tax rates above 100%, as some excise taxes need, but no generator
    produced one; and the trace replay didn't check that every tax is accounted for, so a trace
    that left out an exemption passed. The generators now include such rates, the replay
    accounts for every tax, and known-answer tests cover a tax above 100% and an empty basket
    (the case on which a property test once caught a bug in its own replay).

### `keel-domain` slice 1: payloads, the schema registry, and the order's lines (2026-09-28)

Commits `fbd1507`, `1b6dd56`, `147870c`, and after review `0188d4d`, `13a2847`, `16df799`.
Reviewed 2026-09-28.

- **Built:**
  - Payload codecs: strict maps with integer keys; money, quantity, identifier, text, code and
    identifier-set encodings.
  - The schema registry, with a pinned example payload for every schema. The fold entry point
    skips events of unknown schemas, malformed payloads and other streams, and reports them.
  - The canonical order of provisional events: by clock, then device, then log position.
  - The order aggregate: ten version-1 event schemas; the order's state, with a total fold that
    applies the sync design's conflict rules and reports 15 kinds of conflict; and commands
    checked against the device's view, each producing one event that must decode under its own
    schema before it is recorded.
- **Verified:**
  - 24 known-answer tests and 12 property tests. The property tests also passed 100,000 cases
    each.
  - The pinned payloads of all ten schemas were decoded independently with Python's `cbor2`, and
    match the documented key tables.
  - Independent models:
    - a validity model of every schema, against payloads with fields changed, removed or added;
    - a model of the fold, written as queries over the events' positions. It is checked on any
      events, on events aimed at the conflict rules (colliding lines, other currencies, units
      and locations), and on devices acting concurrently on stale views;
    - a model of the command rules, checked with deliberately faulty commands.
  - 64 planted bugs, all caught, and the property tests alone catch every one.
  - CI green.
- **Decisions:** [ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md), accepted 2026-09-28.
  - **Payloads** are canonical CBOR maps with small integer keys, decoded strictly. Money is
    `[minor units, currency code]`, and quantity `[millionths, unit code]`. Enumerations are
    integer codes.
  - **Schema evolution:** every payload change is a new version. Writers use a version only once
    every kernel at the location knows it (negotiated by sync), so a newer version never meets
    an older kernel in normal operation.
  - **Undecodable events** from a trusted device stay in the log, so the device's chain never
    stalls, but the fold skips them and the order reports them ("needs update", or malformed).
  - **Text** in names and notes can't contain control characters, which could otherwise drive
    receipt printers and kitchen displays.
  - **Events that would break the order don't apply:** a line or modifiers in another currency,
    a quantity in another unit, an event from before the order's creation or from another
    location. They stay in the log and leave a conflict, so every price in an order can always
    be added up. Conflicts are derived by the fold, not recorded as events.
  - **Commands** refuse no-op changes. A void needs a fired line (a pending line is removed). The
    first removal or void of a line wins, and so do the first comp and the first close.
  - **Flags** for a fired line removed rather than voided, and for lines added or fired after
    the order was closed.
  - **Stage** (Draft, Open, Submitted) is derived from the lines, not stored.
  - **A line records** the item's variant, name, tax category, unit price and the catalog
    version it was priced from.
  - **Version 1 is positive:** quantities above zero and prices of zero or more. Returns will
    have their own events.
  - `Id::cast`, to use an order's identifier as its event stream's.
- **Found and fixed during the build:**
  - Planted bugs found gaps twice. At first, 3 of 51 escaped every test: payload rules that span
    fields. A unit test and sharper generators closed that gap. The property tests alone then
    still missed 18 that unit tests caught. A full model of the fold, faulty commands, and
    generators aimed at rules that span fields or sit on a boundary (one currency per payload at
    any depth, text lengths, identifier sets out of order) closed it. The 13 planted bugs added
    for the new models were all caught.
  - `provisional_order` had no test; it now has known answers and a property.
- **After review:** abandoning an order needs that nothing in it was ever fired, so an order the
  kitchen worked on always ends up voided. A line now remembers whether it was ever fired, and the
  fold reports an order abandoned with fired lines. Seven planted bugs aimed at the rule, three of
  them new, are all caught by the property tests alone, which makes 67 for the slice.
- **After review:** every crate's planted bugs now live in `tools/planted-bugs/`, with a runner
  that anyone can rerun. Running them all again found a bug in the record: an old script counted a
  bug that didn't compile as caught (in `keel-events`). It also showed that `keel-events`'
  property tests reached three envelope edge cases too rarely: a sequence number of 0, a padded
  date, and a field under the negative of its key. A new property tries a near miss for every
  field of the envelope, and every field under its negative key, and catches all three every time.

### `keel-events`: the signed, hash-chained event log (2026-09-27)

Commits `34fda99`, `f79e701`.

- **Built:** a strict canonical CBOR codec; the event envelope (format 1); ES256 and Ed25519
  signatures in COSE_Sign1 messages; event hashes; the device log writer (numbering, chaining,
  timestamping and signing in two steps, so memory never runs ahead of storage); the link check
  for received events (gaps, forks, broken links, clocks going backwards); the device registry
  with revocation by log position.
- **Verified:**
  - 38 unit tests and 26 property tests, which also passed 100,000 cases each.
  - Interoperability both ways with Python's `pycose`; a pinned reference event checked
    independently with `cbor2`, `hashlib` and `pycose`.
  - 58 planted bugs, all caught; the property tests alone catch 53. The other 5 are the
    byte-level checks of COSE messages and keys, which known-answer tests catch. (Recounted on
    2026-09-28 with the planted-bug runner, which found that one listed bug had never compiled:
    the earlier scripts counted a compile error as a caught bug. The lists hold 58 bugs; the
    earlier record said 59.)
  - CI green.
- **Decisions:** [ADR-0012](./adr/0012-event-wire-format.md), accepted 2026-09-28: a location
  field in every event, revocation by log position rather than time, one key per device for life,
  and strict decoding everywhere.
- **Found and fixed during the build:**
  - A payload nested deeper than the decoder allows could be signed but never decoded, which
    would have stalled the device's log for good. Such payloads are now refused when created.
  - Planted-bug runs were saving their failure seeds as if they were real regressions. The seeds
    were removed, and the conventions now require disabling proptest's failure persistence for
    planted-bug runs.

### `keel-types`: the kernel's value types (2026-09-27)

Commits `2bb1c68`, `493dd3b`, `de45ce0`.

- **Built:** money in integer minor units, with exact products (up to 192 bits) rounded once;
  seven rounding modes and cash rounding; rates; quantities in millionths with 47 exactly defined
  units; ISO 4217 currencies from a verified snapshot (155 circulating, 6 withdrawn); timestamps
  and clocks; hybrid logical clocks; UUIDv7 identifiers; business dates with daylight-saving-safe
  cutoffs.
- **Verified:** 110 tests and 2 doctests, with property tests against an exact `num-bigint`
  oracle and an exhaustive sweep of every time zone transition from 1970 to 2037. CI green.
  16 planted bugs, all caught; the property tests alone, with the exhaustive sweep, catch 13, and
  unit tests the other 3 (counted on 2026-09-28).
- **Found and fixed during the build:** double rounding in `Money::mul_decimal` (caught by a
  generator aimed at rounding boundaries after 2,000 uniform cases missed it); parsing of
  `i64::MIN`; an order-dependent sum; a naive business-date algorithm that failed on real time
  zone histories (Algiers 1971, Samoa 2011).

### Design (2026-09-27)

Commits `a9fad0c`, `545db4b`.

Six research tracks, the architecture and eleven ADRs, the domain model, the offline and sync
design, the feature catalog and the roadmap. See the [README](../README.md).

## Waiting on a decision

- The license, and the product name ("Keel" is a codename).
- The first payment processor (decision gate G2 in the roadmap).
- Verifying the research's unverified claims, and interviews with merchants.
- Updating the pinned Rust toolchain from 1.94 to 1.98.
- License and security-advisory checks in CI (`cargo-deny`), GitHub Actions pinned to commit
  hashes, and CI on macOS and Windows.

## Known gaps

Work deliberately left for later, so it isn't forgotten:

- **`keel-events`:**
  - Enrollment and revocation records signed by the Device CA, and their distribution to replicas.
  - A way to re-admit genuine events quarantined by a revocation.
  - Hardware signers, which the platform apps provide through the `Signer` trait.
  - Negotiating schema versions between kernels, in `keel-sync`.
- **`keel-types`:** `Locale`, with the first UI.
- **`keel-store`:**
  - Power-loss tests, which need keel-sim's simulated disk; until then the durability settings
    are checked as SQLite reports them.
  - Retention, snapshots and backups, after the sync engine.
  - Salvaging a damaged store: reading what still reads, above all the device's own events not
    yet sent, and starting again from its peers. A store that can't open, because a page opening
    reads is damaged, can't be checked yet either.
  - The platform shells make, wrap, store and rotate the store's key (steps 7 and 8), as ADR-0018
    describes. Rotation follows the protocol the store's crash tests check.
  - SQLCipher's HMAC-SHA512 may be slow on ARM cores without SHA-512 instructions: measure on the
    reference register (step 7), and switch to HMAC-SHA256 before the first deployment if so.
  - `rusqlite` bundles SQLCipher 4.14.0, on SQLite 3.51.3, while SQLCipher is at 4.19: follow its
    releases. Its rekey reports success when it fails, which the store works around (ADR-0018):
    check each upgrade against the store's tests.
  - Check balances in a projection: they need the location's pricing rules in the store.
  - Folding only what's new: each write folds the streams it touches from the start, about
    2.3 µs an event on the development machine. Long-lived streams, such as a gift card's, will
    need folded state kept between writes.
  - Telling the UI what changed: a write reports the streams it touched, for the shell to show
    again; subscriptions come with the shell.
  - Which replica enqueues an effect, by its executor tier, comes with the first effects:
    payments, printing and fiscal submissions.
  - Releasing or discarding quarantined messages: they are kept for a person to look at, with
    no tools yet.
  - Two processes opening one store at once: a store is one process's. Opening re-checks the
    schema version inside its transaction, but no test races two openers.
  - Code that opens SQLite connections of its own, in a process with stores, must call
    `init_sqlite` before its first, or SQLCipher's initialization can race again: the shells
    (steps 7 and 8). SQLite now offers a hook that initializes under its lock
    (`SQLITE_EXTRA_INIT_MUTEXED`); SQLCipher could use it, which is worth reporting.
  - A statement cache: reading an event's number or a page of the feed spends most of its time
    preparing statements, as every read of the store does. `rusqlite`'s `cache` feature would
    help them all.
  - `confirmed()` reads every run of every record. The UI, which shows what is provisional, will
    need each device's confirmed start kept as records arrive.
  - Records of two devices in one epoch, which slice 4's fencing rules out, can give two events
    one number: the feed gives both, and a page that ends between them skips the second.
- **`keel-sync`:**
  - Slices 3 and 4 of step 6: ownership leases, and hub election and failover.
  - Until slice 4, a record from any device enrolled at the location counts. A record from a
    device that isn't the hub could leave a stretch of a log below its highest covered position
    that nothing ever numbers. Slice 4 also brings a hub restored from an older copy of its
    store, which could fork its log (ADR-0020, decision 8).
  - Batching several writes' events into one record, if the records prove too many: when events
    arrive one at a time, the store holds about as many records as events.
  - Devices send the watermark back to the hub with each round: redundant, and harmless.
  - Pruning below the watermark, with retention.
  - Showing what is provisional and what isn't yet backed up, in the shells (step 7).
  - The transport (offline-and-sync §3.2–3.3): WebSocket over TLS, discovery, scopes, device
    certificates, protocol negotiation, priority lanes, compression and reconnection backoff.
  - A device restored from an older copy that must sell before any peer answers forks its log.
    The replicas report the fork, and keep disagreeing about that log until a person resolves
    it, with tools that don't exist yet.
  - A batch waiting for acknowledgement from a peer that has since restarted holds up the next
    for up to the acknowledgement timeout (2 s).
  - A clock set back less than a round delays the rounds by as much. Timers on a monotonic clock
    would need a second kind of time passed in, beside the clock the store stamps events with.
- **`keel-sim`:**
  - Disk faults, torn writes and lost fsyncs, wait for a simulated disk: a SQLite VFS, which
    needs `unsafe` code.
  - The invariants of later slices and features: one lease holder per order, one hub per epoch;
    payments, the ledger and fiscal chains (offline-and-sync §12).
- **Orders:**
  - Permissions, approvals and ownership leases aren't checked yet (`keel-policy`, `keel-sync`).
  - An order with payments on its open checks can be voided, and checkout reports the payments:
    refusing it needs the payments, and refunds come with returns.
  - Check names, and putting a new line straight onto the check it is for (a new version of
    `order.line_added`); re-splitting a line after part of it was paid (ADR-0015).
  - Events for a person's resolution of a conflict.
  - A tool that labels payload fields, for auditors and support.
- **Payments:** everything ADR-0015 defers: refunds, returns and disputes; store-and-forward
  and asynchronous rails; tip adjustment and incremental authorization; stored value, house
  accounts and external accounts; surcharges; receipt details such as card brand, entry mode and
  EMV data, which come with the first connector. A check that closes again after a reopening is
  priced by the rules then in force. Starting a payment checks its identifier against the
  order's own payments only: one that another order's payment already has (which only a bug or
  a forgery could produce, since identifiers are random) is left to that payment's fold, which
  reports a second initiation.
- **Pricing:** everything ADR-0014 defers: price lists and the catalog's modifier rules,
  promotions, service charges, fees and surcharges, tips, tax-inclusive prices and compound
  taxes, per-item thresholds, tax holidays, the SNAP portion, manufacturer coupons,
  destination-based tax, and returns.

## Keeping this document current

Update "Where we are" and the current slice's table as work lands. When a slice ships, move it to
"Completed" with what it built, how it was verified, its decisions and its commits. Add every
deferred item to "Known gaps", and remove it when it's done.
