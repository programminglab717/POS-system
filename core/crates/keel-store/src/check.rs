//! The full check of a store (ADR-0018): everything in it that can be checked, reported as
//! problems, in order.
//!
//! The check stops at damage it can't see past: pages that don't authenticate, then a malformed
//! file. Past those, it checks the store's own data: events and their logs, the store's identity
//! and clock, projections against a rebuild, the outbox and the quarantine.

use keel_events::envelope::{Device, Location};
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::log::LogHead;
use keel_types::Id;
use rusqlite::types::Value;
use rusqlite::{Connection, TransactionBehavior};

use crate::error::StoreError;
use crate::outbox;
use crate::projection::{self, Projection};
use crate::rows;
use crate::schema::{self, hlc_bytes};
use crate::write::Reason;

/// Something the full check found wrong with a store.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// Page `n` of the database file failed its authentication: it was damaged, or changed by
    /// someone without the key. The check stops at damaged pages: nothing else in the file can
    /// be trusted.
    Page(u32),
    /// SQLite found the database malformed, in its own words: a b-tree, an index against its
    /// table, a type or a constraint. The check stops here too.
    Structure(String),
    /// The events table's row `row` (its arrival) holds an event that doesn't decode, doesn't
    /// hash to its stored hash, or is filed under columns its body doesn't match: another
    /// device, position, identifier, HLC, stream or location.
    Event {
        /// The row's arrival: its place in the table.
        row: i64,
    },
    /// `device`'s log breaks before its event at `seq`: events are missing before it, or it
    /// doesn't link to the event before it, by that event's hash and a later HLC.
    Chain {
        /// The device whose log breaks.
        device: Id<Device>,
        /// The event after the break.
        seq: u64,
    },
    /// The store's identity doesn't read, or names another device or location than the store
    /// opened as.
    Identity,
    /// The device's clock, which every write stores, is behind the HLC of its last event.
    Clock,
    /// The row of stream `stream` in projection `projection` isn't what a rebuild from the stored
    /// events makes: it is wrong, missing, or there when it shouldn't be. Rebuilding the
    /// projections mends it.
    Projection {
        /// The projection's table.
        projection: &'static str,
        /// The stream's identifier, as stored.
        stream: Vec<u8>,
    },
    /// The effect with key `key` doesn't read, names a cause the store doesn't hold, or its
    /// state, attempts and start time don't agree.
    Effect {
        /// The effect's key.
        key: Vec<u8>,
    },
    /// The quarantine's row `row` (its arrival) isn't filed under its message's digest, or has a
    /// reason the store doesn't know.
    Quarantined {
        /// The row's arrival: its place in the table.
        row: i64,
    },
}

/// Checks the store in `db`, which belongs to `device` at `location`.
pub(crate) fn run(
    db: &mut Connection,
    device: Id<Device>,
    location: Id<Location>,
) -> Result<Vec<Problem>, StoreError> {
    let pages = pages(db)?;
    if !pages.is_empty() {
        return Ok(pages);
    }
    let structure = structure(db)?;
    if !structure.is_empty() {
        return Ok(structure);
    }
    let mut problems = Vec::new();
    events(db, location, &mut problems)?;
    identity(db, device, location, &mut problems)?;
    projections(db, &mut problems)?;
    outbox::check(db, &mut problems)?;
    quarantine(db, &mut problems)?;
    Ok(problems)
}

/// Moves the WAL into the database file, then authenticates every page of the file: the pages
/// that fail, in order. SQLCipher reads the file itself, so a failed page doesn't stop it, and
/// doesn't break the connection.
pub(crate) fn pages(db: &Connection) -> Result<Vec<Problem>, StoreError> {
    let (busy, _, _): (i64, i64, i64) =
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy != 0 {
        return Err(StoreError::Busy);
    }
    let mut statement = db.prepare("PRAGMA cipher_integrity_check")?;
    let messages = statement.query_map([], |row| row.get::<_, String>(0))?;
    messages
        .map(|message| {
            let message = message?;
            // SQLCipher names the page in each message, such as "HMAC verification failed for
            // page 7".
            Ok(page_of(&message).map_or(Problem::Structure(message), Problem::Page))
        })
        .collect()
}

