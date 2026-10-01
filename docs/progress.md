# Build progress

> A living record of the build: where it stands, what each step delivered and how it was
> verified, and what is waiting on a decision. Updated as each piece of work lands.
> Last updated: 2026-10-01.

## Where we are

**Phase 0 (Foundations), step 6 of 8:** `keel-sim` and `keel-sync` v0, the deterministic
simulator and the sync engine, in four slices
([ADR-0019](./adr/0019-replication-and-deterministic-simulation.md)). Slice 1, replication and
the simulator, slice 2, hub sequencing ([ADR-0020](./adr/0020-hub-sequencing.md)), and slice 3,
ownership leases ([ADR-0021](./adr/0021-ownership-leases.md)), are built and reviewed. Slice 4,
hub election and failover, is built and pushed; its last verification runs are under way, and
then it awaits review ([ADR-0022](./adr/0022-hub-election-and-failover.md), proposed). Step 5,
`keel-store`, is done: built in three slices, each reviewed.

## Phase 0 milestones

From the [roadmap](./roadmap.md#8-first-engineering-milestones-the-next-build-steps).

| # | Milestone | Status | Where |
|---|---|---|---|
| 1 | Monorepo scaffold | Rust workspace and CI done. The Android, web and schema directories arrive with their first code. | [`Cargo.toml`](../Cargo.toml), [CI](../.github/workflows/ci.yml) |
| 2 | `keel-types`: value types | Done, 2026-09-27 | [`core/crates/keel-types`](../core/crates/keel-types/) |
| 3 | `keel-events`: the signed, hash-chained event log | Done, 2026-09-27. Its schema registry was built with the first domain events, in `keel-domain`. | [`core/crates/keel-events`](../core/crates/keel-events/), [ADR-0012](./adr/0012-event-wire-format.md) |
| 4 | `keel-domain` (order, check, payment) and `keel-pricing` v0 | Done, 2026-09-29: built in four slices, each reviewed | [`core/crates/keel-domain`](../core/crates/keel-domain/), [`core/crates/keel-pricing`](../core/crates/keel-pricing/), [ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md), [ADR-0014](./adr/0014-pricing-engine-v0.md), [ADR-0015](./adr/0015-checks-and-payments.md) |
| 5 | `keel-store`: SQLite events, projections and outbox | Done, 2026-09-30: built in three slices, each reviewed | [`core/crates/keel-store`](../core/crates/keel-store/), [ADR-0016](./adr/0016-device-store.md), [ADR-0017](./adr/0017-projections-and-outbox.md), [ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md) |
| 6 | `keel-sim` and `keel-sync` v0 | In progress: slices 1 to 3 of 4 built and reviewed; slice 4 built, its verification finishing | [`core/crates/keel-sync`](../core/crates/keel-sync/), [`core/crates/keel-sim`](../core/crates/keel-sim/), [ADR-0019](./adr/0019-replication-and-deterministic-simulation.md) |
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
2. **Hub sequencing** (built 2026-09-30, reviewed 2026-10-01,
   [ADR-0020](./adr/0020-hub-sequencing.md)):
   `store_seq` per epoch, confirmed and provisional events, store durability, and the cloud's
   durable-ack watermark.
3. **Ownership leases** (built and reviewed 2026-10-01,
   [ADR-0021](./adr/0021-ownership-leases.md)): the hub grants and transfers orders' ownership;
   island mode and the manager's override.
4. **Hub election and failover** (designed and built 2026-10-01,
   [ADR-0022](./adr/0022-hub-election-and-failover.md)): priorities, heartbeats, the hot standby,
   epochs and fencing, and a split brain healing.

## Current slice

Step 6, slice 4, hub election and failover, is built, and its last verification runs are under
way (see "Completed"), as [ADR-0022](./adr/0022-hub-election-and-failover.md), proposed,
describes:

- A hub's term begins with a signed claim in its log: its epoch, its priority, the claim it
  succeeds, and how far into each earlier hub's log it holds. Every replica works out the same
  hub from the claims it holds: the highest epoch, then the higher priority.
- A successor's claim fences what the hub it succeeds wrote after it: those records stop
  counting everywhere, and the new hub numbers their events again, so numbers never repeat
  and each epoch's feed stays gapless.
- Replicas send heartbeats every second. The most preferred candidate that hears no hub for three
  of them claims the next epoch, once settled and caught up; a hub stops serving the moment a
  better claim reaches it. Devices know they are islands when they hear no hub.
- A split brain heals by the same rules; a deposed hub's grants stand, kept consistent by the
  lease's compare-and-swap.

| Piece | Status |
|---|---|
| Design, in ADR-0022 | Done |
| `keel-domain`: the `hub.claimed` payload, the winning claim, the chain and its cuts, and the rules for succeeding it | Done |
| `keel-store`: the claims projection; fencing in confirmation and sequencing; claiming, and refusing to serve when not the hub | Done |
| `keel-sync`: heartbeats and beats, the election, serving only while the term wins, forks, settling against every peer, and islands | Done |
| `keel-sim`: the standby, splits, failover, and the new invariants | Done |
| Known answers, property tests and probes | Done |
| Planted bugs, soaks and the simulator's seeds on the final code | Under way |
| CI-equivalent run and docs | Done; CI runs on the pushed commit |
| Review, and accepting ADR-0022 | Next |

Building it settled what ADR-0022's "As built" records. The largest changes to the design: a
claimant must hold the whole chain of claims back to epoch 1; heartbeats carry the hub's beat,
which a peer passes on, in place of saying it hears the hub, and name the hub's term, since a
split leaves two hubs of one epoch whose beats can't be compared; learning of a new claim counts
as hearing its hub; a replica's log has forked when a peer refuses the replica's own log, the
design's test having been the wrong way round; and a candidate waits to catch up only on what
peers other than the old hub hold. Each was found by a property test, a known answer or the
simulator, and each failure is a named regression test.

## Completed

### Hub election and failover: step 6, slice 4 (2026-10-01)

Built and pushed while its last verification runs: the restart of the build machine stopped
them, and a heartbeat fix late in the build calls for some to run again on the final code. What
is still under way is marked so below, and recorded as it lands.
[ADR-0022](./adr/0022-hub-election-and-failover.md) is proposed, waiting on review.

- **Built** ([ADR-0022](./adr/0022-hub-election-and-failover.md)):
  - `keel-domain`:
    - `hub.claimed`, version 1: a claim of the Store Hub's role, in a stream of its own of kind
      `hub`: its epoch, the claimant's priority, the claim it succeeds, and its **cuts**, how far
      into the log of each device on the chain it succeeds the claimant held.
    - `hub::chain`: the winning claim, by epoch, then priority, then the lower device, then the
      earlier claim; the chain of terms back from it, as far as the replica holds it; and each
      term's cut, the least any later claim on the chain gives. A record counts when its device
      holds the term of its epoch, and it follows the term's claim and lies within its cut.
    - `Claimed::succeeding`: the next claim, cutting each device on the chain where the claimant
      holds its log to, refused, `ClaimError::Behind`, unless the claimant holds the whole chain
      back to epoch 1 and every record that counts on it.
  - `keel-store`:
    - The claims projection, version 1, which works out the chain of terms again at each write
      that touches a claim; the integrity check reports a chain kept that isn't the one the
      claims make, `Problem::Terms`.
    - Confirmation, the feed and sequencing count only the records that count. Sequencing
      numbers each device's log after the last position such a record covers, and the hub's own
      log after its latest record of its epoch as well.
    - `Store::term()`, `terms()` and `claim(priority, now)`. `sequence(now)` and
      `answer_requests(now)` take their epoch from the store's own winning claim, and refuse,
      `StoreError::NotHub`, when it isn't the store's; a store behind the chain refuses to claim,
      `StoreError::Behind`.
  - `keel-sync`:
    - The heartbeat, `[1, 3, location, priority, epoch, hub, acting, beat]`, to every peer every
      second: the sender's priority, 0 while its log is unsettled or forked; its term, the epoch
      and hub of the winning claim it holds; whether it acts as that hub; and the hub's beat,
      which rises every period the hub acts, and which a peer that had it directly passes on.
      Beats are compared only within one hub's term.
    - The election. A replica that can be hub claims the next epoch as a heartbeat period begins
      when its log is settled and not forked, it has heard no hub and no preferred candidate for
      three periods, and it holds as much of each earlier hub's log as its other peers say they
      hold, or has waited three more periods. It serves only while it holds the winning claim with
      its log settled and not forked, and stops the moment a better claim reaches it.
    - Settling against every peer that answers; a fork, told by a peer refusing the replica's own
      log; and islands, `hub_reachable()`.
    - `Roles` gives a replica's priority as hub, and `SyncConfig` the heartbeat period and the
      periods of silence. `Replica` gains `terms()` and `claim(priority, now)`; `Replicator`
      gains `is_hub()`, `serving()`, `term()`, `hub_reachable()` and `forked()`. `keel-sync` now
      depends on `keel-domain`, for the chain.
  - `keel-sim`:
    - A standby: the hub at priority 2 and the standby at 1, linked to each other, to every
      device and to the cloud. No one holds a term at the start; the hub claims epoch 1.
    - Splits, which cut the location in two and leave a hub on each side until it heals; devices
      that are islands by their own heartbeats, not by the simulator's partitions; and runs in
      which the hub crashes without other faults.
    - New invariants: at the end, every replica holds the same winning claim, whose device alone
      serves, and a whole chain; the records that count number every event exactly once, each
      epoch's numbers gapless and each device's log in order; as the run goes, a candidate writes
      records and answers only while it serves, in its term's epoch, and a device's log settles
      only when no peer it heard from holds more of it; no request is answered in two terms.
  - Tooling: the planted-bug runner's `--release`, for the lists the simulator takes most of the
    time of, and `--stale`, which checks every bug's text is still in the code, and which CI now
    runs for every crate.
  - CI: the job's time limit rises from 45 minutes to 75. This slice's tests, the store's terms
    property, the protocol property and the simulator's failover runs above all, take each of
    its two test passes from 14 minutes to 25, and its first commit's run would have been cut
    off.
- **Verified:**
  - Known answers:
    - `keel-domain`, 18 tests: the payload pinned byte for byte, in payloads Python's `cbor2`
      encoded from the key tables, and each way it is refused; which claim wins; the chain and its
      cuts, with two claims of one epoch, a hub elected twice, a later claim that cuts no part of
      a device and a chain the replica holds only part of; and succeeding, refused when behind.
    - `keel-store`, 8 tests: claiming the first epoch; a claim's business date; a successor
      fencing what the hub it succeeds wrote after it claimed; a store behind the chain refusing to
      claim until it catches up; two claims of one epoch, the losing side's events numbered again;
      a hub elected again numbering what only a record its successor cut off numbered; a record
      before its term's claim counting for nothing; and the integrity check finding a chain that
      isn't what the claims make.
    - `keel-sync`: the heartbeat pinned byte for byte, four ways, at both ends of each field's
      range, in frames Python's `cbor2` encoded, and sixteen ways it is refused; 15 tests of the
      election: the most preferred candidate claiming at 3 s, the standby at 8 s and not before,
      deferring to a preferred candidate, hearing the hub through a peer and no further, waiting to
      catch up, claiming after waiting, not waiting for what only the silent hub held, stepping
      down, going on hearing its hub beside a losing hub of its epoch, a heartbeat acting as
      another device's hub, resuming after a restart, settling against every peer, a peer holding
      more of the log, clock jumps, and a forked replica; and two tests of telling a fork.
    - `keel-sim`: without other faults, a crashed hub is succeeded within 5 s, in 8 seeds.
  - Property tests:
    - `keel-domain`: the chain and its cuts against a model of the rules, whatever order the
      claims arrive in; claiming against a model of succeeding; epochs up to the largest; and, at
      a location of three or four replicas that append, replicate a little at a time, claim and
      number, every event counting under at most one number, and in the end exactly one. The
      payload properties take the claim too.
    - `keel-store`: stores that claim, take each other's logs a few events at a time, number and
      confirm, against a model of the chain and of what each should number; the sequencing and
      ownership properties now run under claims.
    - `keel-sync`: the protocol property, rewritten for the election: replicas whose priorities
      may tie or be none, on a star, a line or a mesh, crashing, some down for seconds, under
      loss, delay, duplication and cuts. Frame by frame its model checks each heartbeat, each
      claim's preconditions, that a log settles exactly by the rule and a replica serves exactly
      while the rule says, and that records are written only while serving, in the term's epoch;
      and at the end that the replicas agree, with one hub serving, heard within two hops, and
      every event numbered once by the records that count. Five failures it found are named
      regression tests.
    - 100,000 cases each passed, in release builds, alongside other runs: the hub properties in
      24 s (a million earlier, in 209 s), the payload properties in 3 minutes, and the store's
      sequencing property in 82 minutes and its terms property in 58. The protocol property passed
      100,000 cases in 16 minutes before its last changes, and 8,000 since; 100,000 on the final
      property, and the store's ownership property's, are under way.
  - The simulator: 5,000 seeds passed in a release build, in 92 minutes on two threads alongside
    other runs, on the code before the heartbeat fix below; a run on the final code is under way.
    - The candidates made 8,481 claims, more than one in 2,877 runs; the winning epoch was 1 in
      2,387 runs, 2 in 2,245, 3 in 353 and 4 in 15. 4,993 splits, in 3,325 runs, ran a hub on
      each side. Where the node serving as the hub crashed, in 765 runs, another claimed 3.5 s
      after at the median, 6.5 s at the 90th percentile and 24 s at most, the other faults
      delaying it.
    - Devices made 114,072 moves as islands, hearing no hub, in every run: no hub is heard until
      the first claim, three seconds in.
    - Devices made 47,181 requests for orders; hubs granted 39,794 and refused 1,352 for a moved
      lease, 7,780 for a payment in progress and 1 for the device owning the order already. 4,999
      overrides applied and 175 were stale; 1,410 grants were stale; 2,598 events were recorded
      by a device that didn't own the order.
    - The runs met 683,310 events appended; 17 million frames, of which 973,000 were lost,
      942,000 duplicated, 950,000 cut off and 287,000 sent to a node that was down; 3,472 crashes
      between writes, 3,517 in the middle of one, 3,711 rollbacks, 7,284 clock jumps and 7,575
      cuts; 2,722 events lost to rollbacks, held by no other replica, and 213 devices forked;
      20,718 writes held back until a log settled. 391,027 records that count numbered 734,885
      events, and replicas sent 4.4 million `durable` frames.
    - The replicas agreed after healing within 0.8 s at the median, 1.6 s at the 90th
      percentile, 2.4 s at the 99th, and 6.5 s at most.
  - Failover: in 1,000 seeds without other faults, the hub crashing at moments spread over a
    heartbeat period, the standby claimed 3.0 to 4.0 s after the crash, spread evenly, a quarter
    in each 250 ms (before the heartbeat fix, which changes nothing where one hub holds each
    epoch).
  - Coverage probes, all passing:
    - the hub property, over 2,002 cases: chains of two terms or more in 46%, three in 11% and
      four in 1.6%; records that a later claim cut off in 40%, and records of a claim that lost
      in 74%; two claims of one epoch in 80%; a claim refused as behind in 2.3%;
    - the protocol property, over 1,005 cases: more than one claim in 33%, an epoch of 3 or more
      in 6% and of 5 at most, two claims of one epoch in 13%, records cut off in 25% (726 in all),
      a candidate down for seconds in 41%, a peer refusing a replica's log, a fork until it takes
      it, in 54%; clocks jumping in 67%, and two claims of one epoch with them in 9%; another
      heartbeat period than a second in 31%;
    - the simulator, over 300 seeds, before the heartbeat fix: more than one claim in 177, the winning epoch past 1 in
      155 and past 2 in 26; 296 splits; 49 failovers; 6,676 moves made as islands; 2,925
      requests for orders, 348 overrides and 87 stale grants; 24,208 records counted; 18 devices
      forked.
  - Planted bugs:
    - `keel-domain`: the 34 new ones, all caught by the property tests; slice 3's 50 of
      ownership, run again against this slice's changes, 49 caught and the one left to a unit
      test; and the six that had gone stale, re-texted, all caught.
    - `keel-store`: the 43 new or changed ones, 31 caught by the property tests and 12 by unit
      tests alone; of the 151 others, the 93 in files this slice changed, run again against its
      property tests: 79 so far, 65 caught and 14 left to unit tests, the rest under way.
    - `keel-sync`: 113, 32 of them left to unit tests, each with the test that catches it. The
      final runs, against the property tests and, for those 32, all the tests, are under way;
      the first runs' misses are under "Found and fixed".
  - A CI-equivalent run passed every step before the last property and heartbeat changes:
    formatting, the planted bugs' texts, both lint runs, the tests with the exhaustive sweeps and
    4,096 cases a property (41 minutes alongside other runs), the tests without default features
    (37), the docs, the `wasm32` build, the currency table and the golden baskets. On the final
    code, formatting, the bugs' texts, both lint runs and the tests pass; CI runs the rest.
- **Decisions:** [ADR-0022](./adr/0022-hub-election-and-failover.md), proposed, with what
  building it settled under "As built": a claimant holds the whole chain; the hub's beat in place
  of "hears the hub directly", and a heartbeat naming its hub's term, beats compared only within
  one term; learning of a claim counts as hearing its hub; a peer's latest
  heartbeat acting as the hub kept; catching up on what peers other than the old hub hold; forks
  told by a peer refusing the replica's own log; priority 0 while unsettled or forked; and every
  pair of candidates within two hops.
- **Found and fixed during the build:**
  - The design let a successor cut only the devices it held claims of. With a gap in its chain,
    a claim of an earlier term it didn't hold could make records count again that a later claim
    had cut off; so could a field saying how far back its chain went. A claimant now holds the
    whole chain (the hub property found both).
  - A hub elected again numbered from after its latest record that counted: a later claim may
    cut off records written together, and their numbers with them, so it now numbers from after
    its latest record of its epoch (the hub property).
  - The design's heartbeat state "hears the hub directly", which a peer repeats for three periods
    after the hub falls silent, would have let a standby take over three periods late. Counting
    how many periods ago a peer heard the hub was a period out either way; the hub's beat replaced
    both (the protocol property).
  - A deposed hub whose epoch a new claim had just raised heard no hub of it, and claimed again
    at once: two candidates took the role from each other every second. Learning of a claim now
    counts as hearing its hub (the protocol property).
  - A hub restarting says it isn't acting until its log settles, which overwrote its last acting
    heartbeat, and it was taken for lost. Each peer's latest acting heartbeat is kept.
  - The design's fork test, a peer refusing its own log, was the wrong way round, and a refusal,
    once noted, never ended; a hub that began to serve again didn't number what came meanwhile;
    and replicas were taken to agree before the hub's beats had reached them all (the protocol
    property; each a named regression test).
  - The simulator missed a fork when the device's log had settled against the peers that
    answered (seed 104), and checked numbering over a forked log and orders only another version
    of it held (seeds 455 and 465): it now tells forks by what the other replicas hold, and leaves
    forked logs and their orders out of those checks.
  - Once its clocks jumped, the protocol property found two hubs of one epoch, which a split had
    left, masking each other: beats compared by epoch and number made the losing hub's, its clock
    3.8 s ahead, look newer than the winner's, and a replica that had both took its hub for silent
    and claimed again, four periods after the replicas had agreed. A heartbeat now names its hub's
    term, beats are compared only within one term, and the property checks every replica's
    hearing against the rule at every step, not only the claims it leads to.
  - Once crashes stopped falling at the same moment of a heartbeat period, a standby waited on the
    crashed hub's last word of its own log, and claimed 7 s after the crash, past the 5 s the
    design allows (the simulator). A candidate now waits only on what its other peers hold.
  - The first runs of `keel-sync`'s planted bugs missed four of 106:
    - a log settling while a peer held more of it: the protocol property took a replica's word
      that its log was settled. It now models settling, and the simulator checks that no log
      settles while a peer it heard from holds more of it;
    - the next tick forgetting rounds: heartbeats tick every replica each period, and the
      property's fell due with its rounds, so no round was ever late. A third of its cases now
      have another heartbeat period;
    - periods counted off the clock: the property's clocks never jumped. They now jump forwards
      now and then, by up to half a minute;
    - a heartbeat sent as another kind of frame: both ends read the kind they write, so only the
      pinned bytes can tell, and it is left to them.
  - Six of `keel-domain`'s planted bugs had gone stale in slice 3, when the commands they plant
    in changed, unnoticed because only that slice's own bugs ran. They plant again, all caught,
    and CI now checks every list (`--stale`).

