# ADR-0018: Encryption at rest and integrity checks: SQLCipher with a vendored OpenSSL, a raw key the platform protects, and checks on open and on request

- **Status:** Accepted (2026-09-30), with the details settled in building it, under "As built",
  accepted after review
- **Date:** 2026-09-29

## Context

[ADR-0016](./0016-device-store.md) keeps each device's events, projections and outbox in one
SQLite database. Its third slice encrypts that database and checks it. The design already fixes
the goal:

- The database is encrypted with SQLCipher-class AES-256, with a key wrapped by the platform
  keystore: a device powered off and taken from the store reveals nothing without its
  secure-hardware key ([security.md §4](../architecture/security.md#4-data-protection)).
- Per-device hash chains and signatures make the event history tamper-evident, and the cloud
  keeps Merkle roots of each location's events
  ([security.md §7](../architecture/security.md#7-integrity-of-records)).
- Events reach the hub and the cloud, which keep them longer than a terminal does
  ([offline-and-sync.md §9](../architecture/offline-and-sync.md#9-storage-retention-and-bootstrap)).

ADR-0016 planned "integrity checks when the store opens". Measuring them in designing this
slice changed the plan, since a full check costs time in proportion to the store. On the
development machine, for a store of 155 MB (200,000 events of 500 bytes, on a RAM disk, so only
the CPU counts):

| Operation | Unencrypted | Encrypted |
|---|---|---|
| A write of one event, committed | 22 µs | 84 µs |
| Reading every event | 45 ms | 372 ms |
| Authenticating every page (`cipher_integrity_check`) | | 456 ms |
| SQLite's structure check (`integrity_check`), 2 MiB cache | 581 ms | 3.4 s |
| The same, with a 16 MiB cache | | 1.4 s |
| Opening with a raw key, and with a passphrase (PBKDF2, 256,000 rounds) | | 0.2 ms, 145 ms |
| Re-encrypting with a new key | | 1.3 s |

A low-end register may be ten times slower, and a hub keeps 90 days of events.

Designing it also found a hazard in SQLCipher 4.14's `PRAGMA rekey`. When it can't rewrite the
database, for instance because another connection holds the write lock, it still reports
success, and its connection goes on encrypting the pages it writes with the new key while the
rest stay under the old one. Reproduced here: one ordinary write after such a rekey left a store
that neither key could open.

## Decision

1. **SQLCipher, compiled in with a vendored OpenSSL.** `rusqlite`'s
   `bundled-sqlcipher-vendored-openssl` feature compiles SQLCipher (4.14.0 today, on SQLite
   3.51.3) and builds OpenSSL (3.6.3 today) from source, linked statically: the same code and the
   same crypto on every platform, rather than CommonCrypto on Apple's platforms and the system's
   OpenSSL elsewhere (Android has none that apps may use). The dependency policy gains
   `openssl-sys` and `openssl-src`.
   - **What SQLCipher does:** each 4 KiB page is encrypted with AES-256-CBC under a random IV,
     and authenticated with HMAC-SHA512 over the page, its IV and its page number. A page that
     anyone without the key changed, moved or damaged fails to read. The WAL's pages are
     encrypted too.
   - **Settings pinned:** SQLCipher 4's (`PRAGMA cipher_compatibility = 4`), so a later SQLCipher
     with other defaults still opens the store. Temporary tables and indexes stay in memory
     (`temp_store = MEMORY`), never in a file. SQLCipher's own logging is off: the store reports
     what goes wrong through its errors.
   - **What isn't encrypted:** the file's first 16 bytes, a random salt; the WAL's frame headers
     and its shared-memory index, which hold page numbers and commit sizes; and the files' sizes.
2. **The key is 32 random bytes that the platform protects; the kernel only receives it.** A
   `StoreKey` is zeroed when dropped and never printed. SQLCipher takes it as a raw key: deriving
   a key from a passphrase would cost seconds per open on a register, for nothing, since the key
   is already random. The platform shell:
   - generates the key from the operating system's secure generator;
   - wraps it with a key that never leaves the platform keystore (Android Keystore, in StrongBox
     where there is one; iOS Keychain and the Secure Enclave; the TPM or DPAPI on Windows; the TPM
     on Linux);
   - stores the wrapped key durably before it creates the store, and unwraps it to open it.

   The key never goes into the database, a log or a backup. A lost key loses the store: the
   device starts again from its peers, and loses what it hadn't sent them.
3. **Opening checks what's cheap, every time.** Opening costs the same whatever the store holds:
   - the key, and the cipher settings: a store that doesn't open with them is refused as
     `KeyRejected`. SQLCipher can't tell a wrong key from a damaged first page, or from a file
     that isn't a store;
   - as before: the durability settings, now with temporary storage in memory, the schema
     version, the device and location, the signer of the device's last event, and the
     projections' versions.

   The store also reports whether it was closed cleanly last time, from whether SQLite's journal
   was left behind: after a crash, a kill or a power loss, the shell runs a full check when the
   device is next idle.
4. **A full check, on request.** `Store::check` reports every problem it finds, in order:
   - it checkpoints the WAL into the database file, then authenticates every page of the file.
     A page that fails is reported, and the check stops: nothing else in a damaged file can be
     trusted, and SQLCipher refuses every read after one fails;
   - SQLite's structure check (`integrity_check`), with a 16 MiB cache while it runs: b-trees,
     indexes against their tables, types and constraints;
   - every event: that it decodes, hashes to its stored hash, and is filed under the columns
     its body gives (device, position, identifier, HLC, stream, location); and every device's
     log: no gaps from its first event, each event linked to the one before it by hash, with a
     later HLC;
   - the device's clock, which every write stores, against the HLC of its last event;
   - every projection row against a rebuild from the stored events, made inside a transaction
     that is then rolled back;
   - every effect: its fields readable, its cause stored, and its state, attempts and start
     time consistent; and every quarantined message filed under its digest, with a known
     reason.

   It doesn't verify signatures again: the store verified each received event before storing
   it, and changing a stored event takes the key. It holds the store while it runs, so the shell
   runs it when the device is idle: once a day, after the business-day close, and after an
   unclean shutdown.
5. **A damaged store fails closed.** Once its connection meets a page that fails authentication,
   SQLCipher fails every read after it. The store reports `Damaged` for the read that met the
   page, and for every call after it until the store is reopened, and never returns data from a
   page that failed (how, under "As built"). What to do with a damaged store, salvaging what
   reads and starting again from peers, comes with the sync engine.
6. **Changing the key: `Store::rekey`.** SQLCipher rewrites every page under the new key in one
   write transaction, so a crash leaves the store under one key or the other. Because of the
   hazard above, the store never uses the connection that ran the rekey again. It closes it,
   reopens the store with the new key and authenticates every page under it. If the new key is
   refused, it reopens with the old key and reports `NotRekeyed`, the store unchanged and still
   usable. Rotating a key, for the shell: store the new wrapped key as pending, rekey, then make
   it current. After a crash, if the current key is refused, the pending one opens the store.
7. **Verification**, as for the event log, plus:
   - every existing test, property test and crash test now runs on encrypted stores;
   - known answers: neither the database nor its WAL holds plaintext; another key, an
     unencrypted database, or a file that isn't a database is refused; the settings are pinned;
     a failed rekey is reported and changes nothing. A **golden store**, encrypted with a fixed key and committed to the
     repository, must open and read as it did when it was made, so a later SQLCipher, OpenSSL or
     kernel can't lose the ability to open an existing store unnoticed;
   - property tests against models: of keys (only the current key opens a store, through
     writes, rekeys, failed rekeys and reopenings, and the contents are the model's); of
     physical damage (bits changed in a closed store's file: the store never returns wrong data,
     and the check reports exactly the pages changed); and of logical damage (rows changed with
     the key, as a bug would: the check reports exactly the problems made);
   - crash tests: a full check after every crash, and crashes during a rekey, after which
     exactly one key opens the store, with its contents intact.

## As built

Details settled in building it, for review with it:

- **Opening.** A connection gives SQLCipher the key first, then `cipher_compatibility = 4`, and
  turns SQLCipher's logging off. Its first read tells a key that doesn't open the store (the
  first page doesn't authenticate: `KeyRejected`) from a file SQLite finds malformed
  (`Damaged`). A file cut short by a whole page is damaged when it opens; cut inside its last
  page, it opens, and the check finds that page.
- **A process's first connection** (added after acceptance, when CI failed). SQLite runs
  SQLCipher's initialization only after it has marked itself initialized and let other threads
  on. A connection keyed on another thread meanwhile is refused, as if its key were empty: stores
  opened at once as a process's first use of SQLite failed to open about once in 200 tries. So
  the store opens the process's first connection alone, and other threads wait for it
  (`init_sqlite`). Code that opens SQLite connections of its own, in a process with stores,
  calls it before its first. SQLCipher, up to 4.19, still initializes this way.
- **A journal left behind** says the store wasn't closed cleanly (`Store::recovered`). Building
  it found that a connection that can write moves the WAL into the database file and removes it
  as it closes, even one whose key was refused: a shell trying its current key, then the pending
  one, after a crash during a rotation would lose the sign of the crash. So when there is a WAL,
  the store checks the key through a read-only connection first, which leaves the WAL as it
  found it. When there is none, it doesn't: a read-only connection would leave an empty one.
- **Reads that meet damage.** The damage property test found that SQLCipher doesn't fail a read
  of a damaged page: it hands SQLite the page as zeros, and fails every read of the connection
  after it. A b-tree page of zeros is malformed, which SQLite reports; but the last page of a
  long value holds nothing but the value's bytes, so the first read of it returned the value
  with zeros in it, without an error. Events caught it, by their hashes; an effect's payload or
  a quarantined message wouldn't have. So after each read, the store makes one more, of the
  database's header, which comes from the cache unless the read before met a damaged page, and
  then fails. If it fails, the store reports `Damaged`, and discards what it read. A write that
  meets a damaged page can't write back what it read there, since SQLite won't commit once a
  read has failed, and it reports `Damaged` too. The extra read costs about 2 µs, where a
  read of a device's head costs 10 µs with it.
- **A damaged store** refuses everything after the damage until it is reopened. `Store::check`
  still works: it opens the file afresh to check it, and reports a first page that no longer
  authenticates as damaged, since the key opened the store before. A rekey that can't open the
  file again leaves the store refusing everything with `Closed`.
- **The key.** `StoreKey::new` takes the caller's bytes and zeroes them; the key lives in a box,
  so moving it leaves no copy behind, and is zeroed when dropped; it prints as `StoreKey(..)`.
  The store keeps the key while it is open, to open its file again in a rekey or a check.
- **The check's problems**, in the order it reports them: pages that fail their authentication,
  by number (a page added to the file, or cut short, fails too), and then nothing else; SQLite's
  complaints, in its words, and then nothing else; events that don't decode, don't hash to their
  stored hash, or are filed under other columns than their bodies give, by row; breaks in a
  device's log, by the device and the position after the break: missing events, or an event
  that doesn't link to the one before it by hash and a later HLC, judged only between events
  that read back; the store's identity, and the device's clock; projection rows that differ
  from a rebuild, by projection and stream, except in streams with an event that doesn't read
  back; effects that don't read, name a cause the store doesn't hold, or whose state, attempts
  and start time disagree; and quarantined messages not filed under their digest. The rebuild
  it compares with runs stream by stream, each undone before the next so that the check holds
  one stream's changes at a time, inside a transaction that is rolled back: the check changes
  nothing.
- **The golden store** of schema version 2 is 94 KB: seven of the device's own events, among
  them an order with a line and a closed check and a payment captured on it, three of another
  device's events, a quarantined message, and effects done, failed, running and pending.
- **Costs**, measured on the development machine in a release build, on a RAM disk, against
  the same store unencrypted:

  | Operation | Unencrypted | Encrypted |
  |---|---|---|
  | A write of one event, on a stream no projection follows | 131 µs | 225 µs |
  | The same, the event alone in its order's stream | 205 µs | 349 µs |
  | The same, to an order of about 40 events, all folded again | 385 µs | 595 µs |
  | A store of 100,000 events, 72 MB: opening | 0.5 ms | 0.8 ms |
  | The full check of that store | 1.5 s | 2.7 s |
  | Re-encrypting it with a new key | | 1.1 s |

  Signing each event, and folding its stream again, cost more than encryption; a write pays
  for encryption by the page, so writing 100 events at once costs about the same either way. A
  read from the cache costs the same either way, about 10 µs for a device's head, proven.
  Verifying a signature, which the full check doesn't do, costs 55 µs.

## Consequences

**Positive**

- A register taken from a store reveals nothing without the key its platform protects.
- Changing the file without the key is detected at the first read of the changed page. A
  damaged store fails closed, and never returns what was changed.
- The full check finds damage before anything reads it, and finds what encryption can't: bugs
  in SQLite, in the store or in memory, that wrote data wrong.
- The golden store makes an unreadable store after an upgrade a test failure, not a support
  call.

**Negative**

- OpenSSL, a large C library, in the build: a clean build takes about two minutes longer, and
  needs Perl and a `make` on every platform. Its security advisories must be followed, through
  `openssl-src` releases. SQLCipher uses only its ciphers, hashes and random generator.
- SQLite follows SQLCipher's releases: 3.51.3 now, where the unencrypted build had 3.53.2.
- Each write costs about 90 to 210 µs more on the development machine, the more pages it
  touches, and each read of a page not in the cache decrypts it: reading every event is eight
  times slower. The full check takes seconds for a large store, and more on a register.
- SQLCipher's HMAC-SHA512 may be slow on ARM cores without SHA-512 instructions, which many
  registers have. HMAC-SHA256 would be faster there. To measure on the reference register
  (step 7): before the first deployment, changing it costs nothing; after, it means
  re-encrypting every store.
- The key, and decrypted pages, are in the process's memory while the store is open. Encryption
  at rest protects a device that is off, not a running one.
- The database's bytes aren't reproducible, since IVs are random: tests and the simulator
  compare contents, never bytes.
- The store can't tell when it lost its latest writes, from an older copy of it restored over
  it, or a damaged WAL frame, which SQLite discards with every frame after it, as it would a
  torn write. Every page still authenticates. The device's peers can tell, since they hold the
  device's later events. So before the device writes again, the sync engine must check the
  device's log against theirs when it can, or its next events would fork its log.
- SQLCipher's logging is process-wide, and the store turns it off.

## Alternatives considered

| Alternative | Why not |
|---|---|
| SQLite3 Multiple Ciphers | Its own crypto, so no OpenSSL, with ChaCha20-Poly1305 by default. But `rusqlite` doesn't support it, and it is less widely deployed and reviewed than SQLCipher. |
| SQLCipher with each platform's crypto: CommonCrypto on Apple's platforms, the system's OpenSSL on Linux and Windows | Different crypto code on each platform, and Android would need a vendored OpenSSL anyway. |
| A key derived from a passphrase (PBKDF2) | 145 ms to open here, and seconds on a register, for nothing: the key is 32 random bytes. |
| Encrypting pages in Rust, in a SQLite VFS of our own | Needs `unsafe` code and a VFS to maintain, to redo what a mature, reviewed codec does. |
| Relying on the platform's disk encryption alone | Not under Keel's control on every platform, and it doesn't authenticate pages. It stays underneath where the platform has it. |
| The full check every time the store opens | Seconds at every start for a large store, and more on a register. |
| Verifying signatures in the full check | 55 µs per event here, 5.5 s more for a store of 100,000 events, to find what the page authentication and the chain check already catch without the key. |
| Trusting SQLCipher's rekey to report failure | It doesn't (above). |

## References

- [ADR-0016](./0016-device-store.md) (the device store), [ADR-0017](./0017-projections-and-outbox.md)
  (projections and the outbox)
- [security.md §4, §7](../architecture/security.md),
  [offline-and-sync.md §9](../architecture/offline-and-sync.md#9-storage-retention-and-bootstrap)
- SQLCipher's design: <https://www.zetetic.net/sqlcipher/design/>
- Implementation: [`core/crates/keel-store`](../../core/crates/keel-store/)