/// The page a message of `cipher_integrity_check` names.
fn page_of(message: &str) -> Option<u32> {
    let (_, after) = message.split_once("page ")?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// SQLite's check of the file's structure, with a larger cache while it runs: its complaints.
fn structure(db: &Connection) -> Result<Vec<Problem>, StoreError> {
    let cache: i64 = db.query_row("PRAGMA cache_size", [], |row| row.get(0))?;
    // 16 MiB. SQLite's check reads pages many times over, and each read of a page not in the
    // cache decrypts it again.
    db.pragma_update(None, "cache_size", -16_384)?;
    let messages = {
        let mut statement = db.prepare("PRAGMA integrity_check")?;
        let messages = statement.query_map([], |row| row.get::<_, String>(0))?;
        messages.collect::<Result<Vec<_>, _>>()
    };
    db.pragma_update(None, "cache_size", cache)?;
    Ok(messages?.into_iter().filter(|message| message != "ok").map(Problem::Structure).collect())
}

/// Checks every event, and every device's log.
fn events(
    db: &Connection,
    location: Id<Location>,
    problems: &mut Vec<Problem>,
) -> Result<(), StoreError> {
    let mut statement = db.prepare(
        "SELECT arrival, origin_device, origin_seq, event_id, hash, hlc, stream_kind, stream_id, \
         message FROM events ORDER BY origin_device, origin_seq",
    )?;
    let mut rows = statement.query([])?;
    // The log being checked: its device, the sequence number its next event should have, and
    // its last event, if that event read back and so can be linked to.
    let mut log: Option<(Vec<u8>, u64, Option<LogHead>)> = None;
    while let Some(row) = rows.next()? {
        let stored = Stored {
            row: row.get(0)?,
            device: row.get(1)?,
            seq: row.get(2)?,
            event_id: row.get(3)?,
            hash: row.get(4)?,
            hlc: row.get(5)?,
            stream_kind: row.get(6)?,
            stream_id: row.get(7)?,
            message: row.get(8)?,
        };
        let (Ok(device), Ok(seq)) =
            (schema::id::<Device>(&stored.device), u64::try_from(stored.seq))
        else {
            // Filed under no device or position: it belongs to no log.
            problems.push(Problem::Event { row: stored.row });
            continue;
        };
        let (next, last) = match &log {
            Some((current, next, last)) if *current == stored.device => (*next, *last),
            _ => (1, Some(LogHead::EMPTY)),
        };
        // Events missing before this one: it can't be linked to the one before it.
        let last = if seq == next {
            last
        } else {
            problems.push(Problem::Chain { device, seq });
            None
        };
        let following = seq.checked_add(1).ok_or(StoreError::Corrupt("a sequence number"))?;
        let Some(event) = stored.read(location) else {
            problems.push(Problem::Event { row: stored.row });
            log = Some((stored.device, following, None));
            continue;
        };
        let body = event.body();
        if let Some(last) = last
            && (body.prev_hash != last.hash() || body.hlc <= last.hlc())
        {
            problems.push(Problem::Chain { device, seq });
        }
        log = Some((stored.device, following, Some(LogHead::of(&event))));
    }
    Ok(())
}

/// An event's row, as stored.
struct Stored {
    row: i64,
    device: Vec<u8>,
    seq: i64,
    event_id: Vec<u8>,
    hash: Vec<u8>,
    hlc: Vec<u8>,
    stream_kind: String,
    stream_id: Vec<u8>,
    message: Vec<u8>,
}

impl Stored {
    /// The event, if it decodes, hashes to its stored hash, and is filed under the columns its
    /// body gives, in the store at `location`.
    fn read(&self, location: Id<Location>) -> Option<SignedEvent> {
        let event = SignedEvent::from_stored(&self.message).ok()?;
        let body = event.body();
        let filed = event.hash().as_bytes()[..] == self.hash[..]
            && body.origin_device.to_bytes()[..] == self.device[..]
            && i64::try_from(body.origin_seq.get()).ok() == Some(self.seq)
            && body.event_id.to_bytes()[..] == self.event_id[..]
            && hlc_bytes(body.hlc)[..] == self.hlc[..]
            && body.stream.kind.as_str() == self.stream_kind
            && body.stream.id.to_bytes()[..] == self.stream_id[..]
            && body.location == location;
        filed.then_some(event)
    }
}

/// Checks the store's identity, and the device's clock against its last event.
fn identity(
    db: &Connection,
    device: Id<Device>,
    location: Id<Location>,
    problems: &mut Vec<Problem>,
) -> Result<(), StoreError> {
    let identity = match schema::identity(db) {
        Ok(identity) => identity,
        Err(StoreError::Corrupt(_)) => {
            problems.push(Problem::Identity);
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if identity.device != device || identity.location != location {
        problems.push(Problem::Identity);
    }
    match rows::head(db, device) {
        Ok(head) if identity.clock < head.hlc() => problems.push(Problem::Clock),
        // A head that doesn't read is its event's problem.
        Ok(_) | Err(StoreError::Corrupt(_)) => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

/// Checks every row of every projection against what rebuilding it makes. Each stream's rebuild
/// is undone before the next, so that the transaction holds one stream's changes at a time, and
/// the transaction is rolled back.
fn projections(db: &mut Connection, problems: &mut Vec<Problem>) -> Result<(), StoreError> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for projection in &projection::ALL {
        for stream in streams(&tx, projection)? {
            if let Some(false) = projection_row_holds(&tx, projection, &stream)? {
                problems.push(Problem::Projection { projection: projection.name, stream });
            }
        }
    }
    tx.rollback()?;
    Ok(())
}

/// Every stream `projection` has a row for, or stored events of, in order.
fn streams(db: &Connection, projection: &Projection) -> Result<Vec<Vec<u8>>, StoreError> {
    let mut statement = db.prepare(&format!(
        "SELECT stream_id FROM events WHERE stream_kind = ?1 UNION SELECT {key} FROM {table} \
         ORDER BY 1",
        key = projection.key,
        table = projection.name,
    ))?;
    let streams = statement.query_map([projection.kind], |row| row.get(0))?;
    Ok(streams.collect::<Result<_, _>>()?)
}

/// Whether the row of `stream` in `projection` is what rebuilding it makes: `None` if that can't
/// be told, since the stream has an event that doesn't read back.
fn projection_row_holds(
    db: &Connection,
    projection: &Projection,
    stream: &[u8],
) -> Result<Option<bool>, StoreError> {
    let stored = projection_row(db, projection, stream)?;
    db.execute_batch("SAVEPOINT rebuild")?;
    let rebuilt = rebuild_row(db, projection, stream);
    db.execute_batch("ROLLBACK TO rebuild; RELEASE rebuild")?;
    Ok(match rebuilt? {
        Rebuilt::Row(rebuilt) => Some(rebuilt == stored),
        Rebuilt::Unknown => None,
    })
}

/// What rebuilding a projection's row makes.
enum Rebuilt {
    /// The row, or no row at all.
    Row(Option<Vec<Value>>),
    /// Nothing that can be told: the stream has an event that doesn't read back.
    Unknown,
}

/// The row of `stream` in `projection` as rebuilding it makes it.
fn rebuild_row(
    db: &Connection,
    projection: &Projection,
    stream: &[u8],
) -> Result<Rebuilt, StoreError> {
    db.execute(
        &format!(
            "DELETE FROM {table} WHERE {key} = ?1",
            table = projection.name,
            key = projection.key
        ),
        [stream],
    )?;
    if let Ok(id) = schema::id(stream) {
        match rows::stream_events(db, projection.kind, id) {
            Ok(events) if events.is_empty() => {}
            Ok(events) => projection.project(db, id, &events)?,
            Err(StoreError::Corrupt(_)) => return Ok(Rebuilt::Unknown),
            Err(error) => return Err(error),
        }
    }
    Ok(Rebuilt::Row(projection_row(db, projection, stream)?))
}

/// The row of `stream` in `projection`, as stored.
fn projection_row(
    db: &Connection,
    projection: &Projection,
    stream: &[u8],
) -> Result<Option<Vec<Value>>, StoreError> {
    let mut statement = db.prepare(&format!(
        "SELECT * FROM {table} WHERE {key} = ?1",
        table = projection.name,
        key = projection.key,
    ))?;
    let columns = statement.column_count();
    let mut rows = statement.query([stream])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    Ok(Some((0..columns).map(|column| row.get(column)).collect::<Result<_, _>>()?))
}

/// Checks every quarantined message: filed under its digest, with a reason the store knows.
fn quarantine(db: &Connection, problems: &mut Vec<Problem>) -> Result<(), StoreError> {
    let mut statement =
        db.prepare("SELECT arrival, digest, reason, message FROM quarantine ORDER BY arrival")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let arrival: i64 = row.get(0)?;
        let digest: Vec<u8> = row.get(1)?;
        let reason: String = row.get(2)?;
        let message: Vec<u8> = row.get(3)?;
        if EventHash::of(&message).as_bytes()[..] != digest[..]
            || Reason::from_code(&reason).is_none()
        {
            problems.push(Problem::Quarantined { row: arrival });
        }
    }
    Ok(())
}
