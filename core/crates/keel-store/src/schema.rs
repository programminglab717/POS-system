//! The database's settings and schema.
//!
//! Every connection gives SQLCipher the store's key first, as a raw key, and pins SQLCipher 4's
//! settings: 4 KiB pages, each encrypted with AES-256-CBC and authenticated with HMAC-SHA512
//! (ADR-0018). Then it sets the WAL journal, commits that wait for the disk, and temporary
//! storage in memory, and checks SQLite runs with them.
//!
//! The schema version is `PRAGMA user_version`. Opening a store brings it up to the latest
//! version in one transaction, whatever version it was at. Version 1:
//!
//! - `store`: one row naming the device and location the store belongs to, and the device's
//!   clock, the latest HLC its log writer issued or observed.
//! - `events`: every stored event, as the COSE_Sign1 bytes it was signed as (`message`), with the
//!   columns queries need, taken from its verified body. Each device's log is stored without
//!   gaps, so a device's highest sequence number is how far into its log the store holds.
//!   Identifiers are 16 bytes, and HLCs 8 bytes, big-endian, so both sort as they compare.
//! - `quarantine`: each distinct message that was refused, once, with why.
//!
//! Version 2 (ADR-0017):
//!
//! - `outbox`: effects waiting to happen, each once by its idempotency key, in the order they were
//!   enqueued. Times are milliseconds since the Unix epoch.
//! - `projections`: the version of each projection the store built. The projections' own tables
//!   are theirs to create and drop ([`crate::projection`]).

use std::path::Path;

use keel_events::envelope::{Device, Location};
use keel_types::{Hlc, Id};
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, TransactionBehavior, params};

use crate::error::StoreError;
use crate::faults::{Faults, Point, proceed};
use crate::key::StoreKey;

/// The latest schema version this kernel knows.
pub(crate) const VERSION: i64 = 2;

/// Version 1's tables.
const V1: &str = "
CREATE TABLE store (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    device BLOB NOT NULL CHECK (length(device) = 16),
    location BLOB NOT NULL CHECK (length(location) = 16),
    clock BLOB NOT NULL CHECK (length(clock) = 8)
) STRICT;

CREATE TABLE events (
    arrival INTEGER PRIMARY KEY,
    origin_device BLOB NOT NULL CHECK (length(origin_device) = 16),
    origin_seq INTEGER NOT NULL CHECK (origin_seq >= 1),
    event_id BLOB NOT NULL UNIQUE CHECK (length(event_id) = 16),
    hash BLOB NOT NULL CHECK (length(hash) = 32),
    hlc BLOB NOT NULL CHECK (length(hlc) = 8),
    stream_kind TEXT NOT NULL,
    stream_id BLOB NOT NULL CHECK (length(stream_id) = 16),
    message BLOB NOT NULL,
    UNIQUE (origin_device, origin_seq)
) STRICT;

CREATE INDEX events_by_stream ON events (stream_kind, stream_id, hlc, origin_device, origin_seq);

CREATE TABLE quarantine (
    arrival INTEGER PRIMARY KEY,
    digest BLOB NOT NULL UNIQUE CHECK (length(digest) = 32),
    reason TEXT NOT NULL,
    message BLOB NOT NULL
) STRICT;
";

/// Version 2's tables.
const V2: &str = "
CREATE TABLE outbox (
    seq INTEGER PRIMARY KEY,
    key BLOB NOT NULL UNIQUE CHECK (length(key) BETWEEN 1 AND 64),
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
    payload BLOB NOT NULL CHECK (length(payload) <= 65536),
    cause BLOB CHECK (length(cause) = 16),
    state TEXT NOT NULL CHECK (state IN ('pending', 'running', 'done', 'failed')),
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    enqueued INTEGER NOT NULL,
    due INTEGER NOT NULL,
    started INTEGER
) STRICT;

CREATE INDEX outbox_by_due ON outbox (state, due, seq);

