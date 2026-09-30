//! A device's log: appending to it, and checking where received events fit in it.
//!
//! Each device appends events to its own log. The n-th event has origin sequence number n, records
//! the hash of the event before it, and has a later hybrid logical clock (HLC) than the event
//! before it. [`LogWriter`] keeps these rules as the device appends, and [`LogHead::link`] checks
//! them for events received from other devices. Together with the signatures, they make the log
//! tamper-evident: changing, removing, reordering or inserting an event breaks a link that every
//! replica checks.

use core::num::NonZeroU64;
use core::time::Duration;

use keel_types::{
    BusinessDate, Entropy, Hlc, HlcClock, HlcError, Id, IdError, IdGenerator, Timestamp,
};

use crate::envelope::{
    Actor, Cause, Correlation, Device, Event, EventBody, Location, Payload, SchemaRef, StreamRef,
};
use crate::event::{MAX_EVENT_BYTES, SignedEvent};
use crate::hash::EventHash;
use crate::keys::{SignError, Signer};

/// The last event of a device's log, as a writer or replica holds it: all it takes to check or
/// extend the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogHead {
    seq: u64,
    hash: EventHash,
    hlc: Hlc,
}

impl LogHead {
    /// The head of an empty log. The first event has sequence number 1 and previous hash
    /// [`EventHash::ZERO`].
    pub const EMPTY: LogHead = LogHead { seq: 0, hash: EventHash::ZERO, hlc: Hlc::ZERO };

    /// The head of a log as a store keeps it: its last event's sequence number, hash and HLC.
    /// A sequence number of 0 is an empty log, whose hash is [`EventHash::ZERO`].
    pub const fn from_parts(seq: u64, hash: EventHash, hlc: Hlc) -> LogHead {
        LogHead { seq, hash, hlc }
    }

    /// The head of a log whose last event is `event`.
    pub fn of(event: &SignedEvent) -> LogHead {
        let body = event.body();
        LogHead { seq: body.origin_seq.get(), hash: event.hash(), hlc: body.hlc }
    }

    /// The last event's sequence number: the log's length. 0 for an empty log.
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// The last event's hash, which the next event records as its previous hash.
    pub const fn hash(&self) -> EventHash {
        self.hash
    }

    /// The last event's HLC. The next event's HLC must be later.
    pub const fn hlc(&self) -> Hlc {
        self.hlc
    }

    /// Where `event`, from this log's device, fits in the log that ends at this head.
    ///
    /// # Errors
    /// [`ChainError`] if the event can't be the next one, and isn't this head either.
    pub fn link(&self, event: &SignedEvent) -> Result<Link, ChainError> {
        let body = event.body();
        let seq = body.origin_seq.get();
        if seq < self.seq {
            return Ok(Link::Earlier);
        }
        if seq == self.seq {
            return if event.hash() == self.hash {
                Ok(Link::Duplicate)
            } else {
                Err(ChainError::Fork { seq })
            };
        }
        if self.seq.checked_add(1) != Some(seq) {
            return Err(ChainError::Gap { head: self.seq, seq });
        }
        if body.prev_hash != self.hash {
            return Err(ChainError::BrokenLink { seq });
        }
        if body.hlc <= self.hlc {
            return Err(ChainError::ClockRegressed { seq });
        }
        Ok(Link::Next)
    }
}

/// Where a received event fits in its device's log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// The next event: append it.
    Next,
    /// The head itself, received again: ignore it.
    Duplicate,
    /// An event before the head: compare its hash with the stored event at its sequence number.
    /// The same hash is a duplicate, and a different one a fork.
    Earlier,
}

