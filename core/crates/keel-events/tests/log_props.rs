//! Property tests: devices write their logs by the rules; a replica accepts a log exactly as its
//! device wrote it, and catches any change on the way (a flipped bit, a missing or reordered
//! event), anything a device signs that breaks the rules, forked logs, and events beyond a
//! revocation. The registry of devices behaves the same whatever order changes reach it in. A
//! writer restored to a stored log and clock carries on as a new writer resumed from them would.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU64;
use core::ops::Range;
use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};

use keel_events::envelope::{Device, EventBody, Location};
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::keys::{PublicKey, SignatureAlgorithm, Signer, SoftwareSigner};
use keel_events::log::{ChainError, EventDraft, Link, LogConfig, LogHead, LogWriter};
use keel_events::verify::{DeviceRegistry, RegistryError, Rejection, Revocation};
use keel_types::{Hlc, Id, SeededEntropy, Timestamp};
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::TestCaseError;
use support::{any_draft, clock_readings, uuid_v7_bytes};

fn device() -> Id<Device> {
    Id::parse("0192f0c1-0000-7000-8000-00000000000d").unwrap()
}

fn location() -> Id<Location> {
    Id::parse("0192f0c1-0000-7000-8000-000000000001").unwrap()
}

fn other_location() -> Id<Location> {
    Id::parse("0192f0c1-0000-7000-8000-000000000002").unwrap()
}

/// The device's key, made from a seed.
#[derive(Clone, Copy, Debug)]
struct Key {
    algorithm: SignatureAlgorithm,
    seed: u64,
}

impl Key {
    fn signer(self) -> SoftwareSigner {
        SoftwareSigner::generate(self.algorithm, &mut SeededEntropy::new(self.seed)).unwrap()
    }

    fn public_key(self) -> PublicKey {
        self.signer().public_key().clone()
    }
}

fn any_key() -> impl Strategy<Value = Key> {
    let algorithm = prop_oneof![Just(SignatureAlgorithm::Es256), Just(SignatureAlgorithm::EdDsa)];
    (algorithm, any::<u64>()).prop_map(|(algorithm, seed)| Key { algorithm, seed })
}

fn config(head: LogHead, latest_hlc: Hlc) -> LogConfig {
    LogConfig {
        device: device(),
        location: location(),
        head,
        latest_hlc,
        max_forward_drift: Duration::from_secs(60),
    }
}

/// The events a device records, and the physical clock's reading at each.
#[derive(Clone, Debug)]
struct Script {
    drafts: Vec<EventDraft>,
    readings: Vec<Timestamp>,
}

fn any_script(len: Range<usize>) -> impl Strategy<Value = Script> {
    prop::collection::vec(any_draft(), len).prop_flat_map(|drafts| {
        let len = drafts.len();
        (Just(drafts), clock_readings(len))
            .prop_map(|(drafts, readings)| Script { drafts, readings })
    })
}

/// Runs `script` on a device whose log ends at `head`.
fn write(key: Key, entropy: u64, head: LogHead, script: &Script) -> Vec<SignedEvent> {
    let config = config(head, head.hlc());
    let mut writer = LogWriter::new(config, key.signer(), SeededEntropy::new(entropy));
    let events = script.drafts.iter().zip(&script.readings);
    events.map(|(draft, now)| writer.prepare(draft.clone(), *now).unwrap().commit()).collect()
}

/// Something a device does, at a reading of its physical clock.
#[derive(Clone, Debug)]
enum Step {
    /// Records an event, or abandons it instead of committing it (as when storing it fails).
    Record { draft: EventDraft, abandon: bool },
    /// Receives an event from another device, with an HLC `offset_ms` from physical time.
    Observe { offset_ms: i64, logical: u16 },
    /// Restarts: a new writer continues the stored log.
    Restart,
}

