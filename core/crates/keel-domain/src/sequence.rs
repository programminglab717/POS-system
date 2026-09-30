//! Sequencing records: how the Store Hub numbers its location's events (ADR-0020).
//!
//! The hub numbers the events it receives, in the order it receives them, gapless within its
//! epoch, and records the numbers as events in its own log. Each record is a stream of its own,
//! of kind [`STREAM`], holding one `sequence.assigned` event. A record names the events it
//! numbers as runs: a device, the first and last positions of a stretch of its log, and the hash
//! of the stretch's last event, which pins the version of the stretch the hub numbered.
//!
//! | Schema | Keys |
//! |---|---|
//! | `sequence.assigned` | 1 epoch, 2 first number, 3 runs |
//!
//! A run is `[device, from, to, hash]`: a 16-byte identifier, two positions, and a 32-byte hash.
//! Beyond the types, a record must satisfy these rules:
//! - the epoch, the first number and every position are from 1 to 2^63 − 1, so that each fits
//!   the store, and so does the last number the record assigns;
//! - it has from 1 to [`MAX_RUNS`] runs, and each run starts no later than it ends;
//! - no two neighbouring runs are of one device: each run is as long as it can be;
//! - each device's runs follow on from each other: a device's run starts right after where its
//!   previous run in the record ended.
//!
//! Numbers never change how events fold: every replica folds in HLC order. They confirm events,
//! give the store a gapless feed, and order the hub's own decisions.

use std::collections::BTreeMap;

use keel_events::cbor::Value;
use keel_events::envelope::{Device, SchemaRef};
use keel_events::hash::EventHash;
use keel_types::Id;

use crate::codec::{Field, Fields, PayloadError, Record};
use crate::schema::{DecodeError, DomainEvent, SchemaId};

#[cfg(test)]
mod tests;

/// The kind of stream a sequencing record is.
pub const STREAM: &str = "sequence";

/// The most runs in one record. A record of this many runs is some 70 KiB, far below the event
/// size limit; a hub with more to sequence writes several records.
pub const MAX_RUNS: usize = 1024;

/// The largest epoch, number or position a record may hold: the largest the store can.
pub const MAX_NUMBER: u64 = (1 << 63) - 1;

const ASSIGNED: SchemaId = SchemaId { name: "sequence.assigned", version: 1 };

/// A stretch of a device's log, numbered in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Run {
    /// The device whose log it is.
    pub device: Id<Device>,
    /// The position of its first event.
    pub from: u64,
    /// The position of its last event: no earlier than `from`.
    pub to: u64,
    /// The hash of its last event, which pins the version of every event of the run.
    pub last: EventHash,
}

impl Run {
    /// How many events the run numbers.
    pub fn count(&self) -> u64 {
        self.to.saturating_sub(self.from).saturating_add(1)
    }

    /// Whether the run covers the event at `position` of `device`'s log.
    pub fn covers(&self, device: Id<Device>, position: u64) -> bool {
        self.device == device && (self.from..=self.to).contains(&position)
    }
}

/// The hub numbered events: `first` for the first event of its first run, and on from there,
/// run by run, each run in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assigned {
    /// The hub's epoch.
    pub epoch: u64,
    /// The number of the record's first event.
    pub first: u64,
    /// The events, run by run, in the order the hub received them.
    pub runs: Vec<Run>,
}

impl Assigned {
    /// A record, checked against the rules above.
    ///
    /// # Errors
    /// [`PayloadError::Invalid`] naming the rule the record breaks.
    pub fn new(epoch: u64, first: u64, runs: Vec<Run>) -> Result<Assigned, PayloadError> {
        let assigned = Assigned { epoch, first, runs };
        assigned.check()?;
        Ok(assigned)
    }

    /// How many events the record numbers.
    pub fn count(&self) -> u64 {
        self.runs.iter().fold(0_u64, |count, run| count.saturating_add(run.count()))
    }

    /// The number of the record's last event.
    pub fn last(&self) -> u64 {
        self.first.saturating_add(self.count()).saturating_sub(1)
    }

