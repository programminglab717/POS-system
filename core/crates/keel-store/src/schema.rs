//! The database's settings and schema.
//!
//! The schema version is `PRAGMA user_version`, and each migration takes it one version further,
//! in one transaction. Version 1:
//!
//! - `store`: one row naming the device and location the store belongs to, and the device's
//!   clock, the latest HLC its log writer issued or observed.
//! - `events`: every stored event, as the COSE_Sign1 bytes it was signed as (`message`), with the
//!   columns queries need, taken from its verified body. Each device's log is stored without
//!   gaps, so a device's highest sequence number is how far into its log the store holds.
//!   Identifiers are 16 bytes, and HLCs 8 bytes, big-endian, so both sort as they compare.
//! - `quarantine`: each distinct message that was refused, once, with why.

use keel_events::envelope::{Device, Location};
use keel_types::{Hlc, Id};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::error::StoreError;
use crate::faults::{Faults, Point, proceed};

/// The latest schema version this kernel knows.
pub(crate) const VERSION: i64 = 1;

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

/// Sets the durability settings, and checks that SQLite runs with them: the WAL journal, and
/// every commit on disk before it returns.
pub(crate) fn configure(db: &Connection) -> Result<(), StoreError> {
    let journal: String = db.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Settings("the WAL journal"));
    }
    db.execute_batch("PRAGMA synchronous = FULL; PRAGMA trusted_schema = OFF;")?;
    // FULL is 2.
    let synchronous: i64 = db.query_row("PRAGMA synchronous", [], |row| row.get(0))?;
    if synchronous != 2 {
        return Err(StoreError::Settings("synchronous commits"));
    }
    Ok(())
}

/// Brings the schema up to [`VERSION`]. A new database is created for `device` at `location`.
pub(crate) fn migrate(
    db: &mut Connection,
    device: Id<Device>,
    location: Id<Location>,
    faults: &mut dyn Faults,
) -> Result<(), StoreError> {
    let version = schema_version(db)?;
    if version > VERSION {
        return Err(StoreError::NewerSchema { found: version, known: VERSION });
    }
    if version >= VERSION {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another connection may have migrated since the version was read.
    if schema_version(&tx)? < 1 {
        tx.execute_batch(V1)?;
        tx.execute(
            "INSERT INTO store (singleton, device, location, clock) VALUES (1, ?1, ?2, ?3)",
            params![&device.to_bytes()[..], &location.to_bytes()[..], &hlc_bytes(Hlc::ZERO)[..]],
        )?;
        tx.execute_batch("PRAGMA user_version = 1")?;
        proceed(faults, Point::Migrating)?;
        tx.commit()?;
    }
    Ok(())
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
