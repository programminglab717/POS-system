# ADR-0022: Hub election and failover: hub terms claimed in the log, heartbeats and priorities, and a deposed hub's records fenced by its successor's claim

- **Status:** Proposed
- **Date:** 2026-10-01

## Context

Slice 4 of step 6 is hub election and failover
([ADR-0019](./0019-replication-and-deterministic-simulation.md), decision 8): "priorities,
heartbeats, the hot standby, epochs and fencing, and a split brain healing".

The design fixes the purpose ([offline-and-sync.md §2, §7](../architecture/offline-and-sync.md#7-hub-election-and-failure-handling),
[ADR-0001](./0001-local-first-three-tier-topology.md), [ADR-0006](./0006-own-sync-protocol.md),
decision 5):

- The Store Hub is a role, not a box. The highest-priority reachable eligible device holds it:
  a hub appliance first, then wired and powered devices by configured priority. Browsers never.
- Each hub term carries an **epoch** that only increases, persisted and synced.
- Heartbeats run every second. The **hot standby**, the next in priority, detects the hub's loss
  after three missed heartbeats and takes over in under 5 s. Selling never pauses meanwhile:
  devices keep working, and their events are sequenced once the new hub serves.
- No state passes from hub to hub: every replica already holds the location's events.
- **Fencing:** writes stamped with a deposed hub's epoch, its sequencing records and lease
  grants, stop counting once a newer epoch is known. Events the old hub sequenced but never
  replicated are sequenced again by the new hub. Facts stay; only confirmation changes.
- A **split brain**, a store's network cut in two, can give two hubs. Data is safe, since events
  are facts and replication merges. When it heals, one hub steps down, and what it sequenced
  is sequenced again.

What slices 1 to 3 built shapes the rest:

- The hub's decisions are signed events in its own log, which replicate like any other: its
  sequencing records ([ADR-0020](./0020-hub-sequencing.md)) and its answers to requests for
  orders ([ADR-0021](./0021-ownership-leases.md)). Each names an epoch, which has been 1, from
  the hub's configuration.
- Confirmation is worked out from the records a replica holds, as the store is read, never
  kept. Until this slice, a record from any device enrolled at the location counts
  (ADR-0020, decision 4).
- The replicator is sans I/O: the caller hands it frames and ticks, and sends what it returns.
  Its rounds run on the clock the caller passes in, which may jump.
- A replica settles its own log after a start against the first peer that answers (ADR-0019,
  decision 4). With one peer, as every device has had, that is all its peers. A device with two,
  the hub and a standby, could settle against the one holding less of its log, and fork it.
- An order's fold applies a grant only from the order's current lease (ADR-0021), so two hubs'
  grants of one lease never both apply.

## Decision

1. **A hub's term is a claim in its log.** A replica that becomes hub first records a
   `hub.claimed` event: its epoch, its priority, the claim it succeeds, and how far into the logs
   of the earlier hubs it holds. Claims replicate like any other event, so every replica learns
   of every term.

   | Key | Field | Rules |
   |---|---|---|
   | 1 | epoch | from 1 to 2^63 − 1; 1 exactly when it succeeds no claim |
   | 2 | priority | the claimant's, from 1 to 255; higher is preferred |
   | 3 | previous | the claim it succeeds, an event identifier; absent for epoch 1 |
   | 4 | cuts | for each device that held a term on the chain it succeeds, `[device, position]`: how far into that device's log the claimant held; sorted by device, positions from 1, at most 1,024; absent for epoch 1 |

   A new schema, version 1, on a `hub` stream of its own, strict and pinned byte for byte
   ([ADR-0013](./0013-event-payloads-and-schema-evolution.md)), recorded by the claimant as
   `System("hub")`.
2. **Every replica works out the same hub from the claims it holds.**
   - The **winning claim** has the highest epoch; between claims of one epoch, the higher
     priority, then the lower device identifier, wins.
   - The **chain** runs from the winning claim to the claim it succeeds, and on, as far as the
     replica holds them, each with a lower epoch than the one after it.
   - The **hub** is the winning claim's device, and the store's **epoch** is its epoch. Two
     replicas holding the same claims agree on both, whatever order the claims came in.
3. **A successor's claim fences what the hub it succeeds wrote after it.** A sequencing record
   counts only if:
   - its device holds a term on the chain for the record's epoch, and the record follows that
     claim in the device's log; and
   - it lies within the term's **cut**: no further into its device's log than any later claim on
     the chain says it held of that device.

   So a deposed hub's records that its successor didn't hold when it claimed never count,
   anywhere: neither those it wrote, partitioned, after its successor claimed, nor those it wrote
   just before failing that never left it. Their events are numbered again by a later hub.
   Every claimant holds every record that still counts, so no event gets two numbers, and each
   epoch's numbers stay gapless, ending where its successor cut them. Confirmation, the feed and
   sequencing count only the records that count. A replica that holds a claim but not the claim
   it succeeds follows the chain only as far as it holds it, so confirmation, as before, depends
   only on what the replica holds.
4. **Grants aren't fenced.** An order's fold can't see the hub's claims, which are other
   streams, and needn't: it applies a grant only from the order's current lease, so a deposed
   hub's grant takes an order only where no other change of owner came first, as two
   concurrent overrides would. A request a deposed hub answered past its cut is pending in its
   successor's view and is answered again; the fold applies the first answer, and the second
   changes nothing or is flagged stale. A grant's epoch stays, for audit.
5. **Election by priority and heartbeats.**
   - Every replica sends each peer a `heartbeat` frame every second:
     `[1, 3, location, priority, epoch, state]`. The priority is the replica's as hub, 0 if it
     can never be. The epoch is its winning claim's, 0 for none. The state says whether it is
     that term's hub, acting as one (2); hears that hub directly (1); or neither (0).
   - A replica hears its hub while it has had, in the last three heartbeat periods, a heartbeat
     of state 2 from a replica of its epoch or a later one, or of state 1 from a peer that hears
     such a hub directly. Hearing is never relayed further, so a dead hub can't be kept alive by
     replicas vouching for each other.
   - An eligible replica claims the next epoch when all of these hold:
     - its log is settled, and no peer has refused its own log, which would mean it forked;
     - it hears no hub: none has claimed yet, or its hub has been silent;
     - in the last three periods it has heard no peer that would be preferred as hub: a higher
       priority, or an equal one and a lower identifier;
     - it holds as much of each earlier hub's log as its peers say they hold, so that its cut
       fences nothing they have; or three more periods have passed.
   - Periods are counted as the replicator's own heartbeat ticks, not read off the clock, so a
     clock jumping forwards can't make a hub look silent.
   - The hot standby is no separate role: it is the most preferred live candidate, which claims
     first. Without one, the next does, a little later.
6. **A hub serves only while its term wins.** A replica answers requests and sequences only while
   its log is settled and it holds the winning claim. The moment a better claim reaches it, it
   stops, without a frame of its own: its records from then on are past its successor's cut. A
   hub that restarts resumes its term once settled, if no one has claimed since.
7. **Settling against every peer.** A replica's own log is settled once every peer that has said
   what it holds since the start holds no more of that log than the replica, at least one has
   said so, and either all have, or a heartbeat period has passed since the first did. A peer
   that can't be reached can't help, so a replica restored from an older copy can still fork its
   log if the only peer holding the rest is down; it then never claims.
8. **Islands, told by heartbeats.** `Replicator::hub_reachable()` says whether the replica hears
   its hub. The shell shows "working independently" when it doesn't, and the simulator's devices
   take orders on a manager's word only then, instead of the simulator deciding from its
   partitions (ADR-0021, decision 6).
9. **The store and the replicator.**
   - A `terms` projection keeps every claim and its cuts. `Store::term()` gives the winning
     claim: its epoch, its device and its identifier.
   - `Store::claim(priority, now)` writes the next claim, succeeding the winning one, its cuts
     taken from what the store holds.
   - `Store::sequence(now)` and `Store::answer_requests(now)` take their epoch from the store's
     own winning claim, and refuse, `StoreError::NotHub`, if it doesn't hold it.
   - `Roles` gives a replica's priority as hub, `None` if it can never be, in place of the
     sequencer's fixed epoch. `SyncConfig` gives the heartbeat period, 1 s, and the periods of
     silence before a hub counts as lost, 3. `Replicator::is_hub()` and `term()` say what the
     replicator last read.
10. **The simulator.**
    - **Nodes:** the devices; the hub, priority 2, and a standby, priority 1; and the cloud,
      never hub. Each device links to the hub and the standby, and each of those two to the
      other and to the cloud. No one holds a term at the start: the hub claims epoch 1.
    - **Faults** as before, the hub and the standby crashing too, and **splits**: the store's
      network cut in two for a while, each device on one side, the cloud reaching one side, both
      or neither. Stores restored from an older copy stay devices' only.
    - **New invariants**, checked at the end of each run:
      - every replica holds the same winning claim, and its device is the only one acting as
        hub;
      - on the winning chain, every event but the records is numbered exactly once by the
        records that count, each epoch's numbers gapless from 1, and each device's log in
        order; every replica confirms each event so;
      - a replica writes records and answers only in the term it holds, as it holds it;
      - every request has an answer, and no term answers one twice;
      - a stale grant follows an override, or another term's grant, of the lease it replaced.
    - Without other faults, a hub that crashes is succeeded within 5 s.
11. **Out of scope until later:**
    - a hub-eligible replica restored from an older copy, in the simulator: the protocol's rules
      apply to it as to any replica (decisions 5 and 7);
    - the hub taking an order back from an owner it hasn't heard from, which needs a policy for
      payments in flight;
    - devices taking their time from the hub, and telling a hub whose clock is far behind;
    - the cloud's ingestion of an epoch whose end a successor cut;
    - the priorities as location configuration from the cloud, with the hub's eligibility
      rules (offline-and-sync §7); here each replica is given its own;
    - a quorum: elections here favor availability, and heal split brains afterwards.
12. **Verification**, for slice 4:
    - known answers:
      - the claim's payload pinned byte for byte, checked with Python's `cbor2`, and each way it
        is refused; the heartbeat frame pinned, and refused;
      - the winning claim, the chain and the cuts, and which records count, as claims arrive in
        any order: a failover, a split brain, a hub elected twice, a claim whose predecessor
        isn't held;
      - the store claiming, sequencing and answering as the hub, and refusing when it isn't;
      - the election, frame by frame: the hub's first claim, the standby taking over after three
        missed heartbeats and not before, deferring to a preferred candidate, hearing the hub
        through a peer, waiting to catch up, stepping down, resuming after a restart, and
        settling against every peer;
    - property tests:
      - the payload against the codec's model, as for every schema;
      - the store's terms and confirmation against a model, over claims and records of several
        hubs reaching replicas in different orders: what counts, gapless numbers, and replicas
        agreeing;
      - the protocol property with several eligible replicas and the model replica, through
        partitions and crashes: one hub at the end, every replica agreeing on it;
    - the simulator, with the new invariants, and a calm run with a hub crash;
    - planted bugs in `keel-domain`, `keel-store` and `keel-sync`, and coverage probes.

## As built

Details settled in building it, for review with it:

- **A claimant holds the whole chain** (amends decision 3). A successor's cuts fence a deposed
  hub's records only if the successor held, when it claimed, everything that still counted: with
  a gap in its chain, a claim of an earlier term it didn't hold could make records count again
  that a later claim had cut off. A replica therefore claims only when it holds the chain back to
  a claim of epoch 1, every claim on it, and each term's device's log as far as the term's cut;
  otherwise its store refuses, `StoreError::Behind` (`ClaimError::Behind` in `keel-domain`), and
  the replicator tries again each period. The cost is liveness: a replica missing part of the
  chain waits to catch up before it can claim. Cutting only the devices it held claims of, and a
  field saying how far back its chain went, were tried first: both left holes no store could find
  cheaply.
- **The chain:** a claim's predecessor is followed only while it is held and of a lower epoch. A
  term's cut is the least any later claim on the chain gives for its device; a later claim that
  doesn't cut the device, which no claimant that holds the chain writes, cuts it at 0, so none of
  its records count.
- **The claim** is numbered like any event. It isn't a business event, so it is filed under the
  business date of the latest event the store holds, or the UTC date when it holds none.
- **`keel-store`:**
  - The claims projection, version 1, on streams of kind `hub`, keeps a row for each claim and the
    chain they make, worked out again from every claim at each write that touches one; the
    integrity check compares the chain kept with the one the claims make, `Problem::Terms`.
  - A record counts when its device holds the term of its epoch on the chain, and it follows the
    term's claim and lies within its cut. Confirmation, the feed and sequencing count no others.
  - For each device, sequencing numbers what follows the last position a record that counts
    covers. For the hub's own log, it also starts after the hub's latest record of its current
    epoch, not its latest that counts: a later claim may cut off records written together, and
    their numbers with them, so a hub elected again numbers anew what only a record past its old
    cut numbered. The property found this.
  - `Store::term()`, `terms()` and `claim(priority, now)`, and `StoreError::NotHub` and `Behind`,
    as decision 9 says.
- **The heartbeat** is `[1, 3, location, priority, epoch, hub, acting, beat]` (amends decision
  5).
  - `epoch` and `hub` are the sender's term: the epoch of the winning claim it holds and the
    claimant's device, or 0 and `null` if it holds none. `acting` says the sender is that hub.
    `beat` is the hub's beat, a number the hub raises with every period it acts, its clock's
    reading in microseconds or one past its last beat if that is later: the sender's own, if it
    acts, else the latest it had from its term's hub directly, while recent; `null` if none.
  - A replica hears its hub while the latest beat of its own term's hub, or of the hub of any
    later epoch, first reached it in the current period or the three before, from the hub or
    from a peer that had it directly; or it learned of the winning claim, another replica's, as
    recently.
  - Beats are compared only within one hub's term. A split leaves two hubs of one epoch, and
    their beats, read off two clocks, say nothing of each other: compared by epoch and number
    alone, the losing hub's, its clock ahead, made the winner's look no newer, and a replica that
    had both took its hub for silent and claimed again. The protocol property found it once its
    clocks jumped; the heartbeat first gave only the epoch.
  - Beats replace the design's state 1, "hears that hub directly", which a peer would repeat for
    three periods after the hub fell silent: a standby hearing a dead hub through a device would
    have taken over up to three periods late, missing 5 s. A peer that passes on a beat a replica
    already had directly makes it no newer, so hearing through a peer never outlasts hearing
    directly. A replica hearing the hub only through peers hears each beat up to a period late,
    and counts the hub lost up to a period later. Counting how many periods ago a peer heard the
    hub was tried first: periods that begin at different moments made that a period out either
    way, and the protocol property found both errors.
  - Learning of a claim counts as hearing its hub because a claim travels faster than the beats of
    its hub, which a peer passes on only as its next period begins. Without it, a deposed hub
    whose epoch the claim had just raised heard no hub of it, and claimed the next epoch at once:
    the property found two candidates taking the role from each other every second.
  - Something heard is recent in the period it came in and the three after: the hub counts as lost
    as the period after its third missed heartbeat begins. In the simulator, without other
    faults, the standby claims within 3 to 4 s of the hub's crash.
  - A replica keeps each peer's latest heartbeat acting as the hub, not only its last heartbeat:
    a hub that restarts says it isn't acting until its log settles again, and mustn't be taken
    for lost meanwhile.
- **Catching up** (amends decision 5): a candidate waits only for as much of each earlier hub's
  log as its peers other than that hub say they hold. What a hub it no longer hears said of its
  own log, the candidate could only have from that hub, and a hub that spoke again would be heard
  and not succeeded, so waiting for it only ever ended once the three more periods had passed.
  The simulator found this when crashes stopped falling at the same moment of a heartbeat period:
  a standby waited on the last have of the hub that had crashed, and claimed 7 s after the crash.
- **Forks** (amends decision 5). A replica's log has forked when a peer takes none of a batch of
  the replica's own log that began just after what the peer held of it, until the peer says it
  holds the log that far: a peer that lacks earlier events isn't refusing the log.
  `Replicator::forked()` says so. The design's test, a peer refusing its own log, was the wrong
  way round: it made a replica whose log had forked go on giving its priority, so that every
  other candidate deferred to it for good.
- **Priority 0** while a replica's log is unsettled or forked, as well as when it can never be the
  hub: no candidate defers to a replica that can't be the hub now.
- **Serving** is holding the winning claim with a settled log that hasn't forked. A hub whose log
  forked stops serving: its records would never replicate. A replica answers and numbers as it
  begins to serve, whatever made it begin, and after every write that stores events.
- **`keel-sync`** depends on `keel-domain`, for the chain of terms. `Replica` gains `terms()`,
  `claim(priority, now)`, which says the claim was made, the replica already holds the winning
  claim, or its store is behind, and `answer_requests(now)` and `sequence(now)` without an epoch.
  `Replicator` gains `is_hub()`, `serving()`, `term()`, `hub_reachable()` and `forked()`, and
  `Stats` counts claims.
- **Candidates must hear each other.** Hearing goes one hop through a peer, no further, so two
  candidates that are neither linked nor share a peer never hear each other's beats, and take the
  role from each other in turn. A location's links must keep every pair of candidates within two
  hops: the hub and its standby link to each other, and in the protocol property every pair of
  candidates is within two hops.
- **The simulator** detects a device's fork by what the other replicas hold: a device restored
  from an older copy that writes before it holds its log back as far as another replica does
  forks it, whether or not its log had settled, since settling against the peers that answer
  can't see a peer that can't be reached (decision 7). A forked device's log, and the orders it
  wrote to, are left out of the checks of numbering and answers, as of agreement before. It
  checks as the run goes that a candidate writes records and answers only while it serves, in
  the epoch of the term it holds, and that a replica's log settles only when no peer it has heard
  from holds more of it; and at the end that the answers to each request come from no term
  twice.

## Consequences

**Positive**

- A hub's term, and how it changed hands, is in the location's history: signed, replicated,
  auditable, and the same on every replica once they hold the same claims.
- Failover needs no state handed over and no quorum: the next hub claims, numbers what is left,
  and answers what waits, in about 3 s.
- Fencing is a prefix of each deposed hub's log, decided by its successor's own claim: every
  replica works out the same, numbers never repeat, and each epoch's feed stays gapless.
- A split brain heals by the same rules as a failover: the losing hub's later records stop
  counting, and the winner numbers their events.
- Devices know when they are islands from the protocol itself.

**Negative**

- Confirmations a deposed hub gave past its successor's cut are withdrawn when the claim
  arrives, then given again by the new hub: a device on the losing side of a split sees
  confirmed events turn provisional for a moment. A store whose peers hold more of the old hub's
  log than the new hub when it claims would see the same; the new hub's wait to catch up makes
  that rare.
- A deposed hub's answers stand: a request can get two, and a grant from a hub that didn't know
  it was deposed can win the order. The lease's compare-and-swap keeps it consistent, and the
  flags show it.
- Heartbeats cost a small frame a second on every link.
- Without a quorum, a store cut in two runs two hubs until it heals.
- Every replica must be told its own priority until the location's configuration carries them.

## Alternatives considered

| Alternative | Why not |
|---|---|
| A quorum election, as Raft's | A store's devices come and go, and a split store must keep a hub on each side: the design favors selling over agreement, and heals afterwards. A quorum would leave a side without a hub. |
| Epochs as frames or configuration, with no claim in the log | Unverifiable and lost with the hub; every replica must agree on which hub's records count, so terms must replicate like the records they fence. |
| Fencing a deposed hub's records by the time its successor claimed | Clocks on two sides of a split say nothing about what each side knew. A position in the deposed hub's log, held by its successor, does. |
| The first number of an event, by epoch and number, counting (ADR-0020's rule) | An old hub's records arriving late would take events from the new hub's feed, leaving gaps in it, and change numbers already given. Cutting the old hub's log keeps every number given by a term that counts. |
| Fencing grants in the order's fold | The fold would depend on the hub's claims, in other streams, and every order would fold again when one arrives. The lease's compare-and-swap already keeps two grants of one lease from both applying. |
| A separate standby role, given by configuration | A standby is just the most preferred live candidate; a fixed role would need its own failover. |

## References

- [ADR-0001](./0001-local-first-three-tier-topology.md), [ADR-0006](./0006-own-sync-protocol.md),
  [ADR-0013](./0013-event-payloads-and-schema-evolution.md),
  [ADR-0019](./0019-replication-and-deterministic-simulation.md),
  [ADR-0020](./0020-hub-sequencing.md), [ADR-0021](./0021-ownership-leases.md)
- [offline-and-sync.md §2, §4, §7, §12](../architecture/offline-and-sync.md)