    /// Each run, with the number of its first event.
    pub fn numbered(&self) -> impl Iterator<Item = (u64, &Run)> {
        self.runs.iter().scan(self.first, |next, run| {
            let number = *next;
            *next = next.saturating_add(run.count());
            Some((number, run))
        })
    }

    /// The number the record gives the event at `position` of `device`'s log, if it covers it.
    pub fn number_of(&self, device: Id<Device>, position: u64) -> Option<u64> {
        self.numbered()
            .find(|(_, run)| run.covers(device, position))
            .map(|(number, run)| number.saturating_add(position.saturating_sub(run.from)))
    }

    fn check(&self) -> Result<(), PayloadError> {
        let in_range = |n: u64| (1..=MAX_NUMBER).contains(&n);
        if !in_range(self.epoch) {
            return Err(PayloadError::Invalid("epoch"));
        }
        if !in_range(self.first) {
            return Err(PayloadError::Invalid("first"));
        }
        if self.runs.is_empty() || self.runs.len() > MAX_RUNS {
            return Err(PayloadError::Invalid("runs"));
        }
        let mut count: u64 = 0;
        let mut previous: Option<Id<Device>> = None;
        let mut ends: BTreeMap<Id<Device>, u64> = BTreeMap::new();
        for run in &self.runs {
            if !in_range(run.from) || !in_range(run.to) || run.from > run.to {
                return Err(PayloadError::Invalid("run"));
            }
            if previous == Some(run.device) {
                return Err(PayloadError::Invalid("runs of one device side by side"));
            }
            if let Some(end) = ends.insert(run.device, run.to)
                && end.checked_add(1) != Some(run.from)
            {
                return Err(PayloadError::Invalid("runs that don't follow on"));
            }
            previous = Some(run.device);
            count = count.checked_add(run.count()).ok_or(PayloadError::Invalid("last number"))?;
        }
        let last = count.checked_sub(1).and_then(|after| self.first.checked_add(after));
        if !last.is_some_and(in_range) {
            return Err(PayloadError::Invalid("last number"));
        }
        Ok(())
    }
}

/// What can happen to a sequencing record: it is written, once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceEvent {
    /// The hub numbered events.
    Assigned(Assigned),
}

/// An epoch or a number, as encoded: the record's rules judge its value ([`Assigned::new`]).
struct Number(u64);

impl Field for Number {
    fn to_value(&self) -> Value {
        Value::Unsigned(self.0)
    }

    fn from_value(value: &Value) -> Option<Number> {
        value.as_u64().map(Number)
    }
}

impl Field for Run {
    fn to_value(&self) -> Value {
        Value::Array(vec![
            self.device.to_value(),
            Value::Unsigned(self.from),
            Value::Unsigned(self.to),
            Value::Bytes(self.last.as_bytes().to_vec()),
        ])
    }

    /// A run's shape; the record's rules judge its positions ([`Assigned::new`]).
    fn from_value(value: &Value) -> Option<Run> {
        let [device, from, to, last] = value.as_array()? else { return None };
        let last = <[u8; 32]>::try_from(last.as_bytes()?).ok()?;
        Some(Run {
            device: Id::from_value(device)?,
            from: from.as_u64()?,
            to: to.as_u64()?,
            last: EventHash::from_bytes(last),
        })
    }
}

impl DomainEvent for SequenceEvent {
    const STREAM: &'static str = STREAM;

    const SCHEMAS: &'static [SchemaId] = &[ASSIGNED];

    fn schema(&self) -> SchemaId {
        match self {
            SequenceEvent::Assigned(_) => ASSIGNED,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            SequenceEvent::Assigned(assigned) => Record::default()
                .field(1, &Number(assigned.epoch))
                .field(2, &Number(assigned.first))
                .field(3, &assigned.runs)
                .build(),
        }
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<SequenceEvent, DecodeError> {
        if !ASSIGNED.matches(schema) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let epoch: Number = fields.required(1, "epoch")?;
        let first: Number = fields.required(2, "first")?;
        let runs: Vec<Run> = fields.required(3, "runs")?;
        fields.finish()?;
        Ok(SequenceEvent::Assigned(Assigned::new(epoch.0, first.0, runs)?))
    }
}
