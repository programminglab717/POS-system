//! Sequencing (ADR-0020): the Store Hub numbers the events it holds in the order it received
//! them, in records in its own log, and every replica works out from the records it holds which
//! of its events are confirmed. Only the records that count do anything: those of a term on the
//! chain of claims, within its cut (ADR-0022, [`crate::terms`]).
//!
//! - **Sequencing.** The hub is the store that holds the winning claim, and numbers in its
//!   epoch. For each device, it numbers the events after the last position a record that counts
//!   covers, except records themselves, in the order it received them; its own log, after its own
//!   last record of the epoch too, which numbered all of its log before it with the records
//!   written with it. Each device's log arrives in order, so each is numbered in order, whole,
//!   and a claimant holds every record that counts: what counts covers each device's log from
//!   its start without a gap. Within a record, a device's runs follow on from each other: a
//!   record ends where a device's next event doesn't follow on from the device's run in it, or
//!   when it holds as many runs as it may. Numbers follow on from the hub's last record of the
//!   epoch. It is all one write.
//! - **Confirmation.** A run that counts confirms the stretch of a device's log it covers when
//!   the store holds the run's last event with the hash the record gives it: the hash chain pins
//!   every event before it. Where records that count disagree about an event, which no hub does,
//!   its first number, by epoch and number, counts. Records aren't numbered themselves, and count
//!   as confirmed.

use std::collections::BTreeMap;

use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{self, Assigned, MAX_RUNS, Run, SequenceEvent};
use keel_events::envelope::{Actor, Component, Device, StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::keys::Signer;
use keel_events::log::{AppendError, EventDraft};
use keel_types::{BusinessDate, Entropy, Id, Timestamp};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StoreError;
use crate::rows::{self, seq_value};
use crate::schema::id;
use crate::store::noted;
use crate::terms::{self, COUNTING};
use crate::write::Writing;

/// An event's place in the store's sequence: the epoch of the hub that numbered it, and its
/// number in that epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreSeq {
    /// The hub's epoch.
    pub epoch: u64,
    /// The event's number in the epoch.
    pub number: u64,
}

/// A confirmed event and its number: an entry of the store's feed ([`crate::Store::sequenced`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequenced {
    /// The event's number in its epoch.
    pub number: u64,
    /// The event.
    pub event: SignedEvent,
}

/// An event no record covers yet.
struct Pending {
    arrival: i64,
    device: Id<Device>,
    position: u64,
    hash: EventHash,
    business_date: BusinessDate,
}

/// Why a record can't be made. Only the epoch's numbers running out can do it.
fn unrecordable<T>(_: T) -> StoreError {
    StoreError::OutOfRange("a sequencing record")
}

impl<S: Signer, E: Entropy> Writing<'_, S, E> {
    /// Numbers, as the hub, every event the store holds that no record that counts covers, and
    /// appends the records at physical time `now`. Returns them: none when nothing is new.
    pub(crate) fn sequence(&mut self, now: Timestamp) -> Result<Vec<SignedEvent>, StoreError> {
        let health = self.health;
        let own = self.writer.device();
        let epoch = terms::own_epoch(self.tx, own).map_err(|error| noted(health, error))?.get();
        let (pending, mut next) = pending(self.tx, own, epoch)
            .and_then(|pending| Ok((pending, next_number(self.tx, own, epoch)?)))
            .map_err(|error| noted(health, error))?;
        let mut written = Vec::new();
        for (runs, business_date) in records(pending) {
            let record = Assigned::new(epoch, next, runs).map_err(unrecordable)?;
            next = record.last().saturating_add(1);
            let (schema, payload) =
                SequenceEvent::Assigned(record).encode().map_err(unrecordable)?;
            let stream = StreamRef {
                kind: StreamKind::new(sequence::STREAM).map_err(unrecordable)?,
                id: self.writer.generate_id(now).map_err(AppendError::from)?,
            };
            let draft = EventDraft {
                stream,
                schema,
                business_date,
                actor: Actor::System(Component::new("sequencer").map_err(unrecordable)?),
                approval: None,
                causation: None,
                correlation: None,
                payload,
            };
            written.push(self.append(draft, now)?);
        }
        Ok(written)
    }
}