CREATE TABLE projections (
    name TEXT PRIMARY KEY,
    version INTEGER NOT NULL
) STRICT;
";

/// Opens the database at `path`, creating it if there is none, with `key`: SQLCipher encrypts
/// it with SQLCipher 4's settings (ADR-0018). Then sets the durability settings, and checks that
/// SQLite runs with them.
///
/// # Errors
/// [`StoreError::KeyRejected`] if the key doesn't open the database, [`StoreError::Settings`]
/// if SQLite won't run with a setting, and [`StoreError::Database`] if the file can't be opened.
pub(crate) fn connect(path: &Path, key: &StoreKey) -> Result<Connection, StoreError> {
    let db = Connection::open(path)?;
    // SQLCipher logs to standard error, or the device's log; the store reports what goes wrong
    // through its errors instead. The setting is the process's.
    db.execute_batch("PRAGMA cipher_log_level = NONE")?;
    db.execute_batch(&key.pragma("key"))?;
    // Pinned, so a later SQLCipher with other defaults still opens the store.
    db.execute_batch("PRAGMA cipher_compatibility = 4")?;
    // The first read of the file, so the first to meet a key that doesn't open it: SQLCipher
    // finds the first page doesn't authenticate. A file SQLite finds malformed, such as one cut
    // short, is damaged.
    if let Err(error) = db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)) {
        return Err(if error.sqlite_error_code() == Some(ErrorCode::NotADatabase) {
            StoreError::KeyRejected
        } else {
            error.into()
        });
    }
    configure(&db)?;
    Ok(db)
}

/// Checks that `key` opens the database at `path`, through a connection that can't write. A
/// connection that can write moves the WAL into the database file and removes it as it closes,
/// even one whose key was refused; this one leaves the WAL as it found it, so the store can
/// still tell it wasn't closed cleanly. Used only when there is a WAL: where there is none, this
/// connection would leave an empty one behind.
///
/// # Errors
/// [`StoreError::KeyRejected`] if the key doesn't open the database. Anything else is left for
/// opening the database to report.
pub(crate) fn check_key(path: &Path, key: &StoreKey) -> Result<(), StoreError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let Ok(db) = Connection::open_with_flags(path, flags) else { return Ok(()) };
    let read = db
        .execute_batch("PRAGMA cipher_log_level = NONE")
        .and_then(|()| db.execute_batch(&key.pragma("key")))
        .and_then(|()| db.execute_batch("PRAGMA cipher_compatibility = 4"))
        .and_then(|()| db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)));
    match read {
        Err(error) if error.sqlite_error_code() == Some(ErrorCode::NotADatabase) => {
            Err(StoreError::KeyRejected)
        }
        _ => Ok(()),
    }
}

/// Whether the connection can still read the database: `false` once it has met a damaged page.
///
/// SQLCipher gives SQLite a page that fails its authentication as zeros, and fails every read of
/// the connection after it. A b-tree page of zeros is malformed, which SQLite reports; but the
/// last page of a long value holds nothing but the value's bytes, so a read of it returns zeros
/// in the value, without an error. One more read, of the database's header, fails if the read
/// before met a damaged page: SQLite empties its cache after a failed read, so the header comes
/// from the file again (ADR-0018). Otherwise it comes from the cache, at no cost.
pub(crate) fn sound(db: &Connection) -> bool {
    match db.query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0)) {
        Ok(_) => true,
        // SQLCipher's failure after a damaged page reads as SQLite running out of memory.
        Err(error) => !matches!(
            error.sqlite_error_code(),
            Some(ErrorCode::OutOfMemory | ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
        ),
    }
}

