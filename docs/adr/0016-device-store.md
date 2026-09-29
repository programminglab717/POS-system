# ADR-0016: The device store: SQLite through rusqlite, one transaction per write, crash-tested

- **Status:** Proposed
- **Date:** 2026-09-29

## Context

Every device, hub and standby keeps its events in a local store. The design already fixes much of
it:

- One SQLite database holds the event log, the projections and the outbox, so appending events,
  updating projections and enqueuing effects is one atomic transaction
  ([ADR-0002](./0002-event-sourced-signed-event-log.md),
  [offline-and-sync.md §9](../architecture/offline-and-sync.md#9-storage-retention-and-bootstrap)).
- The database runs in WAL mode, money-affecting commits are fsynced, and it is encrypted at rest
  with a key the platform keystore protects ([security.md §4](../architecture/security.md#4-data-protection)).
- A replica stores events from every device in its scope and keeps a version vector: for each
  device, how far into its log the replica holds. It verifies each received event (signature,
  location, chain, revocation), quarantines what fails, and keeps the first of two events a
  device recorded at the same position
  ([offline-and-sync.md §3.1–3.2](../architecture/offline-and-sync.md#3-replication-protocol)).
- Retention windows, pruning below the cloud's durable-ack watermark, and snapshots at the
  business-day close ([§9](../architecture/offline-and-sync.md#9-storage-retention-and-bootstrap)).
- The kernel's disk I/O sits behind interfaces, so a simulator can crash it mid-transaction and
  tear its writes ([§12](../architecture/offline-and-sync.md#12-verification-deterministic-simulation-testing)).
- `keel-events` appends to a device's log in two steps: the writer makes and signs the next
  event, and makes it the log's head only once the caller has stored it. The log in memory never
  runs ahead of the log in storage.

This leaves the binding to SQLite, and the dependency policy exception it needs; the durability
settings; the schema and how it changes; how a write works; how received events are stored and
read back; how crash safety is tested; and how step 5 is split into slices.

## Decision

1. **SQLite through `rusqlite`, with SQLite compiled in.** `keel-store` uses `rusqlite` with its
   bundled SQLite (3.53.2 today), so every platform runs the same SQLite, whatever its operating
   system ships. `keel-store` is a **platform crate**: it does I/O, links C code, and isn't built
   for `wasm32`, since browsers are clients, never replicas. The dependency policy gains a rule
   for platform crates: they may use a native library that an ADR chooses, and the portable
   kernel crates never depend on them.
2. **Durability settings**, checked when the store opens, which refuses to run without them:
   - the WAL journal;
   - `synchronous = FULL`: every commit is on disk before it returns. The design asks this of
     money-affecting commits. A register writes a few transactions a second at most, so every
     commit pays it, and no commit can pass for durable when it isn't;
   - `trusted_schema = OFF`;
   - one connection writes, and each write starts with `BEGIN IMMEDIATE`, so it never fails
     halfway on a lock.
3. **The schema is versioned** by `PRAGMA user_version` and changed only by forward migrations,
   each in one transaction. A store whose schema is newer than the kernel knows is refused. A
   store belongs to one device and location, and refuses to open for another, or with a key that
   didn't sign the device's stored events.
4. **Events are stored as they were signed**: the COSE_Sign1 bytes, which replicas exchange and
   signatures cover. Columns hold what queries need, taken from the verified body: the origin
   device and sequence number, the event identifier, the hash, the HLC (8 bytes, big-endian, which
   sort like the HLC) and the stream. Reading an event back decodes its bytes without checking the
   signature again: the store checked it before storing it, and checking on every read would cost
   a millisecond or more per order on a low-end device. `keel-events` offers this as
   `SignedEvent::from_stored`, behind a `stored` feature that only the store enables.
5. **A write is one transaction.** `Store::write` runs a closure that appends the device's own
   events, through the store's log writer, and stores received events. They commit together or
   not at all. The appended events leave the store only once the commit has returned, so none is
   sent anywhere before it is durable. The writer's clock is stored with each write, so it resumes
   exactly where it was. If the transaction fails, the writer goes back to the stored head and the
   stored clock, so it is never ahead of storage: an event it made, or an HLC it observed, in a
   write that didn't happen doesn't move its next event's HLC. Later slices add projection
   updates and outbox entries to the same transaction.
6. **Received events** are verified with the device registry, and must be for the store's
   location. Then, by where each fits in its device's log:
   - the next event is stored, and the writer's clock observes its HLC, unless the HLC is beyond
     the drift limit: such an event is stored, but doesn't drag the device's clock along;
   - a duplicate changes nothing;
   - after a gap, nothing is stored, and the caller learns where the log ends, to ask for the
     rest in order;
   - a different event at a position already held (a fork), an event that doesn't link to the one
     before it, and one whose HLC isn't later, are quarantined, and the first event stays. So is
     an event that fails verification.

   The quarantine keeps each distinct message once, with its reason, for a person to look at. The
   version vector is each device's highest stored sequence number, since logs are stored without
   gaps. The device's own events, received from another replica (a restored device catching up),
   extend its log like anyone's, and move its writer forward, clock included: the device's own
   clock issued them, so no drift limit applies.
7. **Crash safety is tested at every point where a write can stop.** The store calls a fault hook
   before a migration commits, after a write begins, after each event is stored or quarantined,
   and before and after the commit. Tests make the hook fail, and the write must roll back and
   leave the store usable. Tests also make it abort a child process, at every point of a workload
   in turn. After each crash the store is reopened and checked: every write that returned is
   there, every write is all or nothing, every log verifies, and the device's writer continues its
   log and clock. Kills at moments spread over the writes add points inside SQLite. Power loss also
   loses what the operating system hadn't written to disk yet, which needs a simulated disk:
   keel-sim's (step 6). Until then, the settings in decision 2 are checked directly.
8. **Step 5 comes in three slices, each ending with a review:**
   1. the event log store: decisions 1 to 7;
   2. projections and the outbox, in the same transaction as the events;
   3. encryption at rest, SQLCipher-class with its key from the platform keystore, and integrity
      checks when the store opens.

   Retention, snapshots and backups follow the sync engine (step 6), which brings the durable-ack
   watermark and the business-day close they depend on.

## As built (slice 1)

Details settled while building the event log store, for review with it:

- **What `keel-events` gained.** `LogWriter::restore(head, latest_hlc)` resets a writer to its
  log as stored, head and clock, as `LogWriter::new` would resume them; `LogWriter::latest_hlc`
  is the clock to store; `LogHead::from_parts` rebuilds a head from its stored columns. The
  property test found the need for the clock: an interrupted write had received the device's own
  event from an hour ahead, and after the rollback the writer's clock stayed an hour ahead.
- **The checks on a received event, in order:** the registry's verification; the store's
  location; a sequence number the store can hold (SQLite's integers are signed, so up to
  2^63 − 1); then where it fits in its device's log. The next event whose identifier the store
  already holds is quarantined. Each refusal has a stable code: `malformed`, `unknown_device`,
  `bad_signature`, `wrong_location`, `revoked`, `rejected`, `other_location`, `fork`,
  `broken_link`, `clock_regressed`, `unlinked`, `duplicate_id` and `out_of_range`. `rejected`
  and `unlinked` catch refusals that `keel-events` may add later; nothing reaches them today.
- **The quarantine** keys each message by its hash, so a message received again keeps its first
  reason, even when its fate has changed since (an event that didn't link becomes a fork once its
  position is filled).
- **Reads:** a device's log from a position, up to a limit; each device's head and the version
  vector; an event by identifier; a stream's events in canonical order, by HLC, then device, then
  sequence number; and the quarantine, in the order it arrived. The stored hash of every event
  read back is checked against its bytes.
- **The fault points** are `Migrating`, `Began`, `Stored`, `Committing` and `Committed`. At
  `Committed` the write has happened, so the store ignores a refusal there; only a crash
  interrupts it.
- **Tests.** The property and crash tests keep their databases in memory (`/dev/shm`) where
  there is one, since a process crash doesn't depend on the disk and the property test commits
  thousands of times. The unit tests check the durability settings as SQLite reports them.

## Consequences

**Positive**

- One tested SQLite everywhere, and atomic writes by construction.
- The store can't announce an event it hasn't stored, and a write that fails or is interrupted
  can't leave the device's writer ahead of its log, where its next event would fork it.
- Crash safety is tested at every point of a write, not assumed.

**Negative**

- A native dependency: C code in the build, a C toolchain for every target (the Android NDK,
  Xcode), and SQLite upgrades that arrive with `rusqlite` releases.
- Every commit waits for the disk.
- Reading events back trusts the store. Encryption at rest (slice 3) and page checksums are the
  defence against tampering with the file.
- Power-loss tests wait for keel-sim.

## Alternatives considered

| Alternative | Why not |
|---|---|
| The operating system's SQLite | Versions differ by OS release, and apps can't update Android's, so the same kernel would behave differently across a fleet. |
| `sqlx` | Asynchronous. The kernel is synchronous and deterministic; an async runtime on a register gains nothing. |
| A pure-Rust store (`redb`, `fjall`) | No SQL or full-text search, and no SQLCipher-class encryption. The architecture chose SQLite so one transaction spans events, projections and outbox. |
| A Rust rewrite of SQLite (Limbo, now Turso) | Not yet mature enough for money data. |
| `synchronous = NORMAL` | Faster, but a power loss can lose the last commits in WAL mode, such as a payment already made. |
| Verifying signatures on every read | A millisecond or more per order on a low-end device, against a 5 ms budget for a whole command, for no protection encryption at rest doesn't give. |

## References

- [ADR-0002](./0002-event-sourced-signed-event-log.md) (the event log),
  [ADR-0012](./0012-event-wire-format.md) (the wire format and the log writer)
- [offline-and-sync.md §3, §9, §12](../architecture/offline-and-sync.md),
  [security.md §4](../architecture/security.md#4-data-protection)
- Implementation: [`core/crates/keel-store`](../../core/crates/keel-store/)