/// The events no record that counts covers, except records, in the order the store received
/// them: for each device, those after the last position a record that counts covers. The store's
/// own log is pending only after its latest record of `epoch`, too, which numbered everything of
/// its log before it with the records written with it: so its records, which no record covers,
/// aren't read again each time. (Not after its latest record that counts: a later claim may have
/// cut off a record written with it, and its numbers with it.)
fn pending(db: &Connection, own: Id<Device>, epoch: u64) -> Result<Vec<Pending>, StoreError> {
    // Every device the store holds events of, skipping from one to the next through the index
    // on each device's log.
    let devices: Vec<Vec<u8>> = {
        let mut statement = db.prepare(
            "WITH RECURSIVE devices (device) AS (SELECT min(origin_device) FROM events UNION ALL \
             SELECT (SELECT min(origin_device) FROM events WHERE origin_device > devices.device) \
             FROM devices WHERE devices.device IS NOT NULL) SELECT device FROM devices WHERE \
             device IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut covered = db.prepare(&format!(
        "SELECT s.to_seq FROM {COUNTING} WHERE s.device = ?1 ORDER BY s.to_seq DESC LIMIT 1"
    ))?;
    let mut numbered_own = db.prepare(
        "SELECT author_seq FROM sequence WHERE author = ?1 AND epoch = ?2 \
         ORDER BY number DESC LIMIT 1",
    )?;
    let mut statement = db.prepare(
        "SELECT message, hash, arrival FROM events WHERE origin_device = ?1 AND origin_seq > ?2 \
         AND stream_kind <> ?3 ORDER BY origin_seq",
    )?;
    let own = own.to_bytes();
    let mut pending = Vec::new();
    let epoch = seq_value(epoch)?;
    for device in devices {
        let mut after: i64 =
            covered.query_row([&device], |row| row.get(0)).optional()?.unwrap_or(0);
        if device[..] == own[..] {
            let records: Option<i64> =
                numbered_own.query_row(params![&device, epoch], |row| row.get(0)).optional()?;
            after = after.max(records.unwrap_or(0));
        }
        let mut rows = statement.query(params![device, after, sequence::STREAM])?;
        while let Some(row) = rows.next()? {
            let event = rows::stored_event(row)??;
            let body = event.body();
            pending.push(Pending {
                arrival: row.get(2)?,
                device: body.origin_device,
                position: body.origin_seq.get(),
                hash: event.hash(),
                business_date: body.business_date,
            });
        }
    }
    pending.sort_unstable_by_key(|event| event.arrival);
    Ok(pending)
}

/// The number after the last one the store's own records gave in `epoch`: 1 if they gave none.
fn next_number(db: &Connection, own: Id<Device>, epoch: u64) -> Result<u64, StoreError> {
    let last: Option<i64> = db
        .query_row(
            "SELECT number + (to_seq - from_seq) FROM sequence WHERE author = ?1 AND epoch = ?2 \
             ORDER BY number DESC LIMIT 1",
            params![&own.to_bytes()[..], seq_value(epoch)?],
            |row| row.get(0),
        )
        .optional()?;
    let last = last.map_or(Ok(0), |last| {
        u64::try_from(last).map_err(|_| StoreError::Corrupt("a store sequence number"))
    })?;
    Ok(last.saturating_add(1))
}