### Ownership leases: step 6, slice 3 (2026-10-01)

Commit `710ed27`, with CI green on it (33.6 minutes). Reviewed 2026-10-01.

- **Built** ([ADR-0021](./adr/0021-ownership-leases.md)):
  - `keel-domain`:
    - Four order events, version 1: `order.ownership_requested`, `_granted`, `_refused` and
      `_overridden`. `Lease` counts an order's changes of owner, from 0; `Epoch` is the hub's.
    - The order's fold keeps its owner, lease and waiting requests. The creator owns the order
      under lease 0; a grant or an override applies only from the current lease. An answer
      names its request wherever it folds. Four new conflicts: `NotOwner`, `StaleGrant`,
      `Overridden` and `StaleOverride`.
    - `Order::decide` takes the device: commands that need ownership are refused to any other
      device, `NotOwner`, and requests and overrides are commands of their own. Checkout's
      `start_payment` and `close_check` need ownership too.
    - `Order::answers(paying, epoch)`: the hub's grants and refusals, a pure function of the
      order and whether a payment of it is in progress.
  - `keel-store`: the orders projection, version 2, with each order's owner, lease and waiting
    requests, and `Store::answer_requests(epoch, now)`, the hub's answers in a write of their
    own, each caused by the request it answers.
  - `keel-sync`: `Replica::answer_requests`. The hub answers whenever it would sequence, then
    numbers its answers with everything else.
  - `keel-sim`:
    - A move that needs an order another device owns becomes a request, or, from a device cut
      off from the hub, an override. Devices also ask for each other's orders, and half the
      moves on other orders go to the latest two, so that devices contend for them.
    - Three devices in half the runs, up from three in ten.
    - Four new invariants, checked at the end of each run, and the hub's answers and the
      orders' ownership conflicts counted in its report.
  - CI: the job's time limit rises from 30 minutes to 45. Slice 2's commit took 28.6 of them,
    in two test passes of 14 minutes each, and this slice's tests add to both: its own commit
    took 33.6.
