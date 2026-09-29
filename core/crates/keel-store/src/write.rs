//! Writing: the device's own events, events received from other replicas, and the outbox.

use keel_domain::aggregate::Aggregate;
use keel_events::envelope::{Location, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::keys::Signer;
use keel_events::log::{ChainError, EventDraft, Link, LogHead, LogWriter};
use keel_events::verify::{DeviceRegistry, Rejection};
use keel_types::{Entropy, Id, Timestamp};
use rusqlite::Transaction;

use crate::error::StoreError;
use crate::faults::{Faults, Point, proceed};
use crate::outbox::{self, Effect, EffectState, Enqueued, Queued};
use crate::projection;
use crate::rows;

/// A write in progress: everything it stores commits together, or not at all. See
/// [`crate::Store::write`].
pub struct Writing<'a, S, E> {
    pub(crate) tx: &'a Transaction<'a>,
    pub(crate) writer: &'a mut LogWriter<S, E>,
    pub(crate) faults: &'a mut dyn Faults,
    pub(crate) location: Id<Location>,
    /// The streams the write stored events in, in the order it first did.
    pub(crate) touched: Vec<StreamRef>,
}

/// What became of a received event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Received {
    /// Stored: the next event of its device's log.
    Stored(Box<SignedEvent>),
    /// Already stored: nothing changed.
    Duplicate,
    /// Events are missing before it, so it wasn't stored. Ask for its device's log after `head`
    /// again, in order.
    Gap {
        /// The last event of its device's log that the store holds.
        head: u64,
    },
    /// Refused, and kept in the quarantine for a person to look at.
    Quarantined(Reason),
}

/// Why a received event was quarantined.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Reason {
    /// The bytes aren't a well-formed event.
    Malformed,
    /// No enrolled device has signed it.
    UnknownDevice,
    /// Its signature doesn't verify with its device's key.
    BadSignature,
    /// Its device is enrolled at another location than the event names.
    WrongLocation,
    /// Its device was revoked before it.
    Revoked,
    /// The device registry refused it for another reason.
    Rejected,
    /// It names another location than the store's.
    OtherLocation,
    /// The store holds a different event at its position: its device's log forked. The first
    /// event stays.
    Fork,
    /// It doesn't link to the event before it in its device's log.
    BrokenLink,
    /// Its HLC isn't later than the event's before it.
    ClockRegressed,
    /// It doesn't fit its device's log for another reason.
    Unlinked,
    /// Another stored event has its identifier.
    DuplicateId,
    /// Its sequence number is beyond what the store holds, 2^63 − 1.
    OutOfRange,
}

impl Reason {
    /// Every reason.
    pub const ALL: [Reason; 13] = [
        Reason::Malformed,
        Reason::UnknownDevice,
        Reason::BadSignature,
        Reason::WrongLocation,
        Reason::Revoked,
        Reason::Rejected,
        Reason::OtherLocation,
        Reason::Fork,
        Reason::BrokenLink,
        Reason::ClockRegressed,
        Reason::Unlinked,
        Reason::DuplicateId,
        Reason::OutOfRange,
    ];