/// The runs of `pending`, record by record, each with the latest business date among its
/// events. A device's events side by side make one run; a record ends when it holds as many runs
/// as it may, or where a device's next run wouldn't follow on from its run before in the record.
fn records(pending: Vec<Pending>) -> Vec<(Vec<Run>, BusinessDate)> {
    let mut records = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut ends: BTreeMap<Id<Device>, u64> = BTreeMap::new();
    let mut date: Option<BusinessDate> = None;
    for event in pending {
        let follows = |end: u64| end.checked_add(1) == Some(event.position);
        match runs.last_mut() {
            Some(run) if run.device == event.device && follows(run.to) => {
                run.to = event.position;
                run.last = event.hash;
            }
            _ => {
                let fits = runs.len() < MAX_RUNS
                    && ends.get(&event.device).is_none_or(|&end| follows(end));
                if !fits && let Some(date) = date.take() {
                    records.push((core::mem::take(&mut runs), date));
                    ends.clear();
                }
                let (from, to, last) = (event.position, event.position, event.hash);
                runs.push(Run { device: event.device, from, to, last });
            }
        }
        ends.insert(event.device, event.position);
        date = Some(date.map_or(event.business_date, |date| date.max(event.business_date)));
    }
    if let Some(date) = date {
        records.push((runs, date));
    }
    records
}

/// Runs that confirm what they cover: runs that count, whose last event the store holds with the
/// hash the record gives it.
fn confirming() -> String {
    format!(
        "{COUNTING} JOIN events AS e ON e.origin_device = s.device AND e.origin_seq = s.to_seq \
         AND e.hash = s.last_hash"
    )
}

/// For each device, how far into its log the store holds confirmed events: the longest start of
/// its log in which every event is confirmed, or a record.
pub(crate) fn confirmed(db: &Connection) -> Result<BTreeMap<Id<Device>, u64>, StoreError> {
    let mut stretches: BTreeMap<Vec<u8>, Vec<(i64, i64)>> = BTreeMap::new();
    let mut statement =
        db.prepare(&format!("SELECT s.device, s.from_seq, s.to_seq FROM {}", confirming()))?;
    let runs = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    for run in runs {
        let (device, from, to) = run?;
        stretches.entry(device).or_default().push((from, to));
    }
    let mut statement =
        db.prepare("SELECT origin_device, origin_seq FROM events WHERE stream_kind = ?1")?;
    let records = statement.query_map([sequence::STREAM], |row| Ok((row.get(0)?, row.get(1)?)))?;
    for record in records {
        let (device, position): (Vec<u8>, i64) = record?;
        stretches.entry(device).or_default().push((position, position));
    }
    let mut confirmed = BTreeMap::new();
    for (device, mut stretches) in stretches {
        stretches.sort_unstable();
        let mut reach: i64 = 0;
        for (from, to) in stretches {
            if reach.checked_add(1).is_none_or(|next| from > next) {
                break;
            }
            reach = reach.max(to);
        }
        if reach > 0 {
            let reach = u64::try_from(reach).map_err(|_| StoreError::Corrupt("a position"))?;
            confirmed.insert(id(&device)?, reach);
        }
    }
    Ok(confirmed)
}