- **Verified:**
  - Known answers:
    - `keel-domain`, 14 tests of ownership:
      - the four payloads pinned byte for byte, in payloads Python's `cbor2` encoded from the
        key tables, and each way each is refused;
      - the fold: the creator's ownership, a grant, a stale grant, a refusal, an override and a
        stale one, every owner-only event recorded by another device, an answer before its
        request, and one before the order's creation;
      - each command's ownership check, requests one at a time, and a closed order asked for;
      - each of the hub's answers;
      - leases and epochs in range.
    - Checkout: only the owner starts a payment or closes a check, and a payment's outcome is
      recorded whoever owns the order by then.
    - `keel-store`, 5 tests: the hub granting, every store seeing the new owner; a payment in
      flight holding the order where an authorized one doesn't; two requests from one lease;
      a request waiting for its order's creation; an interrupted answering leaving nothing.
    - `keel-sync`: a hub, in epoch 2, answering a request it receives and numbering its answer
      in the same pass.
  - Property tests:
    - `keel-domain`:
      - the payloads against the codec's model, with a property of their own changing one field
        at a time, and a sweep of every lease and epoch across the ends of its range;
      - the fold against a model of ownership, over requests, grants, refusals and overrides
        from three devices and the hub in any canonical order, sometimes before the order's
        creation;
      - the hub's answers against a model of their own, the grants a chain from the owner, with
        a payment in progress or not; folded after everything, they leave no request waiting;
      - commands tried from the owner, from each device whose request waits, and from a device
        with none, on every state merged orders pass through; a device asking twice, then
        overriding, with the requests it leaves and their leases the model's;
      - checkout's decisions for a device that doesn't own the order.
    - `keel-store`: a hub in an epoch of its own takes in two devices' logs a few events at a
      time, asks for orders itself, and answers, sometimes interrupted. Each answer must be the
      order's rules applied to what it holds; at the end every request has one answer, and
      another replica, taking every log in any order, agrees.
    - `keel-sync`: the protocol property, its model replica asserting that the hub answers
      before it numbers, every time.
    - 100,000 cases each passed, in release builds: `keel-domain`'s payload properties in
      112 s, its order properties in 211 s and checkout's in 85 s; the store's ownership
      property in 40 minutes and its projection properties in 29; and the protocol property in
      11 minutes.
  - The simulator: 5,000 seeds passed in a release build, in 46 minutes, alongside other runs,
    three devices in 2,438 of them.
    - Devices made 43,783 requests and 3,173 overrides. The hub held 43,219 of the requests,
      the rest lost to rollbacks, and answered each once: 34,975 grants, 1,205 refusals of a
      moved lease, 7,039 of a payment in progress, and none of an owner, which the simulator's
      star rules out.
    - In the orders' folds on the hub: 2,783 overrides applied, in 1,812 seeds; 192 stale
      overrides, in 177; 243 stale grants, in 221, each after an override of the lease it
      replaced; and 3,298 events recorded without ownership, in 1,617.
    - The runs met what slice 2's did: 672,198 events appended; 5.2 million frames, of which
      265,000 were lost, 287,000 duplicated, 124,000 cut off and 71,000 sent to a node that was
      down; 3,530 crashes between writes, 3,422 in the middle of one, 3,633 rollbacks, 7,526
      clock jumps and 7,575 cuts; 5,159 events lost to rollbacks, held by no other replica, and
      528 devices forked. The hub wrote 406,327 records, numbering 700,104 events, and replicas
      sent 1.1 million `durable` frames.
    - The replicas agreed after healing within 1.5 s at the median, 2.1 s at the 90th
      percentile, 5.1 s at the 99th, and 10.1 s at most.
  - Coverage probes, all passing:
    - the fold property, over 1,000 cases: stale grants in 81%, applied overrides in 40%, stale
      overrides in 73%, events flagged `NotOwner` in 76%, an answer before its request in 71%,
      leases of 2 or more in 41%; the hub's answers refused a moved lease in 46%, an owner in
      7% and a payment in progress in 15%, granted in 15%, and granted twice in 1.8%;
    - commands, over 1,000 merged orders: a request from another device waited in every one,
      and in 69% of the 22,500 states tried; the device carrying on had its first request
      accepted and its second refused as pending in 649, the rest ended orders;
    - checkout: of 7,206 payments a device that didn't own the order tried to start, 75% were
      refused for that, and 36% of 9,430 closes; the others failed earlier rules first;
    - the store property, over 1,000 cases: 4,053 answering passes, 1,789 with answers: 553
      grants, 3,009 refusals of a moved lease, 647 of an owner and 355 of a payment in
      progress; 760 passes answered both orders, 67 granted twice; 993 answers were
      interrupted, 325 with answers to lose; 505 of 3,000 passes held a request for an order
      not yet created.
  - Planted bugs, all caught: 51 in `keel-domain`, 15 in `keel-store` and 4 in `keel-sync`.
    The property tests alone catch all but two: no fold reaches the largest lease, after 2^63 − 1
    changes of owner; and every simulated hub is in epoch 1, so only the known answer, in epoch
    2, sees a store answering in the wrong one. Of `keel-sync`'s other three, the simulator
    catches all three and the protocol property two: the third is the store's, which its model
    replica stands in for.
  - A CI-equivalent run passed every step, in 34 minutes alongside the soaks: formatting, both
    lint runs, the tests with the exhaustive sweeps and 4,096 cases a property (17 minutes), the
    tests without default features (16), the docs, the `wasm32` build, the currency table and
    the golden baskets.