    /// The reason's code, as the quarantine stores it.
    pub const fn code(self) -> &'static str {
        match self {
            Reason::Malformed => "malformed",
            Reason::UnknownDevice => "unknown_device",
            Reason::BadSignature => "bad_signature",
            Reason::WrongLocation => "wrong_location",
            Reason::Revoked => "revoked",
            Reason::Rejected => "rejected",
            Reason::OtherLocation => "other_location",
            Reason::Fork => "fork",
            Reason::BrokenLink => "broken_link",
            Reason::ClockRegressed => "clock_regressed",
            Reason::Unlinked => "unlinked",
            Reason::DuplicateId => "duplicate_id",
            Reason::OutOfRange => "out_of_range",
        }
    }

    /// The reason with code `code`.
    pub fn from_code(code: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|reason| reason.code() == code)
    }

    fn of_rejection(rejection: &Rejection) -> Reason {
        match rejection {
            Rejection::Malformed(_) => Reason::Malformed,
            Rejection::UnknownDevice(_) => Reason::UnknownDevice,
            Rejection::Signature(_) => Reason::BadSignature,
            Rejection::WrongLocation => Reason::WrongLocation,
            Rejection::Revoked { .. } => Reason::Revoked,
            _ => Reason::Rejected,
        }
    }

    fn of_chain(error: &ChainError) -> Reason {
        match error {
            ChainError::Fork { .. } => Reason::Fork,
            ChainError::BrokenLink { .. } => Reason::BrokenLink,
            ChainError::ClockRegressed { .. } => Reason::ClockRegressed,
            _ => Reason::Unlinked,
        }
    }
}

/// A message in the quarantine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quarantined {
    /// Why it was refused.
    pub reason: Reason,
    /// The message as received.
    pub message: Vec<u8>,
}