/// The epoch and number of the event at `position` of `device`'s log, once confirmed.
pub(crate) fn store_seq(
    db: &Connection,
    device: Id<Device>,
    position: u64,
) -> Result<Option<StoreSeq>, StoreError> {
    let Some(longest) = longest_run(db, device)? else { return Ok(None) };
    let position_value = seq_value(position)?;
    let found: Option<(i64, i64)> = db
        .prepare(&format!(
            "SELECT s.epoch, s.number + (?2 - s.from_seq) FROM {} WHERE \
                 s.device = ?1 AND s.to_seq BETWEEN ?2 AND ?3 AND s.from_seq <= ?2 \
                 ORDER BY s.epoch, s.number LIMIT 1",
            confirming()
        ))?
        .query_row(
            params![&device.to_bytes()[..], position_value, position_value.saturating_add(longest)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    found
        .map(|(epoch, number)| {
            Ok(StoreSeq {
                epoch: u64::try_from(epoch).map_err(|_| StoreError::Corrupt("an epoch"))?,
                number: u64::try_from(number)
                    .map_err(|_| StoreError::Corrupt("a store sequence number"))?,
            })
        })
        .transpose()
}

/// How many events past its first the longest run of `device`'s log reaches: a run covering a
/// position ends no further than this past it. `None` if no record covers any of its log.
fn longest_run(db: &Connection, device: Id<Device>) -> Result<Option<i64>, StoreError> {
    Ok(db
        .prepare(
            "SELECT to_seq - from_seq FROM sequence WHERE device = ?1 \
             ORDER BY to_seq - from_seq DESC LIMIT 1",
        )?
        .query_row([&device.to_bytes()[..]], |row| row.get(0))
        .optional()?)
}

/// A confirming run of the feed.
struct FeedRun {
    number: u64,
    device: Id<Device>,
    from: u64,
    to: u64,
}

/// Up to `limit` of the confirmed events of `epoch` numbered after `after`, in number order: the
/// store's feed. An event counts under its first number only.
pub(crate) fn sequenced(
    db: &Connection,
    epoch: u64,
    after: u64,
    limit: u32,
) -> Result<Vec<Sequenced>, StoreError> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let epoch_value = seq_value(epoch)?;
    // A run numbering past `after` starts no further back than the epoch's longest run reaches.
    let longest: Option<i64> = db
        .prepare(
            "SELECT to_seq - from_seq FROM sequence WHERE epoch = ?1 \
             ORDER BY to_seq - from_seq DESC LIMIT 1",
        )?
        .query_row([epoch_value], |row| row.get(0))
        .optional()?;
    let Some(longest) = longest else { return Ok(Vec::new()) };
    let mut runs = db.prepare(&format!(
        "SELECT s.number, s.device, s.from_seq, s.to_seq FROM {} WHERE s.epoch = ?1 \
         AND s.number > ?3 AND s.number + (s.to_seq - s.from_seq) > ?2 \
         ORDER BY s.number, s.record, s.run",
        confirming()
    ))?;
    let mut earlier = db.prepare(&format!(
        "SELECT s.from_seq, s.to_seq FROM {} WHERE s.device = ?1 \
         AND s.to_seq BETWEEN ?2 AND ?6 AND s.from_seq <= ?3 \
         AND (s.epoch < ?4 OR (s.epoch = ?4 AND s.number < ?5))",
        confirming()
    ))?;
    let mut events = db.prepare(
        "SELECT message, hash FROM events WHERE origin_device = ?1 AND origin_seq BETWEEN ?2 \
         AND ?3 ORDER BY origin_seq",
    )?;
    let after_value = i64::try_from(after).unwrap_or(i64::MAX);
    let mut rows = runs.query(params![
        epoch_value,
        after_value,
        after_value.saturating_sub(longest).saturating_sub(1)
    ])?;
    let mut feed = Vec::new();
    while feed.len() < limit
        && let Some(row) = rows.next()?
    {
        let run = FeedRun {
            number: unsigned(row.get(0)?)?,
            device: id(&row.get::<_, Vec<u8>>(1)?)?,
            from: unsigned(row.get(2)?)?,
            to: unsigned(row.get(3)?)?,
        };
        // The stretches of the run that a run numbered earlier confirms too.
        let device = run.device.to_bytes();
        let reach = longest_run(db, run.device)?.unwrap_or(0);
        let taken: Vec<(u64, u64)> = earlier
            .query_map(
                params![
                    &device[..],
                    seq_value(run.from)?,
                    seq_value(run.to)?,
                    epoch_value,
                    seq_value(run.number)?,
                    seq_value(run.to)?.saturating_add(reach)
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )?
            .map(|stretch| {
                let (from, to) = stretch?;
                Ok((unsigned(from)?, unsigned(to)?))
            })
            .collect::<Result<_, StoreError>>()?;
        // The run's events numbered `after` or below are skipped.
        let skipped = after.saturating_add(1).saturating_sub(run.number);
        let first = run.from.saturating_add(skipped);
        let stored = events.query_map(
            params![&device[..], seq_value(first)?, seq_value(run.to)?],
            rows::stored_event,
        )?;
        for event in stored {
            if feed.len() == limit {
                break;
            }
            let event = event??;
            let position = event.body().origin_seq.get();
            if taken.iter().any(|&(from, to)| (from..=to).contains(&position)) {
                continue;
            }
            let number = run.number.saturating_add(position.saturating_sub(run.from));
            feed.push(Sequenced { number, event });
        }
    }
    Ok(feed)
}

fn unsigned(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt("a store sequence number"))
}