/// Why a received event can't extend its device's log.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ChainError {
    /// Events are missing between the head and this event. Each device's log is delivered in
    /// order, so the missing events should be requested again.
    #[error("event {seq} arrived, but the log ends at event {head}")]
    Gap {
        /// The head's sequence number.
        head: u64,
        /// The received event's sequence number.
        seq: u64,
    },
    /// A different event already holds this position: the device's log has forked.
    #[error("a different event {seq} is already in the log")]
    Fork {
        /// The contested sequence number.
        seq: u64,
    },
    /// The event doesn't record the hash of the event before it: the device's log has forked,
    /// or the event is forged.
    #[error("event {seq} doesn't link to the event before it")]
    BrokenLink {
        /// The received event's sequence number.
        seq: u64,
    },
    /// The event's HLC isn't later than the HLC of the event before it.
    #[error("event {seq} has an HLC no later than the event before it")]
    ClockRegressed {
        /// The received event's sequence number.
        seq: u64,
    },
}

/// What an event is about: everything in its body that the log writer doesn't fill in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventDraft {
    /// The stream (aggregate) the event belongs to.
    pub stream: StreamRef,
    /// The payload's schema.
    pub schema: SchemaRef,
    /// The business date the event is reported under.
    pub business_date: BusinessDate,
    /// Who or what caused the event.
    pub actor: Actor,
    /// The event granting approval, when the action needed a manager's approval.
    pub approval: Option<Id<Event>>,
    /// The command or event that caused this event.
    pub causation: Option<Id<Cause>>,
    /// Groups the events of one user action across aggregates.
    pub correlation: Option<Id<Correlation>>,
    /// The event-specific data.
    pub payload: Payload,
}

/// Everything a [`LogWriter`] needs besides its signer and entropy.
#[derive(Clone, Copy, Debug)]
pub struct LogConfig {
    /// The device whose log this is.
    pub device: Id<Device>,
    /// The location the device is enrolled at.
    pub location: Id<Location>,
    /// The log's last event: [`LogHead::EMPTY`] for a new log.
    pub head: LogHead,
    /// The latest HLC the device has issued or seen, including on events received from other
    /// devices. New events are later still, so they follow everything the device knows of.
    pub latest_hlc: Hlc,
    /// How far ahead of physical time a remote HLC may be (see [`HlcClock::new`]).
    pub max_forward_drift: Duration,
}

/// Appends events to a device's own log: numbers, chains, timestamps and signs them.
///
/// Appending takes two steps, so the log in memory never runs ahead of the log in storage.
/// [`LogWriter::prepare`] makes and signs the next event; the caller stores it durably, then calls
/// [`PendingEvent::commit`] to make it the head. An event that is dropped instead of committed is
/// forgotten, and the next event takes its position, so an event must never be sent anywhere
/// before it is committed. If storing fails in a way that leaves it unclear whether the event was
/// stored, make a new writer from the stored log.
pub struct LogWriter<S, E> {
    device: Id<Device>,
    location: Id<Location>,
    signer: S,
    ids: IdGenerator<E>,
    clock: HlcClock,
    max_forward_drift: Duration,
    head: LogHead,
}

impl<S: Signer, E: Entropy> LogWriter<S, E> {
    /// A writer appending to the log described by `config`, signing with `signer` (the device's
    /// key) and drawing event identifiers from `entropy`.
    pub fn new(config: LogConfig, signer: S, entropy: E) -> LogWriter<S, E> {
        let latest = config.latest_hlc.max(config.head.hlc);
        LogWriter {
            device: config.device,
            location: config.location,
            signer,
            ids: IdGenerator::new(entropy),
            clock: HlcClock::resume(latest, config.max_forward_drift),
            max_forward_drift: config.max_forward_drift,
            head: config.head,
        }
    }

    /// The device whose log this is.
    pub const fn device(&self) -> Id<Device> {
        self.device
    }

    /// The last committed event.
    pub const fn head(&self) -> LogHead {
        self.head
    }

    /// The latest HLC the writer has issued or observed. Store it with the log, and pass it as
    /// [`LogConfig::latest_hlc`] when resuming, so the device's HLCs keep increasing.
    pub const fn latest_hlc(&self) -> Hlc {
        self.clock.last()
    }