/// Sets the durability settings, and checks that SQLite runs with them: the WAL journal, every
/// commit on disk before it returns, and temporary tables and indexes in memory, never in a
/// file that isn't encrypted.
fn configure(db: &Connection) -> Result<(), StoreError> {
    let journal: String = db.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Settings("the WAL journal"));
    }
    db.execute_batch(
        "PRAGMA synchronous = FULL; PRAGMA trusted_schema = OFF; PRAGMA temp_store = MEMORY;",
    )?;
    // FULL is 2.
    let synchronous: i64 = db.query_row("PRAGMA synchronous", [], |row| row.get(0))?;
    if synchronous != 2 {
        return Err(StoreError::Settings("synchronous commits"));
    }
    // MEMORY is 2.
    let temp_store: i64 = db.query_row("PRAGMA temp_store", [], |row| row.get(0))?;
    if temp_store != 2 {
        return Err(StoreError::Settings("temporary storage in memory"));
    }
    Ok(())
}

/// Brings the schema up to [`VERSION`], in one transaction. A new database is created for
/// `device` at `location`.
pub(crate) fn migrate(
    db: &mut Connection,
    device: Id<Device>,
    location: Id<Location>,
    faults: &mut dyn Faults,
) -> Result<(), StoreError> {
    if up_to_date(schema_version(db)?)? {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another connection may have migrated since the version was read.
    let version = schema_version(&tx)?;
    if up_to_date(version)? {
        return Ok(());
    }
    if version < 1 {
        tx.execute_batch(V1)?;
        tx.execute(
            "INSERT INTO store (singleton, device, location, clock) VALUES (1, ?1, ?2, ?3)",
            params![&device.to_bytes()[..], &location.to_bytes()[..], &hlc_bytes(Hlc::ZERO)[..]],
        )?;
    }
    if version < 2 {
        tx.execute_batch(V2)?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    proceed(faults, Point::Migrating)?;
    tx.commit()?;
    Ok(())
}

/// Whether a schema at `version` is at [`VERSION`]: `false` if it is older.
///
/// # Errors
/// [`StoreError::NewerSchema`] if a newer kernel wrote it.
fn up_to_date(version: i64) -> Result<bool, StoreError> {
    if version > VERSION {
        return Err(StoreError::NewerSchema { found: version, known: VERSION });
    }
    Ok(version == VERSION)
}

fn schema_version(db: &Connection) -> Result<i64, StoreError> {
    Ok(db.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Who the store belongs to, and the device's clock.
pub(crate) struct Identity {
    pub(crate) device: Id<Device>,
    pub(crate) location: Id<Location>,
    pub(crate) clock: Hlc,
}

/// Reads who the store belongs to.
pub(crate) fn identity(db: &Connection) -> Result<Identity, StoreError> {
    let row = db
        .query_row("SELECT device, location, clock FROM store WHERE singleton = 1", [], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Vec<u8>>(2)?))
        })
        .optional()?;
    let (device, location, clock) = row.ok_or(StoreError::Corrupt("the store's identity"))?;
    Ok(Identity { device: id(&device)?, location: id(&location)?, clock: hlc(&clock)? })
}

/// Stores the device's clock.
pub(crate) fn store_clock(db: &Connection, clock: Hlc) -> Result<(), StoreError> {
    db.execute("UPDATE store SET clock = ?1 WHERE singleton = 1", [&hlc_bytes(clock)[..]])?;
    Ok(())
}

/// An HLC as stored: its packed form, big-endian, which sorts like the HLC.
pub(crate) fn hlc_bytes(hlc: Hlc) -> [u8; 8] {
    hlc.to_u64().to_be_bytes()
}

/// An HLC from its stored bytes.
pub(crate) fn hlc(bytes: &[u8]) -> Result<Hlc, StoreError> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| StoreError::Corrupt("an HLC"))?;
    Ok(Hlc::from_u64(u64::from_be_bytes(bytes)))
}

/// An identifier from its stored bytes.
pub(crate) fn id<T>(bytes: &[u8]) -> Result<Id<T>, StoreError> {
    let bytes: [u8; 16] = bytes.try_into().map_err(|_| StoreError::Corrupt("an identifier"))?;
    Id::from_bytes(bytes).map_err(|_| StoreError::Corrupt("an identifier"))
}