- **Decisions:** [ADR-0021](./adr/0021-ownership-leases.md), accepted 2026-10-01 after review,
  with what the build settled under "As built":
  - refusals coded from 0, as every code is, and an override's reason with an optional note;
  - an answer names its request wherever it folds, even before the order's creation;
  - the order of the checks: the order's state, then ownership, then the command's own rules,
    and the hub's lease, then device, then payment;
  - the hub's answers recorded by `System("hub")`, caused by their requests, under the order's
    business date;
  - the simulator's devices contending for the latest orders, and three of them in half the
    runs.
- **Found and fixed during the build:**
  - The design assumed a request folds before its answer. A store keeps an event from a device
    whose clock is too far ahead without moving its own clock past it, so the hub's answer can
    sort first: the request would have waited for ever, and the hub answered it at every write.
    The fold now remembers every request an answer names.
  - Reviewing the fold found the same with an answer before the order's creation, which the
    fold sets aside: a hub more than a minute behind two devices would answer the same request
    at every write. Answers are now noted before anything else.
  - Writing the planted bugs showed that `keel-store`'s property uses the order's own rules as
    its model of the hub's answers, so no property checked the rules themselves.
    `keel-domain`'s ownership property now has a model of the answers.
  - The first run of the planted bugs missed nine of 50, in five places the property tests
    didn't reach:
    - a grant's lease or epoch of 0 came up once in thousands of cases: a payload property for
      the four schemas, and a sweep of their numbers, now reach them every time;
    - every racing device overrode the order first, so no merged order had a request waiting:
      one device in two now asks first, commands are tried from each device whose request
      waits, and the device carrying on asks twice before it overrides;
    - no command anyone may make was tried from another device;
    - requests were compared by device, not by the lease they name;
    - checkout's model ignored ownership.
  - The first simulator runs never met the race that makes a grant stale, in 64 seeds: devices
    spread their moves over too many orders, and three devices were too rare. Moves on other
    orders now go to the latest two half the time, an island always overrides, and three
    devices run in half the seeds: 12 stale grants in 256 seeds.