    /// Resets the writer to its log as stored: `head`, the log's last event, and `latest_hlc`,
    /// the latest HLC stored with it, as [`LogWriter::new`] resumes them. After a transaction
    /// that stored the writer's events failed, both go back, since nothing the writer made in it
    /// was kept or sent; after the device's own events arrived from another replica, both move
    /// forward.
    pub fn restore(&mut self, head: LogHead, latest_hlc: Hlc) {
        self.head = head;
        self.clock = HlcClock::resume(latest_hlc.max(head.hlc), self.max_forward_drift);
    }

    /// Takes in the HLC of an event received from another device at physical time `now`, so the
    /// device's next events are later than it.
    ///
    /// # Errors
    /// As [`HlcClock::observe`]: [`HlcError::ClockDrift`] if `remote` is too far ahead of `now`.
    pub fn observe(&mut self, remote: Hlc, now: Timestamp) -> Result<Hlc, HlcError> {
        self.clock.observe(remote, now)
    }

    /// A fresh identifier, drawn at physical time `now` as the writer draws its events'
    /// identifiers: for an aggregate the device creates as it writes, such as a sequencing
    /// record's stream.
    ///
    /// # Errors
    /// [`IdError`] if the entropy source fails.
    pub fn generate_id<T>(&mut self, now: Timestamp) -> Result<Id<T>, IdError> {
        self.ids.generate(now)
    }

    /// Makes and signs the next event from `draft`, at physical time `now`. It becomes part of the
    /// log only when committed.
    ///
    /// # Errors
    /// [`AppendError`] if the clock, identifier generator or signer fails, or the log is full.
    pub fn prepare(
        &mut self,
        draft: EventDraft,
        now: Timestamp,
    ) -> Result<PendingEvent<'_, S, E>, AppendError> {
        let seq =
            self.head.seq.checked_add(1).and_then(NonZeroU64::new).ok_or(AppendError::Full)?;
        let hlc = self.clock.tick(now)?;
        let event_id = self.ids.generate(now)?;
        let body = EventBody {
            event_id,
            location: self.location,
            stream: draft.stream,
            schema: draft.schema,
            origin_device: self.device,
            origin_seq: seq,
            hlc,
            business_date: draft.business_date,
            actor: draft.actor,
            approval: draft.approval,
            causation: draft.causation,
            correlation: draft.correlation,
            payload: draft.payload,
            prev_hash: self.head.hash,
        };
        let event = SignedEvent::sign(body, &self.signer)?;
        if event.to_bytes().len() > MAX_EVENT_BYTES {
            return Err(AppendError::TooLarge);
        }
        Ok(PendingEvent { writer: self, event })
    }
}

impl<S, E> core::fmt::Debug for LogWriter<S, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LogWriter")
            .field("device", &self.device)
            .field("location", &self.location)
            .field("head", &self.head)
            .finish_non_exhaustive()
    }
}

/// A signed event that isn't in the log yet: store it durably, then commit it.
#[must_use = "an event is only appended once it is committed"]
pub struct PendingEvent<'a, S, E> {
    writer: &'a mut LogWriter<S, E>,
    event: SignedEvent,
}

impl<S, E> core::fmt::Debug for PendingEvent<'_, S, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PendingEvent").field("event", &self.event).finish_non_exhaustive()
    }
}

impl<S, E> PendingEvent<'_, S, E> {
    /// The event, to store.
    pub const fn event(&self) -> &SignedEvent {
        &self.event
    }

    /// Makes the event the head of the log, once it is stored.
    pub fn commit(self) -> SignedEvent {
        self.writer.head = LogHead::of(&self.event);
        self.event
    }
}