fn any_steps(len: Range<usize>) -> impl Strategy<Value = Vec<(Step, Timestamp)>> {
    let step = prop_oneof![
        6 => (any_draft(), prop::bool::weighted(0.15))
            .prop_map(|(draft, abandon)| Step::Record { draft, abandon }),
        2 => (-60_000_i64..60_000, any::<u16>())
            .prop_map(|(offset_ms, logical)| Step::Observe { offset_ms, logical }),
        1 => Just(Step::Restart),
    ];
    prop::collection::vec(step, len).prop_flat_map(|steps| {
        let len = steps.len();
        (Just(steps), clock_readings(len))
            .prop_map(|(steps, readings)| steps.into_iter().zip(readings).collect())
    })
}

/// Runs `steps` on a device with a new log, checking each event against the rules directly
/// (not through the library's own view of the log).
fn run(
    key: Key,
    entropy: u64,
    steps: &[(Step, Timestamp)],
) -> Result<Vec<SignedEvent>, TestCaseError> {
    let mut writer = LogWriter::new(
        config(LogHead::EMPTY, Hlc::ZERO),
        key.signer(),
        SeededEntropy::new(entropy),
    );
    let mut events: Vec<SignedEvent> = Vec::new();
    // The latest HLC received from other devices.
    let mut seen = Hlc::ZERO;
    for (index, (step, now)) in steps.iter().enumerate() {
        match step {
            Step::Record { draft, abandon } => {
                let pending = writer.prepare(draft.clone(), *now).unwrap();
                let event = pending.event();
                let body = event.body();
                let previous = events.last();
                prop_assert_eq!(body.origin_seq.get(), u64::try_from(events.len()).unwrap() + 1);
                prop_assert_eq!(
                    body.prev_hash,
                    previous.map_or(EventHash::ZERO, SignedEvent::hash)
                );
                if let Some(previous) = previous {
                    prop_assert!(body.hlc > previous.body().hlc, "the HLC went backwards");
                }
                prop_assert!(body.hlc > seen, "the HLC isn't after an event already seen");
                prop_assert!(i64::try_from(body.hlc.wall_ms()).unwrap() >= now.as_millis());
                prop_assert_eq!(body.origin_device, device());
                prop_assert_eq!(body.location, location());
                prop_assert_eq!(&draft_of(event), draft);
                prop_assert_eq!(event.key_id(), key.public_key().key_id());
                if !abandon {
                    events.push(pending.commit());
                }
            }
            Step::Observe { offset_ms, logical } => {
                let remote = remote_hlc(*now, *offset_ms, *logical);
                writer.observe(remote, *now).unwrap();
                seen = seen.max(remote);
            }
            Step::Restart => {
                let head = events.last().map_or(LogHead::EMPTY, LogHead::of);
                let entropy =
                    SeededEntropy::new(entropy.rotate_left(u32::try_from(index).unwrap()));
                writer = LogWriter::new(config(head, seen), key.signer(), entropy);
            }
        }
    }
    Ok(events)
}

/// What a replica did with an event it received.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    Appended,
    Duplicate,
    /// The registry rejected it.
    Rejected(Rejection),
    /// It doesn't fit the device's log.
    Unlinked(ChainError),
    /// It is before the head, and differs from the stored event at its position.
    Conflict {
        seq: u64,
    },
}

/// A replica, receiving events as a store would: it verifies each one, links it to its device's
/// log, and appends it, ignores it as a duplicate, or reports it.
struct Replica {
    registry: DeviceRegistry,
    logs: BTreeMap<Id<Device>, Vec<SignedEvent>>,
    outcomes: Vec<Outcome>,
}

impl Replica {
    fn new(key: Key, revocation: Option<Revocation>) -> Replica {
        let mut registry = DeviceRegistry::new();
        registry.enroll(device(), location(), key.public_key()).unwrap();
        if let Some(revocation) = revocation {
            registry.revoke(device(), revocation).unwrap();
        }
        Replica { registry, logs: BTreeMap::new(), outcomes: Vec::new() }
    }