### Hub sequencing: step 6, slice 2 (2026-09-30)

Commit `7b6bd52`, with CI green on it (28.6 minutes). Reviewed 2026-10-01.

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
- **Decisions:** [ADR-0020](./adr/0020-hub-sequencing.md), accepted 2026-10-01 after review,
  with what the build settled under "As built":
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
- **`keel-sync`:**
  - Every pair of candidates for the hub must be within two hops of each other, or two of them
    take the role from each other in turn (ADR-0022): the location's links, which discovery will
    make, must keep them so. Nothing checks it as the replicas run.
  - Without a quorum, a split store runs a hub on each side. When it heals, the losing side's
    records stop counting and its events are numbered again: the shells must show that numbers
    shown as confirmed there have changed (step 7).
  - Batching several writes' events into one record, if the records prove too many: when events
    arrive one at a time, the store holds about as many records as events.
  - Devices send the watermark back to the hub with each round: redundant, and harmless.
  - Pruning below the watermark, with retention.
  - Showing what is provisional and what isn't yet backed up, in the shells (step 7).
  - The transport (offline-and-sync §3.2–3.3): WebSocket over TLS, discovery, scopes, device
    certificates, protocol negotiation, priority lanes, compression and reconnection backoff.
  - A device restored from an older copy that must sell before any peer answers forks its log.
    The replicas report the fork, and keep disagreeing about that log until a person resolves
    it, with tools that don't exist yet. A forked candidate never claims, and a forked hub stops
    serving.
  - A batch waiting for acknowledgement from a peer that has since restarted holds up the next
    for up to the acknowledgement timeout (2 s).
  - A clock set back less than a round delays the rounds by as much. Timers on a monotonic clock
    would need a second kind of time passed in, beside the clock the store stamps events with.
