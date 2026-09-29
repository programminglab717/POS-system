//! Reading and writing events' rows, inside a write's transaction or outside one.

use keel_events::envelope::Device;
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::log::LogHead;
use keel_types::Id;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::StoreError;
use crate::schema::{hlc, hlc_bytes};

/// A sequence number as stored. SQLite's integers are signed, so the store holds logs of up to
/// 2^63 − 1 events.
pub(crate) fn seq_value(seq: u64) -> Result<i64, StoreError> {
    i64::try_from(seq).map_err(|_| StoreError::OutOfRange("a sequence number"))
}

/// Stores `event`, which the caller has checked is the next in its device's log.
pub(crate) fn insert(db: &Connection, event: &SignedEvent) -> Result<(), StoreError> {
    let body = event.body();
    db.execute(
        "INSERT INTO events \
         (origin_device, origin_seq, event_id, hash, hlc, stream_kind, stream_id, message) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            &body.origin_device.to_bytes()[..],
            seq_value(body.origin_seq.get())?,
            &body.event_id.to_bytes()[..],
            &event.hash().as_bytes()[..],
            &hlc_bytes(body.hlc)[..],
            body.stream.kind.as_str(),
            &body.stream.id.to_bytes()[..],
            event.to_bytes(),
        ],
    )?;
    Ok(())
}

/// The head of `device`'s log as stored: its last event's sequence number, hash and HLC.
pub(crate) fn head(db: &Connection, device: Id<Device>) -> Result<LogHead, StoreError> {
    let row = db
        .query_row(
            "SELECT origin_seq, hash, hlc FROM events WHERE origin_device = ?1 \
             ORDER BY origin_seq DESC LIMIT 1",
            [&device.to_bytes()[..]],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Vec<u8>>(2)?)),
        )
        .optional()?;
    let Some((seq, hash, stored_hlc)) = row else { return Ok(LogHead::EMPTY) };
    let seq = u64::try_from(seq).map_err(|_| StoreError::Corrupt("a sequence number"))?;
    Ok(LogHead::from_parts(seq, event_hash(&hash)?, hlc(&stored_hlc)?))
}

/// The hash of the event at `seq` in `device`'s log, if the store holds it.
pub(crate) fn hash_at(
    db: &Connection,
    device: Id<Device>,
    seq: u64,
) -> Result<Option<EventHash>, StoreError> {
    let hash = db
        .query_row(
            "SELECT hash FROM events WHERE origin_device = ?1 AND origin_seq = ?2",
            params![&device.to_bytes()[..], seq_value(seq)?],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    hash.map(|hash| event_hash(&hash)).transpose()
}

/// Whether the store holds an event with identifier `id`.
pub(crate) fn has_id(db: &Connection, id: &[u8]) -> Result<bool, StoreError> {
    let found = db
        .query_row("SELECT 1 FROM events WHERE event_id = ?1", [id], |row| row.get::<_, i64>(0))
        .optional()?;
    Ok(found.is_some())
}

/// An event read back from a row whose first column is its message and second its hash. The
/// store verified the event before storing it, so its signature isn't checked again; its hash
/// is, against the stored one, which catches a message that doesn't read back as stored.
pub(crate) fn stored_event(row: &Row<'_>) -> rusqlite::Result<Result<SignedEvent, StoreError>> {
    let message: Vec<u8> = row.get(0)?;
    let hash: Vec<u8> = row.get(1)?;
    Ok(read_event(&message, &hash))
}

fn read_event(message: &[u8], hash: &[u8]) -> Result<SignedEvent, StoreError> {
    let event = SignedEvent::from_stored(message).map_err(|_| StoreError::Corrupt("an event"))?;
    if event.hash().as_bytes()[..] != *hash {
        return Err(StoreError::Corrupt("an event's hash"));
    }
    Ok(event)
}

fn event_hash(bytes: &[u8]) -> Result<EventHash, StoreError> {
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| StoreError::Corrupt("a hash"))?;
    Ok(EventHash::from_bytes(bytes))
}

/// Collects rows of stored events, failing on the first that doesn't read back.
pub(crate) fn collect(
    rows: impl Iterator<Item = rusqlite::Result<Result<SignedEvent, StoreError>>>,
) -> Result<Vec<SignedEvent>, StoreError> {
    rows.map(|row| row?).collect()
}