    fn receive(&mut self, bytes: &[u8]) {
        let outcome = match self.registry.verify(bytes) {
            Err(rejection) => Outcome::Rejected(rejection),
            Ok(event) => {
                let log = self.logs.entry(event.body().origin_device).or_default();
                let head = log.last().map_or(LogHead::EMPTY, LogHead::of);
                match head.link(&event) {
                    Ok(Link::Next) => {
                        log.push(event);
                        Outcome::Appended
                    }
                    Ok(Link::Duplicate) => Outcome::Duplicate,
                    Ok(Link::Earlier) => {
                        let seq = event.body().origin_seq.get();
                        let stored = &log[usize::try_from(seq - 1).unwrap()];
                        if stored.hash() == event.hash() {
                            Outcome::Duplicate
                        } else {
                            Outcome::Conflict { seq }
                        }
                    }
                    Err(error) => Outcome::Unlinked(error),
                }
            }
        };
        self.outcomes.push(outcome);
    }

    fn receive_all<'a>(&mut self, events: impl IntoIterator<Item = &'a SignedEvent>) {
        for event in events {
            self.receive(&event.to_bytes());
        }
    }

    fn rejections(&self) -> usize {
        let rejected =
            |outcome: &&Outcome| !matches!(outcome, Outcome::Appended | Outcome::Duplicate);
        self.outcomes.iter().filter(rejected).count()
    }

    fn log(&self) -> &[SignedEvent] {
        self.logs.get(&device()).map_or(&[], Vec::as_slice)
    }
}