- **Ownership** (ADR-0021, decision 8):
  - The hub taking an order back from a device it hasn't heard from in a while: until then an
    order whose owner died in the middle of a payment can be taken only by override. Heartbeats
    now tell a device that it is an island, and the simulator's devices override only then.
  - A hub whose clock is more than a minute behind the devices can't give their new orders
    away: its grants sort before the orders' creation, where they don't apply. The hub's beat,
    its clock's reading, could tell the devices.
  - Who may override: a manager, checked with `keel-policy`.
  - The owner handing an order to another device without that device asking.
  - A payment started by a device that didn't own its order is flagged by the device's check
    and checkout's issues, not in the payment's own fold, which can't see the order.
  - Other leases: tables, store-wide order numbers, limited quantities (offline-and-sync §6.3).
  - The simulator's hub seldom refuses a request from the device that owns the order, once in
    5,000 seeds: the known answers and the property tests reach that refusal.
- **`keel-sim`:**
  - Disk faults, torn writes and lost fsyncs, wait for a simulated disk: a SQLite VFS, which
    needs `unsafe` code.
  - The invariants of later features: payments, the ledger and fiscal chains
    (offline-and-sync §12).
  - Its locations have two candidates for the hub, which every device links to: more candidates,
    and devices linked to only one, only the protocol property reaches.
- **Orders:**
  - Permissions and approvals aren't checked yet (`keel-policy`).
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