impl<S: Signer, E: Entropy> Writing<'_, S, E> {
    /// Makes the device's next event from `draft`, at physical time `now`, signs it and stores
    /// it. It leaves the store only once the write commits: don't send it anywhere before.
    ///
    /// # Errors
    /// [`StoreError::Append`] if the log writer can't make the event, [`StoreError::Database`]
    /// if it can't be stored, and [`StoreError::Interrupted`] if a fault hook interrupts the
    /// write. The write then fails as a whole.
    pub fn append(&mut self, draft: EventDraft, now: Timestamp) -> Result<SignedEvent, StoreError> {
        let pending = self.writer.prepare(draft, now)?;
        rows::insert(self.tx, pending.event())?;
        let event = pending.commit();
        self.touch(&event);
        proceed(self.faults, Point::Stored)?;
        Ok(event)
    }

    /// Marks the stream of `event`, just stored, as touched.
    fn touch(&mut self, event: &SignedEvent) {
        let stream = &event.body().stream;
        if !self.touched.contains(stream) {
            self.touched.push(stream.clone());
        }
    }

    /// The streams the write has stored events in so far, in the order it first did. Their
    /// projections are recomputed when the write commits: return them from the write to show
    /// them again.
    pub fn touched(&self) -> &[StreamRef] {
        &self.touched
    }

    /// Folds `aggregate` from its stream's events in canonical order, as the write sees them: the
    /// events it stored itself included.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the stream can't be read, and [`StoreError::Corrupt`] if an
    /// event doesn't read back as stored.
    pub fn load<A: Aggregate>(&self, aggregate: A) -> Result<A, StoreError> {
        projection::load(self.tx, aggregate)
    }

    /// Enqueues `effect`, due at `now`, to commit with the write. Enqueuing an effect the outbox
    /// already holds, with the same key, changes nothing.
    ///
    /// # Errors
    /// [`StoreError::Effect`] if another effect has the key, the event that caused it isn't
    /// stored, or its key or payload is out of bounds; [`StoreError::Database`] if the outbox
    /// can't be read or written.
    pub fn enqueue(&mut self, effect: &Effect, now: Timestamp) -> Result<Enqueued, StoreError> {
        outbox::enqueue(self.tx, effect, now)
    }

    /// Starts the pending effect with key `key` at `now`, counting an attempt. Act on it only once
    /// the write has committed.
    ///
    /// # Errors
    /// [`StoreError::Effect`] if no effect has the key, it isn't pending, or it isn't due by
    /// `now`; [`StoreError::Database`] if the outbox can't be read or written.
    pub fn start(&mut self, key: &[u8], now: Timestamp) -> Result<Queued, StoreError> {
        outbox::start(self.tx, key, now)
    }

    /// Finishes the running effect with key `key`: it is done.
    ///
    /// # Errors
    /// [`StoreError::Effect`] if no effect has the key or it isn't running;
    /// [`StoreError::Database`] if the outbox can't be read or written.
    pub fn finish(&mut self, key: &[u8]) -> Result<(), StoreError> {
        outbox::end(self.tx, key, EffectState::Done)
    }

    /// Makes the running effect with key `key` pending again, due at `due`.
    ///
    /// # Errors
    /// As [`Writing::finish`].
    pub fn retry(&mut self, key: &[u8], due: Timestamp) -> Result<(), StoreError> {
        outbox::retry(self.tx, key, due)
    }

    /// Fails the running effect with key `key` for good, for a person to look at.
    ///
    /// # Errors
    /// As [`Writing::finish`].
    pub fn fail(&mut self, key: &[u8]) -> Result<(), StoreError> {
        outbox::end(self.tx, key, EffectState::Failed)
    }

    /// Takes in `bytes`, an event received from another replica at physical time `now`. The
    /// event is verified with `registry`, and must be for the store's location. The next event of
    /// its device's log is stored, and the device's clock observes it; a duplicate changes
    /// nothing; an event after a gap isn't stored; and anything else is quarantined.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the store can't be read or written, and
    /// [`StoreError::Interrupted`] if a fault hook interrupts the write. A refused event is not
    /// an error: it is quarantined.
    pub fn receive(
        &mut self,
        bytes: &[u8],
        registry: &DeviceRegistry,
        now: Timestamp,
    ) -> Result<Received, StoreError> {
        let event = match registry.verify(bytes) {
            Ok(event) => event,
            Err(rejection) => return self.quarantine(bytes, Reason::of_rejection(&rejection)),
        };
        let body = event.body();
        if body.location != self.location {
            return self.quarantine(bytes, Reason::OtherLocation);
        }
        let seq = body.origin_seq.get();
        if rows::seq_value(seq).is_err() {
            return self.quarantine(bytes, Reason::OutOfRange);
        }
        let device = body.origin_device;
        let head = rows::head(self.tx, device)?;
        match head.link(&event) {
            Ok(Link::Next) if rows::has_id(self.tx, &body.event_id.to_bytes())? => {
                self.quarantine(bytes, Reason::DuplicateId)
            }
            Ok(Link::Next) => {
                rows::insert(self.tx, &event)?;
                self.touch(&event);
                if device == self.writer.device() {
                    // The device's own log, as another replica held it: continue after it.
                    let clock = self.writer.latest_hlc();
                    self.writer.restore(LogHead::of(&event), clock);
                } else {
                    // A clock too far ahead doesn't drag the device's along; the event still
                    // counts.
                    let _ = self.writer.observe(body.hlc, now);
                }
                proceed(self.faults, Point::Stored)?;
                Ok(Received::Stored(Box::new(event)))
            }
            Ok(Link::Duplicate) => Ok(Received::Duplicate),
            Ok(Link::Earlier) => match rows::hash_at(self.tx, device, seq)? {
                Some(hash) if hash == event.hash() => Ok(Received::Duplicate),
                Some(_) => self.quarantine(bytes, Reason::Fork),
                None => Err(StoreError::Corrupt("a gap in a stored log")),
            },
            Err(ChainError::Gap { head, .. }) => Ok(Received::Gap { head }),
            Err(error) => self.quarantine(bytes, Reason::of_chain(&error)),
        }
    }

    /// Keeps `bytes` in the quarantine for `reason`, once.
    fn quarantine(&mut self, bytes: &[u8], reason: Reason) -> Result<Received, StoreError> {
        let digest = EventHash::of(bytes);
        self.tx.execute(
            "INSERT INTO quarantine (digest, reason, message) VALUES (?1, ?2, ?3) \
             ON CONFLICT (digest) DO NOTHING",
            rusqlite::params![&digest.as_bytes()[..], reason.code(), bytes],
        )?;
        proceed(self.faults, Point::Stored)?;
        Ok(Received::Quarantined(reason))
    }
}