/// The draft an event was made from.
fn draft_of(event: &SignedEvent) -> EventDraft {
    let body = event.body().clone();
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

/// A change to a log on its way to a replica.
#[derive(Clone, Debug)]
enum Tampering {
    FlipBit { event: Index, byte: Index, bit: u8 },
    Delete { event: Index },
    Swap { first: Index, second: Index },
    Replay { event: Index, at: Index },
    Truncate { keep: Index },
}

fn any_tampering() -> impl Strategy<Value = Tampering> {
    prop_oneof![
        (any::<Index>(), any::<Index>(), 0_u8..8)
            .prop_map(|(event, byte, bit)| Tampering::FlipBit { event, byte, bit }),
        any::<Index>().prop_map(|event| Tampering::Delete { event }),
        (any::<Index>(), any::<Index>())
            .prop_map(|(first, second)| Tampering::Swap { first, second }),
        (any::<Index>(), any::<Index>()).prop_map(|(event, at)| Tampering::Replay { event, at }),
        any::<Index>().prop_map(|keep| Tampering::Truncate { keep }),
    ]
}

/// The log as delivered after `tampering`, with how much of it a replica should accept and how
/// many events it should report (`None`: at least one).
fn tamper(events: &[SignedEvent], tampering: &Tampering) -> (Vec<Vec<u8>>, usize, Option<usize>) {
    let mut delivered: Vec<Vec<u8>> = events.iter().map(SignedEvent::to_bytes).collect();
    let len = events.len();
    match tampering {
        Tampering::FlipBit { event, byte, bit } => {
            let event = event.index(len);
            let bytes = &mut delivered[event];
            let byte = byte.index(bytes.len());
            bytes[byte] ^= 1 << bit;
            (delivered, event, None)
        }
        Tampering::Delete { event } => {
            let event = event.index(len);
            delivered.remove(event);
            // Losing the last event looks like it hasn't been sent yet.
            let rejections = if event == len - 1 { Some(0) } else { None };
            (delivered, event, rejections)
        }
        Tampering::Swap { first, second } => {
            let (first, second) = (first.index(len), second.index(len));
            delivered.swap(first, second);
            if first == second {
                (delivered, len, Some(0))
            } else {
                (delivered, first.min(second) + 1, None)
            }
        }
        Tampering::Replay { event, at } => {
            let (event, at) = (event.index(len), at.index(len + 1));
            let copy = delivered[event].clone();
            delivered.insert(at, copy);
            // A copy that arrives before its turn is out of order; the original is then accepted.
            (delivered, len, Some(usize::from(at < event)))
        }
        Tampering::Truncate { keep } => {
            let keep = keep.index(len + 1);
            delivered.truncate(keep);
            (delivered, keep, Some(0))
        }
    }
}

/// Interleaves two sequences, taking from `a` where `choices` says so, keeping each in order.
fn interleave<T: Clone>(a: &[T], b: &[T], choices: &[bool]) -> Vec<T> {
    let (mut a, mut b) = (a.iter(), b.iter());
    let mut merged: Vec<T> = Vec::new();
    for &from_a in choices {
        let next =
            if from_a { a.next().or_else(|| b.next()) } else { b.next().or_else(|| a.next()) };
        merged.extend(next.cloned());
    }
    merged.extend(a.cloned());
    merged.extend(b.cloned());
    merged
}

/// How a device, with its own key, sets the fields a replica checks on an event following a log.
#[derive(Clone, Debug)]
struct Misstep {
    draft: EventDraft,
    event_id: [u8; 16],
    /// A sequence number around the next one, or any.
    seq: prop::sample::Selector,
    any_seq: Option<NonZeroU64>,
    /// The head's hash, the one before it, zero, or any.
    prev_hash: u8,
    random_hash: [u8; 32],
    /// Just before the head's HLC, the same, just after, or any.
    hlc: u8,
    random_hlc: u64,
    other_location: bool,
}

fn any_misstep() -> impl Strategy<Value = Misstep> {
    let seq = (
        any::<prop::sample::Selector>(),
        prop::option::weighted(0.1, (1..=u64::MAX).prop_map(|seq| NonZeroU64::new(seq).unwrap())),
    );
    let fields = (0_u8..4, any::<[u8; 32]>(), 0_u8..4, any::<u64>(), prop::bool::weighted(0.1));
    (any_draft(), uuid_v7_bytes(), seq, fields).prop_map(
        |(
            draft,
            event_id,
            (seq, any_seq),
            (prev_hash, random_hash, hlc, random_hlc, other_location),
        )| Misstep {
            draft,
            event_id,
            seq,
            any_seq,
            prev_hash,
            random_hash,
            hlc,
            random_hlc,
            other_location,
        },
    )
}

/// A two-way choice of hash for revocations.
fn revocation_hash(seq: u64, choice: u8) -> EventHash {
    match choice {
        0 => EventHash::ZERO,
        _ => EventHash::of(&[&seq.to_be_bytes()[..], &[choice]].concat()),
    }
}

/// Where a writer is restored to: a log's head and a clock, as a store keeps them.
#[derive(Clone, Debug)]
enum Target {
    /// The head and clock the writer had after one of its steps: a transaction that stored what
    /// came after failed.
    Back { at: Index },
    /// Past `count` of the device's own later events, written elsewhere `ahead_ms` after the last
    /// reading, keeping the writer's clock: they arrived from another replica.
    Forward { count: usize, ahead_ms: i64 },
    /// A head the writer had, with a clock `offset_ms` from the head's, before or after it.
    Anywhere { at: Index, offset_ms: i64 },
}

fn any_target() -> impl Strategy<Value = Target> {
    prop_oneof![
        any::<Index>().prop_map(|at| Target::Back { at }),
        (1_usize..4, 0_i64..7_200_000)
            .prop_map(|(count, ahead_ms)| Target::Forward { count, ahead_ms }),
        (any::<Index>(), -120_000_i64..120_000)
            .prop_map(|(at, offset_ms)| Target::Anywhere { at, offset_ms }),
    ]
}

/// What a restored writer does next: records an event, or observes a remote HLC `offset_ms` from
/// physical time, which may be beyond the drift limit.
#[derive(Clone, Debug)]
enum Then {
    Record(EventDraft),
    Observe { offset_ms: i64, logical: u16 },
}

fn any_then(len: Range<usize>) -> impl Strategy<Value = Vec<(Then, Timestamp)>> {
    let then = prop_oneof![
        3 => any_draft().prop_map(Then::Record),
        1 => (-120_000_i64..120_000, any::<u16>())
            .prop_map(|(offset_ms, logical)| Then::Observe { offset_ms, logical }),
    ];
    prop::collection::vec(then, len).prop_flat_map(|thens| {
        let len = thens.len();
        (Just(thens), clock_readings(len))
            .prop_map(|(thens, readings)| thens.into_iter().zip(readings).collect())
    })
}

/// The remote HLC `offset_ms` from physical time `now`.
fn remote_hlc(now: Timestamp, offset_ms: i64, logical: u16) -> Hlc {
    let wall = u64::try_from((now.as_millis() + offset_ms).max(0)).unwrap();
    Hlc::new(wall, logical).unwrap()
}

proptest! {
    /// A device's log follows the rules: sequence numbers from 1 without gaps, each event linked
    /// to the one before, HLCs increasing, later than every event the device has seen, and never
    /// behind physical time, and unique identifiers. This holds across abandoned events, restarts
    /// and whatever the physical clock does. Writing is deterministic, and a replica accepts the
    /// log exactly as written.
    #[test]
    fn logs_are_written_by_the_rules_and_accepted_as_written(
        steps in any_steps(1..16),
        key in any_key(),
        entropy in any::<u64>(),
    ) {
        let events = run(key, entropy, &steps)?;
        let ids: BTreeSet<_> = events.iter().map(|event| event.body().event_id).collect();
        prop_assert_eq!(ids.len(), events.len());
        prop_assert_eq!(&run(key, entropy, &steps)?, &events);

        let mut replica = Replica::new(key, None);
        replica.receive_all(&events);
        prop_assert_eq!(replica.rejections(), 0);
        prop_assert_eq!(replica.log(), &events[..]);
    }

    /// Whatever happens to a log on its way to a replica, the replica holds a prefix of the log
    /// as written, and reports every change it can see: a changed bit, a missing event (except
    /// the last, which looks unsent) and a reordering. Replays and truncations lose nothing that
    /// was delivered.
    #[test]
    fn changes_on_the_way_are_caught(
        script in any_script(1..10),
        key in any_key(),
        entropy in any::<u64>(),
        tampering in any_tampering(),
    ) {
        let events = write(key, entropy, LogHead::EMPTY, &script);
        let (delivered, accepted, rejections) = tamper(&events, &tampering);
        let mut replica = Replica::new(key, None);
        for bytes in &delivered {
            replica.receive(bytes);
        }
        prop_assert_eq!(replica.log(), &events[..accepted]);
        match rejections {
            Some(count) => prop_assert_eq!(replica.rejections(), count),
            None => prop_assert!(replica.rejections() > 0),
        }
    }

    /// An event a device signs after its log, with any sequence number, previous hash, HLC and
    /// location, is appended exactly when the rules allow, and otherwise reported for the rule
    /// it breaks, as a model of the rules decides.
    #[test]
    fn replicas_hold_devices_to_the_rules(
        script in any_script(1..6),
        key in any_key(),
        entropy in any::<u64>(),
        misstep in any_misstep(),
    ) {
        let events = write(key, entropy, LogHead::EMPTY, &script);
        let last = events.last().unwrap();
        let (head_seq, head_hash, head_hlc) =
            (u64::try_from(events.len()).unwrap(), last.hash(), last.body().hlc);
        let seq = misstep.any_seq.map_or_else(
            || misstep.seq.select(1..=head_seq + 2),
            NonZeroU64::get,
        );
        let prev_hash = match misstep.prev_hash {
            0 => head_hash,
            1 => events.len().checked_sub(2).map_or(EventHash::ZERO, |index| events[index].hash()),
            2 => EventHash::ZERO,
            _ => EventHash::from_bytes(misstep.random_hash),
        };
        let hlc = match misstep.hlc {
            0 => Hlc::from_u64(head_hlc.to_u64() - 1),
            1 => head_hlc,
            2 => Hlc::from_u64(head_hlc.to_u64() + 1),
            _ => Hlc::from_u64(misstep.random_hlc),
        };
        let draft = misstep.draft;
        let body = EventBody {
            event_id: Id::from_bytes(misstep.event_id).unwrap(),
            location: if misstep.other_location { other_location() } else { location() },
            stream: draft.stream,
            schema: draft.schema,
            origin_device: device(),
            origin_seq: NonZeroU64::new(seq).unwrap(),
            hlc,
            business_date: draft.business_date,
            actor: draft.actor,
            approval: draft.approval,
            causation: draft.causation,
            correlation: draft.correlation,
            payload: draft.payload,
            prev_hash,
        };
        let candidate = SignedEvent::sign(body, &key.signer()).unwrap();

        let expected = if misstep.other_location {
            Outcome::Rejected(Rejection::WrongLocation)
        } else if seq < head_seq {
            Outcome::Conflict { seq }
        } else if seq == head_seq {
            Outcome::Unlinked(ChainError::Fork { seq })
        } else if seq > head_seq + 1 {
            Outcome::Unlinked(ChainError::Gap { head: head_seq, seq })
        } else if prev_hash != head_hash {
            Outcome::Unlinked(ChainError::BrokenLink { seq })
        } else if hlc <= head_hlc {
            Outcome::Unlinked(ChainError::ClockRegressed { seq })
        } else {
            Outcome::Appended
        };
        let mut replica = Replica::new(key, None);
        replica.receive_all(events.iter().chain([&candidate]));
        prop_assert_eq!(replica.outcomes.last(), Some(&expected));
        let mut log = events.clone();
        if expected == Outcome::Appended {
            log.push(candidate);
        }
        prop_assert_eq!(replica.log(), &log[..]);
    }

    /// A device that signs two different continuations of its log (because it is compromised,
    /// or was restored from a backup) is caught: a replica keeps the continuation it received
    /// first and reports every event of the other.
    #[test]
    fn forks_are_caught(
        (prefix, first, second) in (any_script(0..4), any_script(1..5), any_script(1..5)),
        key in any_key(),
        entropy in any::<u64>(),
        choices in prop::collection::vec(any::<bool>(), 0..10),
    ) {
        let common = write(key, entropy, LogHead::EMPTY, &prefix);
        let head = common.last().map_or(LogHead::EMPTY, LogHead::of);
        let first = write(key, entropy ^ 1, head, &first);
        let second = write(key, entropy ^ 2, head, &second);
        let delivered = interleave(&first, &second, &choices);
        let (winner, loser) =
            if delivered[0] == first[0] { (&first, &second) } else { (&second, &first) };

        let mut replica = Replica::new(key, None);
        replica.receive_all(common.iter().chain(&delivered));
        let expected: Vec<SignedEvent> = common.iter().chain(winner.iter()).cloned().collect();
        prop_assert_eq!(replica.log(), &expected[..]);
        prop_assert_eq!(replica.rejections(), loser.len());
    }

    /// Once a device is revoked after event r, no replica accepts anything beyond r, and the
    /// event at r is the one the revocation trusts, whatever the device signs and whatever
    /// order events arrive in. A history forged before r with the device's key may be accepted
    /// until the genuine one arrives, but then it is reported.
    #[test]
    fn revocation_cuts_every_history_at_the_same_place(
        (script, forgery) in (any_script(1..8), any_script(0..5)),
        key in any_key(),
        entropy in any::<u64>(),
        (cut, fork) in (any::<Index>(), any::<Index>()),
        forgery_first in any::<bool>(),
    ) {
        let events = write(key, entropy, LogHead::EMPTY, &script);
        let cut = cut.index(events.len() + 1);
        let last_trusted = if cut == 0 { EventHash::ZERO } else { events[cut - 1].hash() };
        let revocation = Revocation { after_seq: u64::try_from(cut).unwrap(), last_trusted };
        let fork = fork.index(events.len() + 1);
        let head = if fork == 0 { LogHead::EMPTY } else { LogHead::of(&events[fork - 1]) };
        let forged = write(key, !entropy, head, &forgery);

        let mut replica = Replica::new(key, Some(revocation));
        if forgery_first {
            replica.receive_all(forged.iter().chain(&events));
        } else {
            replica.receive_all(events.iter().chain(&forged));
        }
        let log = replica.log();
        prop_assert!(log.len() <= cut);
        if cut > 0 && log.len() == cut {
            prop_assert_eq!(log[cut - 1].hash(), last_trusted);
        }
        if log != &events[..log.len()] {
            prop_assert!(replica.rejections() > 0);
        }
        if !forgery_first {
            prop_assert_eq!(log, &events[..cut]);
        }
    }

    /// Revocations of a device, in any order, leave its earliest cut; invalid ones are refused;
    /// and two revocations disagreeing about the event at the earliest cut are reported.
    #[test]
    fn revocations_merge_in_any_order(
        revocations in prop::collection::vec((0_u64..5, 0_u8..3, any::<u64>()), 0..8),
    ) {
        let revocation = |&(seq, hash, _): &(u64, u8, u64)| Revocation {
            after_seq: seq,
            last_trusted: revocation_hash(seq, hash),
        };
        let valid: Vec<Revocation> = revocations
            .iter()
            .map(revocation)
            .filter(|revocation| (revocation.after_seq == 0) == (revocation.last_trusted == EventHash::ZERO))
            .collect();
        let earliest = valid.iter().map(|revocation| revocation.after_seq).min();
        let at_earliest: BTreeSet<EventHash> = valid
            .iter()
            .filter(|revocation| Some(revocation.after_seq) == earliest)
            .map(|revocation| revocation.last_trusted)
            .collect();

        let mut shuffled = revocations.clone();
        shuffled.sort_by_key(|&(_, _, order)| order);
        for order in [&revocations, &shuffled] {
            let mut registry = DeviceRegistry::new();
            registry.enroll(device(), location(), Key { algorithm: SignatureAlgorithm::EdDsa, seed: 1 }.public_key()).unwrap();
            let mut conflicts = 0;
            for entry in order {
                let revocation = revocation(entry);
                match registry.revoke(device(), revocation) {
                    Ok(()) => prop_assert!(valid.contains(&revocation)),
                    Err(RegistryError::InvalidRevocation) => prop_assert!(!valid.contains(&revocation)),
                    Err(RegistryError::ConflictingRevocation(_)) => conflicts += 1,
                    Err(error) => return Err(TestCaseError::fail(format!("unexpected {error}"))),
                }
            }
            let kept = registry.get(device()).unwrap().revocation();
            prop_assert_eq!(kept.map(|revocation| revocation.after_seq), earliest);
            if at_earliest.len() == 1 {
                prop_assert_eq!(kept.map(|revocation| revocation.last_trusted), at_earliest.first().copied());
            } else if at_earliest.len() > 1 {
                prop_assert!(conflicts > 0);
            }
        }
    }

    /// The first enrollment of a device holds: enrolling it again the same way changes nothing,
    /// and any other way is refused.
    #[test]
    fn enrollments_are_first_come(attempts in prop::collection::vec((0_u8..2, 0_u8..2, 1_u64..3), 1..8)) {
        let devices = [device(), Id::parse("0192f0c1-0000-7000-8000-00000000000e").unwrap()];
        let locations = [location(), other_location()];
        let key = |seed: u64| Key { algorithm: SignatureAlgorithm::EdDsa, seed }.public_key();
        let mut registry = DeviceRegistry::new();
        let mut first: BTreeMap<u8, (u8, u64)> = BTreeMap::new();
        for &(device, location, seed) in &attempts {
            let result = registry.enroll(devices[usize::from(device)], locations[usize::from(location)], key(seed));
            let enrolled = *first.entry(device).or_insert((location, seed));
            if enrolled == (location, seed) {
                prop_assert_eq!(result, Ok(()));
            } else {
                prop_assert_eq!(result, Err(RegistryError::AlreadyEnrolled(devices[usize::from(device)])));
            }
        }
        for (device, (location, seed)) in first {
            let record = registry.get(devices[usize::from(device)]).unwrap();
            prop_assert_eq!(record.location(), locations[usize::from(location)]);
            prop_assert_eq!(record.key(), &key(seed));
        }
    }

    /// A writer restored to a head and a clock, as a store keeps them, carries on exactly as a
    /// new writer resumed from them would, whatever it did before: back to an earlier state
    /// after a failed transaction, on past the device's own events from elsewhere, or to any head
    /// with any clock. Its latest HLC is always the last it issued or observed.
    #[test]
    fn a_restored_writer_carries_on_as_a_resumed_one(
        key in any_key(),
        steps in any_steps(1..10),
        target in any_target(),
        elsewhere_draft in any_draft(),
        then in any_then(1..6),
    ) {
        let mut writer =
            LogWriter::new(config(LogHead::EMPTY, Hlc::ZERO), key.signer(), SeededEntropy::new(1));
        // The head and latest HLC before and after each step.
        let mut states = vec![(LogHead::EMPTY, Hlc::ZERO)];
        for (step, now) in &steps {
            let last = match step {
                Step::Record { draft, abandon } => {
                    let pending = writer.prepare(draft.clone(), *now).unwrap();
                    let hlc = pending.event().body().hlc;
                    if !abandon {
                        pending.commit();
                    }
                    hlc
                }
                Step::Observe { offset_ms, logical } => {
                    writer.observe(remote_hlc(*now, *offset_ms, *logical), *now).unwrap()
                }
                // Restoring the writer to where it is changes nothing.
                Step::Restart => {
                    let (head, latest) = (writer.head(), writer.latest_hlc());
                    writer.restore(head, latest);
                    prop_assert_eq!(writer.head(), head);
                    latest
                }
            };
            prop_assert_eq!(writer.latest_hlc(), last, "the latest HLC is the last issued or seen");
            states.push((writer.head(), writer.latest_hlc()));
        }
        let (head, latest) = match target {
            Target::Back { at } => states[at.index(states.len())],
            Target::Forward { count, ahead_ms } => {
                let config = config(writer.head(), Hlc::ZERO);
                let mut elsewhere = LogWriter::new(config, key.signer(), SeededEntropy::new(2));
                let start = steps.last().unwrap().1.as_millis() + ahead_ms;
                let mut head = writer.head();
                for k in 0..count {
                    let now = Timestamp::from_millis(start + i64::try_from(k).unwrap()).unwrap();
                    let event = elsewhere.prepare(elsewhere_draft.clone(), now).unwrap().commit();
                    // The head as a store keeps it.
                    let body = event.body();
                    head = LogHead::from_parts(body.origin_seq.get(), event.hash(), body.hlc);
                    prop_assert_eq!(head, LogHead::of(&event));
                }
                (head, writer.latest_hlc())
            }
            Target::Anywhere { at, offset_ms } => {
                let (head, _) = states[at.index(states.len())];
                let wall = i64::try_from(head.hlc().wall_ms()).unwrap() + offset_ms;
                (head, Hlc::new(u64::try_from(wall.max(0)).unwrap(), 0).unwrap())
            }
        };
        writer.restore(head, latest);
        let mut resumed = LogWriter::new(config(head, latest), key.signer(), SeededEntropy::new(3));
        prop_assert_eq!(writer.head(), resumed.head());
        prop_assert_eq!(writer.latest_hlc(), resumed.latest_hlc());
        for (then, now) in &then {
            match then {
                Then::Record(draft) => {
                    let (restored_head, fresh_head) = (writer.head(), resumed.head());
                    let restored = writer.prepare(draft.clone(), *now).unwrap().commit();
                    let fresh = resumed.prepare(draft.clone(), *now).unwrap().commit();
                    // Each links to its own writer's log, and it is the same event but for its
                    // identifier, which each writer draws from its own entropy, and so for the
                    // hashes it links to after the first.
                    prop_assert_eq!(restored.body().prev_hash, restored_head.hash());
                    prop_assert_eq!(fresh.body().prev_hash, fresh_head.hash());
                    let mut body = restored.body().clone();
                    body.event_id = fresh.body().event_id;
                    body.prev_hash = fresh.body().prev_hash;
                    prop_assert_eq!(&body, fresh.body());
                }
                Then::Observe { offset_ms, logical } => {
                    let remote = remote_hlc(*now, *offset_ms, *logical);
                    prop_assert_eq!(writer.observe(remote, *now), resumed.observe(remote, *now));
                }
            }
            prop_assert_eq!(writer.latest_hlc(), resumed.latest_hlc());
        }
    }
}