/// Why an event couldn't be appended.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AppendError {
    /// The hybrid logical clock can't advance.
    #[error(transparent)]
    Clock(#[from] HlcError),
    /// No identifier could be generated.
    #[error(transparent)]
    Id(#[from] IdError),
    /// The signer failed.
    #[error(transparent)]
    Sign(#[from] SignError),
    /// The log has reached the largest sequence number.
    #[error("the log is full")]
    Full,
    /// The event would be larger than [`MAX_EVENT_BYTES`].
    #[error("the event is larger than {MAX_EVENT_BYTES} bytes")]
    TooLarge,
}

#[cfg(test)]
mod tests {
    use keel_types::SeededEntropy;

    use super::*;
    use crate::event::golden;
    use crate::keys::SoftwareSigner;

    fn draft() -> EventDraft {
        let body = golden::body();
        EventDraft {
            stream: body.stream,
            schema: body.schema,
            business_date: body.business_date,
            actor: body.actor,
            approval: body.approval,
            causation: body.causation,
            correlation: body.correlation,
            payload: body.payload,
        }
    }

    fn config(head: LogHead) -> LogConfig {
        let body = golden::body();
        LogConfig {
            device: body.origin_device,
            location: body.location,
            head,
            latest_hlc: Hlc::ZERO,
            max_forward_drift: Duration::from_secs(60),
        }
    }

    fn writer(head: LogHead) -> LogWriter<SoftwareSigner, SeededEntropy> {
        LogWriter::new(config(head), golden::signer(), SeededEntropy::new(1))
    }

    /// Microseconds since the Unix epoch on 2026-09-27.
    const BASE: i64 = 1_790_517_780_000_000;

    /// A draft whose payload is a byte string of `len` bytes.
    fn draft_of(len: usize) -> EventDraft {
        let payload = Payload::new(&crate::cbor::Value::Bytes(vec![0x5A; len])).unwrap();
        EventDraft { payload, ..draft() }
    }

    /// The first event a fresh writer makes from `draft`, or why it can't.
    fn first(draft: EventDraft) -> Result<SignedEvent, AppendError> {
        writer(LogHead::EMPTY).prepare(draft, at(0)).map(PendingEvent::commit)
    }

    #[test]
    fn an_event_is_at_most_max_event_bytes() {
        // What an event holds besides its payload's bytes is the same for every payload from
        // 64 KiB up: the headers of the byte strings holding it are all five bytes long.
        let sample = first(draft_of(100_000)).unwrap().to_bytes().len();
        let overhead = sample.checked_sub(100_000).unwrap();
        let largest = MAX_EVENT_BYTES.checked_sub(overhead).unwrap();
        assert_eq!(first(draft_of(largest)).unwrap().to_bytes().len(), MAX_EVENT_BYTES);
        assert_eq!(first(draft_of(largest.checked_add(1).unwrap())), Err(AppendError::TooLarge));
    }

    fn at(seconds: i64) -> Timestamp {
        let micros = seconds.checked_mul(1_000_000).and_then(|micros| micros.checked_add(BASE));
        Timestamp::from_micros(micros.unwrap()).unwrap()
    }

    #[test]
    fn events_are_numbered_chained_and_timestamped() {
        let mut writer = writer(LogHead::EMPTY);
        assert_eq!(writer.head(), LogHead::EMPTY);
        let first = writer.prepare(draft(), at(0)).unwrap().commit();
        // The wall clock goes backwards: the HLC still moves forward.
        let second = writer.prepare(draft(), at(-5)).unwrap().commit();
        assert_eq!(first.body().origin_seq.get(), 1);
        assert_eq!(first.body().prev_hash, EventHash::ZERO);
        assert_eq!(second.body().origin_seq.get(), 2);
        assert_eq!(second.body().prev_hash, first.hash());
        assert!(second.body().hlc > first.body().hlc);
        assert!(second.body().event_id > first.body().event_id);
        assert_eq!(writer.head(), LogHead::of(&second));
        assert_eq!(writer.head().seq(), 2);
        assert_eq!(writer.head().hash(), second.hash());
        assert_eq!(writer.head().hlc(), second.body().hlc);
        assert_eq!(writer.device(), golden::body().origin_device);
        assert_eq!(second.body().location, golden::body().location);
    }

    #[test]
    fn identifiers_drawn_between_events_are_fresh_and_in_order() {
        let mut writer = writer(LogHead::EMPTY);
        let before: Id<()> = writer.generate_id(at(0)).unwrap();
        let event = writer.prepare(draft(), at(0)).unwrap().commit();
        let after: Id<()> = writer.generate_id(at(0)).unwrap();
        let event_id = event.body().event_id.to_bytes();
        assert!(before.to_bytes() < event_id && event_id < after.to_bytes());
        // Drawing one changes nothing of the log.
        assert_eq!(writer.head(), LogHead::of(&event));
    }

    #[test]
    fn an_uncommitted_event_is_forgotten() {
        let mut writer = writer(LogHead::EMPTY);
        let first = writer.prepare(draft(), at(0)).unwrap().commit();
        let dropped = writer.prepare(draft(), at(1)).unwrap().event().clone();
        assert_eq!(writer.head(), LogHead::of(&first));
        let second = writer.prepare(draft(), at(2)).unwrap().commit();
        assert_eq!(second.body().origin_seq, dropped.body().origin_seq);
        assert_eq!(second.body().prev_hash, dropped.body().prev_hash);
        assert_ne!(second.hash(), dropped.hash());
    }

    #[test]
    fn a_resumed_writer_continues_the_log() {
        let mut writer = writer(LogHead::EMPTY);
        let first = writer.prepare(draft(), at(10)).unwrap().commit();
        // Resumed with an earlier physical time and no other HLC: still after the head.
        let mut resumed =
            LogWriter::new(config(LogHead::of(&first)), golden::signer(), SeededEntropy::new(2));
        let second = resumed.prepare(draft(), at(0)).unwrap().commit();
        assert_eq!(LogHead::of(&first).link(&second), Ok(Link::Next));
    }

    #[test]
    fn events_follow_observed_clocks() {
        let mut writer = writer(LogHead::EMPTY);
        let remote = Hlc::new(u64::try_from(at(30).as_millis()).unwrap(), 7).unwrap();
        writer.observe(remote, at(0)).unwrap();
        let event = writer.prepare(draft(), at(0)).unwrap().commit();
        assert!(event.body().hlc > remote);
        // So does a writer resumed with a later latest HLC than its head.
        let mut config = config(LogHead::of(&event));
        config.latest_hlc = Hlc::new(u64::try_from(at(60).as_millis()).unwrap(), 0).unwrap();
        let mut resumed = LogWriter::new(config, golden::signer(), SeededEntropy::new(3));
        let next = resumed.prepare(draft(), at(0)).unwrap().commit();
        assert!(next.body().hlc > config.latest_hlc);
        let too_far = Hlc::new(u64::try_from(at(3600).as_millis()).unwrap(), 0).unwrap();
        assert!(matches!(resumed.observe(too_far, at(0)), Err(HlcError::ClockDrift { .. })));
    }

    #[test]
    fn a_restored_writer_continues_from_the_stored_head() {
        let mut writer = writer(LogHead::EMPTY);
        let first = writer.prepare(draft(), at(0)).unwrap().commit();
        let stored_hlc = writer.latest_hlc();
        let lost = writer.prepare(draft(), at(10)).unwrap().commit();
        // The transaction storing the second event failed: back to the first, clock included.
        writer.restore(LogHead::of(&first), stored_hlc);
        assert_eq!(writer.head(), LogHead::of(&first));
        assert_eq!(writer.latest_hlc(), first.body().hlc);
        let second = writer.prepare(draft(), at(1)).unwrap().commit();
        assert_eq!(LogHead::of(&first).link(&second), Ok(Link::Next));
        assert!(second.body().hlc < lost.body().hlc, "the lost event's time was given back");

        // The device's own later events arrive from another replica, with an HLC far ahead of
        // this writer's clock: the writer continues after them, without a drift check.
        let mut elsewhere =
            LogWriter::new(config(LogHead::of(&second)), golden::signer(), SeededEntropy::new(5));
        let third = elsewhere.prepare(draft(), at(7_200)).unwrap().commit();
        writer.restore(LogHead::of(&third), writer.latest_hlc());
        assert_eq!(writer.latest_hlc(), third.body().hlc);
        let fourth = writer.prepare(draft(), at(0)).unwrap().commit();
        assert_eq!(LogHead::of(&third).link(&fourth), Ok(Link::Next));
        // A stored clock later than the head is kept.
        let later = Hlc::new(u64::try_from(at(9_000).as_millis()).unwrap(), 0).unwrap();
        writer.restore(LogHead::of(&fourth), later);
        assert_eq!(writer.latest_hlc(), later);
    }

    #[test]
    fn a_head_rebuilds_from_its_parts() {
        let mut writer = writer(LogHead::EMPTY);
        let event = writer.prepare(draft(), at(0)).unwrap().commit();
        let head = LogHead::of(&event);
        assert_eq!(LogHead::from_parts(head.seq(), head.hash(), head.hlc()), head);
        assert_eq!(LogHead::from_parts(0, EventHash::ZERO, Hlc::ZERO), LogHead::EMPTY);
    }

    #[test]
    fn a_full_log_takes_no_more_events() {
        let full = LogHead { seq: u64::MAX, hash: EventHash::ZERO, hlc: Hlc::ZERO };
        assert!(matches!(writer(full).prepare(draft(), at(0)), Err(AppendError::Full)));
    }

    #[test]
    fn links_are_checked() {
        let mut writer = writer(LogHead::EMPTY);
        let events: Vec<SignedEvent> =
            (0..3).map(|second| writer.prepare(draft(), at(second)).unwrap().commit()).collect();
        let [first, second, third] = &events[..] else { panic!("three events") };
        let head = LogHead::of(second);
        assert_eq!(LogHead::EMPTY.link(first), Ok(Link::Next));
        assert_eq!(LogHead::of(first).link(second), Ok(Link::Next));
        assert_eq!(head.link(third), Ok(Link::Next));
        assert_eq!(head.link(second), Ok(Link::Duplicate));
        assert_eq!(head.link(first), Ok(Link::Earlier));
        assert_eq!(LogHead::EMPTY.link(second), Err(ChainError::Gap { head: 0, seq: 2 }));
        assert_eq!(LogHead::of(first).link(third), Err(ChainError::Gap { head: 1, seq: 3 }));

        // Another log from the same device, forked after the first event.
        let mut fork =
            LogWriter::new(config(LogHead::of(first)), golden::signer(), SeededEntropy::new(9));
        let other_second = fork.prepare(draft(), at(1)).unwrap().commit();
        let other_third = fork.prepare(draft(), at(2)).unwrap().commit();
        assert_eq!(head.link(&other_second), Err(ChainError::Fork { seq: 2 }));
        assert_eq!(head.link(&other_third), Err(ChainError::BrokenLink { seq: 3 }));

        // A next event whose HLC isn't later than the head's.
        let mut stale =
            LogWriter::new(config(LogHead::of(first)), golden::signer(), SeededEntropy::new(4));
        let behind = LogHead { hlc: Hlc::from_u64(u64::MAX), ..LogHead::of(first) };
        let event = stale.prepare(draft(), at(1)).unwrap().commit();
        assert_eq!(behind.link(&event), Err(ChainError::ClockRegressed { seq: 2 }));
        let equal = LogHead { hlc: event.body().hlc, ..LogHead::of(first) };
        assert_eq!(equal.link(&event), Err(ChainError::ClockRegressed { seq: 2 }));
    }
}
