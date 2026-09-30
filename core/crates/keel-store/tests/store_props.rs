//! Property tests for the store, against a model of its rules.
//!
//! A case is a sequence of writes and reopenings. A write appends the device's own events and
//! receives events of every kind: the next of a device's log, one received before, a fork, one
//! after a gap, one that doesn't link or whose clock went back, one signed with the wrong key,
//! from an unknown or revoked device, for another location, with a used identifier, at the last
//! position a store holds or beyond, garbage, a message quarantined before, and the device's own
//! events from elsewhere. Steps may happen at the same physical time, so that events of different
//! devices tie on their HLCs. A write ends by committing, failing, or being interrupted at a
//! point of the write. A reopening may first open the store as another device, at another
//! location or with another key, which must be refused.
//!
//! The model keeps each device's log as a list, and decides what becomes of each received event
//! from how the test made it, never by calling the verifier. After every write and reopening,
//! everything the store reads back is checked against it: logs, heads, the version vector,
//! streams in canonical order, events by identifier, the quarantine, and the device's clock.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use core::num::NonZeroU64;

use keel_events::envelope::{Device, Location};
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::log::LogHead;
use keel_store::{Faults, Point, Quarantined, Reason, Received, Store, StoreConfig, StoreError};
use keel_types::{Hlc, Id, SeededEntropy, Timestamp};
use proptest::prelude::*;
use proptest::sample::Index;
use support::{
    DRIFT, FOREIGN, OWN, PEERS, REVOKED, Scratch, UNKNOWN, at, config, device, draft, elsewhere,
    here, next_event, registry, resigned, signer, store_key, store_key_of, stray_hash,
    trusted_first,
};

// ---------------------------------------------------------------------------------------------
// What a case does.

/// A received message, described by how to make it from what the store holds.
#[derive(Clone, Debug)]
enum Pick {
    /// The next event of a device's log; `late` puts its clock beyond the drift limit.
    Next { device: u8, variant: u8, late: bool },
    /// An event the store holds, received again.
    Replay { device: u8, at: Index },
    /// A different event at a position the store holds.
    Fork { device: u8, at: Index, variant: u8 },
    /// The event after the next, so one is missing.
    Ahead { device: u8, variant: u8 },
    /// The next position, linking to an event the store doesn't hold.
    Broken { device: u8, variant: u8 },
    /// The next event, with a clock no later than the event's before it.
    Regressed { device: u8, variant: u8 },
    /// The next event, signed with another device's key.
    BadKey { device: u8, variant: u8 },
    /// An event from a device nobody enrolled.
    Unknown { variant: u8 },
    /// The next event, naming another location than its device is enrolled at.
    Misplaced { device: u8, variant: u8 },
    /// An event from a device enrolled at another location; `ahead`, the second of its log.
    Foreign { variant: u8, ahead: bool },
    /// The next event, with the identifier of an event the store holds.
    DuplicateId { device: u8, at: Index, variant: u8 },
    /// The next event, renumbered to position `seq`: the last a store holds, 2^63 − 1, or beyond.
    Far { device: u8, variant: u8, seq: u64 },
    /// Bytes that aren't an event.
    Garbage { bytes: Vec<u8> },
    /// A message the store quarantined, received again. `changed` aims at one whose fate has
    /// changed since, when there is one: a broken link, a clock that went back or a used
    /// identifier at the next position becomes a fork once the position is filled. The
    /// quarantine keeps the first reason.
    Again { at: Index, changed: bool },
    /// The device's own next event, from another replica; `late` puts it an hour ahead.
    Own { variant: u8, late: bool },
    /// The revoked device's next event: only its trusted first event is taken.
    Revoked { variant: u8 },
}

#[derive(Clone, Debug)]
enum Step {
    Append { stream: u8, note: u8 },
    Receive(Pick),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    Commit,
    /// The caller fails after its steps.
    Fail,
    /// A fault hook refuses at the `nth` time the write reaches the point.
    Interrupt(Point, u8),
}

/// Someone other than the device, opening its store.
#[derive(Clone, Copy, Debug)]
enum Stranger {
    /// Another device, with its own key.
    Device,
    /// The device, enrolled at another location.
    Location,
    /// The device, with another device's signing key.
    Signer,
    /// The device, with another store's key.
    StoreKey,
}

#[derive(Clone, Debug)]
enum Op {
    /// A write's steps, each with whether it happens at the same physical time as the step
    /// before, and how the write ends.
    Write(Vec<(Step, bool)>, Ending),
    Reopen,
    /// A stranger opens the store, then the device reopens it.
    OpenAs(Stranger),
}

fn peer() -> impl Strategy<Value = u8> {
    prop::sample::select(PEERS.to_vec())
}

fn any_pick() -> impl Strategy<Value = Pick> {
    let variant = 0_u8..4;
    prop_oneof![
        8 => (peer(), variant.clone(), prop::bool::weighted(0.2))
            .prop_map(|(device, variant, late)| Pick::Next { device, variant, late }),
        3 => (prop::sample::select(vec![OWN, PEERS[0], PEERS[1], REVOKED]), any::<Index>())
            .prop_map(|(device, at)| Pick::Replay { device, at }),
        2 => (peer(), any::<Index>(), variant.clone())
            .prop_map(|(device, at, variant)| Pick::Fork { device, at, variant }),
        2 => (peer(), variant.clone()).prop_map(|(device, variant)| Pick::Ahead { device, variant }),
        2 => (peer(), variant.clone()).prop_map(|(device, variant)| Pick::Broken { device, variant }),
        2 => (peer(), variant.clone())
            .prop_map(|(device, variant)| Pick::Regressed { device, variant }),
        1 => (peer(), variant.clone()).prop_map(|(device, variant)| Pick::BadKey { device, variant }),
        1 => variant.clone().prop_map(|variant| Pick::Unknown { variant }),
        1 => (peer(), variant.clone())
            .prop_map(|(device, variant)| Pick::Misplaced { device, variant }),
        1 => (variant.clone(), any::<bool>())
            .prop_map(|(variant, ahead)| Pick::Foreign { variant, ahead }),
        2 => (peer(), any::<Index>(), variant.clone())
            .prop_map(|(device, at, variant)| Pick::DuplicateId { device, at, variant }),
        1 => (peer(), variant.clone(), prop::sample::select(vec![(1_u64 << 63) - 1, 1 << 63, u64::MAX]))
            .prop_map(|(device, variant, seq)| Pick::Far { device, variant, seq }),
        1 => prop::collection::vec(any::<u8>(), 0..40).prop_map(|bytes| Pick::Garbage { bytes }),
        3 => (any::<Index>(), prop::bool::weighted(0.7))
            .prop_map(|(at, changed)| Pick::Again { at, changed }),
        2 => (variant.clone(), prop::bool::weighted(0.3))
            .prop_map(|(variant, late)| Pick::Own { variant, late }),
        2 => variant.prop_map(|variant| Pick::Revoked { variant }),
    ]
}

fn any_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => (1_u8..=3, any::<u8>()).prop_map(|(stream, note)| Step::Append { stream, note }),
        5 => any_pick().prop_map(Step::Receive),
    ]
}

fn any_ending() -> impl Strategy<Value = Ending> {
    prop_oneof![
        12 => Just(Ending::Commit),
        2 => Just(Ending::Fail),
        1 => Just(Ending::Interrupt(Point::Began, 1)),
        3 => (1_u8..=4).prop_map(|nth| Ending::Interrupt(Point::Stored, nth)),
        1 => Just(Ending::Interrupt(Point::Committing, 1)),
        1 => Just(Ending::Interrupt(Point::Committed, 1)),
    ]
}

fn any_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => (prop::collection::vec((any_step(), prop::bool::weighted(0.3)), 1..6), any_ending())
            .prop_map(|(steps, ending)| Op::Write(steps, ending)),
        1 => Just(Op::Reopen),
        1 => prop_oneof![
            Just(Stranger::Device),
            Just(Stranger::Location),
            Just(Stranger::Signer),
            Just(Stranger::StoreKey),
        ]
            .prop_map(Op::OpenAs),
    ]
}

// ---------------------------------------------------------------------------------------------
// The model.

/// What the model holds: each device's log, the quarantine, and what the device's clock must
/// follow.
#[derive(Clone, Debug)]
struct Model {
    logs: BTreeMap<Id<Device>, Vec<SignedEvent>>,
    /// Each distinct message refused, with the reason it was first refused for.
    quarantine: Vec<(Reason, Made)>,
    /// The latest HLC the device's next event must follow: its own events', and those of
    /// received events that were within the drift limit when they arrived.
    floor: Hlc,
    /// The device's clock when the last write committed.
    committed_clock: Hlc,
}

impl Model {
    fn new() -> Model {
        Model {
            logs: BTreeMap::new(),
            quarantine: Vec::new(),
            floor: Hlc::ZERO,
            committed_clock: Hlc::ZERO,
        }
    }

    fn log(&self, n: u8) -> &[SignedEvent] {
        self.logs.get(&device(n)).map_or(&[], Vec::as_slice)
    }

    fn head(&self, n: u8) -> LogHead {
        self.log(n).last().map_or(LogHead::EMPTY, LogHead::of)
    }

    fn holds_id(&self, id: Id<keel_events::envelope::Event>) -> bool {
        self.logs.values().flatten().any(|event| event.body().event_id == id)
    }

    fn all_ids(&self) -> Vec<Id<keel_events::envelope::Event>> {
        self.logs.values().flatten().map(|event| event.body().event_id).collect()
    }
}

/// How the test made a received message: what the model decides its fate from.
#[derive(Clone, Debug)]
struct Made {
    bytes: Vec<u8>,
    /// The event, unless the bytes aren't one.
    event: Option<SignedEvent>,
    /// The device whose key signed it.
    signer: u8,
}

/// The device an event claims, by number.
fn number_of(id: Id<Device>) -> u8 {
    [OWN, PEERS[0], PEERS[1], REVOKED, UNKNOWN, FOREIGN]
        .into_iter()
        .find(|&n| device(n) == id)
        .unwrap()
}

fn enrolled_at(n: u8) -> Option<Id<Location>> {
    match n {
        UNKNOWN => None,
        FOREIGN => Some(elsewhere()),
        _ => Some(here()),
    }
}

/// What the model says becomes of a received message.
fn expected(model: &Model, made: &Made) -> Received {
    let Some(event) = &made.event else { return Received::Quarantined(Reason::Malformed) };
    let body = event.body();
    let n = number_of(body.origin_device);
    let Some(enrolled) = enrolled_at(n) else {
        return Received::Quarantined(Reason::UnknownDevice);
    };
    if made.signer != n {
        return Received::Quarantined(Reason::BadSignature);
    }
    if body.location != enrolled {
        return Received::Quarantined(Reason::WrongLocation);
    }
    let seq = body.origin_seq.get();
    if n == REVOKED && !(seq == 1 && event.hash() == trusted_first().hash()) {
        return Received::Quarantined(Reason::Revoked);
    }
    if body.location != here() {
        return Received::Quarantined(Reason::OtherLocation);
    }
    // A store holds positions up to 2^63 − 1.
    if seq >= 1 << 63 {
        return Received::Quarantined(Reason::OutOfRange);
    }
    let log = model.log(n);
    let held = u64::try_from(log.len()).unwrap();
    if seq <= held {
        let stored = &log[usize::try_from(seq - 1).unwrap()];
        return if stored.hash() == event.hash() {
            Received::Duplicate
        } else {
            Received::Quarantined(Reason::Fork)
        };
    }
    if seq > held + 1 {
        return Received::Gap { head: held };
    }
    let (prev_hash, prev_hlc) =
        log.last().map_or((EventHash::ZERO, Hlc::ZERO), |last| (last.hash(), last.body().hlc));
    if body.prev_hash != prev_hash {
        return Received::Quarantined(Reason::BrokenLink);
    }
    if body.hlc <= prev_hlc {
        return Received::Quarantined(Reason::ClockRegressed);
    }
    if model.holds_id(body.event_id) {
        return Received::Quarantined(Reason::DuplicateId);
    }
    Received::Stored(Box::new(event.clone()))
}

/// Makes the message `pick` describes, from what `model` holds, at physical time `now`.
fn make(model: &Model, pick: &Pick, now: Timestamp, now_ms: i64) -> Made {
    let signed = |event: SignedEvent, signer: u8| Made {
        bytes: event.to_bytes(),
        event: Some(event),
        signer,
    };
    let next = |n: u8, variant: u8, when: Timestamp| {
        next_event(n, here(), model.head(n), when, 1 + variant % 3, u64::from(variant))
    };
    match pick {
        Pick::Next { device, variant, late } => {
            let when = if *late { at(now_ms + 120_000) } else { now };
            signed(next(*device, *variant, when), *device)
        }
        Pick::Replay { device, at } => {
            let log = model.log(*device);
            if log.is_empty() {
                return signed(next(*device, 0, now), *device);
            }
            signed(log[at.index(log.len())].clone(), *device)
        }
        Pick::Fork { device, at, variant } => {
            let log = model.log(*device);
            if log.is_empty() {
                return signed(next(*device, *variant, now), *device);
            }
            let position = at.index(log.len());
            let head = if position == 0 { LogHead::EMPTY } else { LogHead::of(&log[position - 1]) };
            let fork = next_event(*device, here(), head, now, 1, 100 + u64::from(*variant));
            signed(fork, *device)
        }
        Pick::Ahead { device, variant } => {
            let first = next(*device, *variant, now);
            let second =
                next_event(*device, here(), LogHead::of(&first), now, 1, u64::from(*variant));
            signed(second, *device)
        }
        Pick::Broken { device, variant } => {
            let head = model.head(*device);
            let fake = LogHead::from_parts(head.seq(), stray_hash(*variant), head.hlc());
            signed(next_event(*device, here(), fake, now, 1, u64::from(*variant)), *device)
        }
        Pick::Regressed { device, variant } => {
            let behind = model.head(*device).hlc();
            let event = resigned(&next(*device, *variant, now), *device, |body| body.hlc = behind);
            signed(event, *device)
        }
        Pick::BadKey { device, variant } => {
            let other = if *device == PEERS[0] { PEERS[1] } else { PEERS[0] };
            signed(resigned(&next(*device, *variant, now), other, |_| {}), other)
        }
        Pick::Unknown { variant } => signed(next(UNKNOWN, *variant, now), UNKNOWN),
        Pick::Misplaced { device, variant } => {
            let event = resigned(&next(*device, *variant, now), *device, |body| {
                body.location = elsewhere();
            });
            signed(event, *device)
        }
        Pick::Foreign { variant, ahead } => {
            let first =
                next_event(FOREIGN, elsewhere(), LogHead::EMPTY, now, 1, u64::from(*variant));
            if !*ahead {
                return signed(first, FOREIGN);
            }
            let second =
                next_event(FOREIGN, elsewhere(), LogHead::of(&first), now, 1, u64::from(*variant));
            signed(second, FOREIGN)
        }
        Pick::DuplicateId { device, at, variant } => {
            let ids = model.all_ids();
            let event = next(*device, *variant, now);
            if ids.is_empty() {
                return signed(event, *device);
            }
            let used = ids[at.index(ids.len())];
            signed(resigned(&event, *device, |body| body.event_id = used), *device)
        }
        Pick::Far { device, variant, seq } => {
            let event = resigned(&next(*device, *variant, now), *device, |body| {
                body.origin_seq = NonZeroU64::new(*seq).unwrap();
            });
            signed(event, *device)
        }
        Pick::Garbage { bytes } => Made { bytes: bytes.clone(), event: None, signer: 0 },
        Pick::Again { at, changed } => again(model, *at, *changed),
        Pick::Own { variant, late } => {
            let when = if *late { at(now_ms + 3_600_000) } else { now };
            signed(next(OWN, *variant, when), OWN)
        }
        Pick::Revoked { variant } => {
            let head = model.head(REVOKED);
            let event = if head.seq() == 0 && *variant == 0 {
                trusted_first()
            } else {
                next_event(REVOKED, here(), head, now, 1, u64::from(*variant))
            };
            signed(event, REVOKED)
        }
    }
}

/// A message from the model's quarantine, received again: one whose fate has changed since, if
/// `changed` and there is one. Bytes that aren't an event if the quarantine is empty.
fn again(model: &Model, at: Index, changed: bool) -> Made {
    let aimed: Vec<&Made> = model
        .quarantine
        .iter()
        .filter(|(reason, made)| {
            !changed || expected(model, made) != Received::Quarantined(*reason)
        })
        .map(|(_, made)| made)
        .collect();
    let pool = if aimed.is_empty() {
        model.quarantine.iter().map(|(_, made)| made).collect()
    } else {
        aimed
    };
    if pool.is_empty() {
        return Made { bytes: vec![0xA0], event: None, signer: 0 };
    }
    pool[at.index(pool.len())].clone()
}

/// Applies a received message's fate to `model`, received at physical time `now_ms`.
fn apply_received(model: &mut Model, made: &Made, received: &Received, now_ms: i64) {
    match received {
        Received::Stored(event) => {
            let body = event.body();
            model.logs.entry(body.origin_device).or_default().push((**event).clone());
            // The device's own events catch its clock up; others' move it only within the
            // drift limit.
            let within = body.hlc.wall_ms()
                <= u64::try_from(at(now_ms).as_millis()).unwrap()
                    + u64::try_from(DRIFT.as_millis()).unwrap();
            if body.origin_device == device(OWN) || within {
                model.floor = model.floor.max(body.hlc);
            }
        }
        Received::Quarantined(reason) => {
            if !model.quarantine.iter().any(|(_, refused)| refused.bytes == made.bytes) {
                model.quarantine.push((*reason, made.clone()));
            }
        }
        Received::Duplicate | Received::Gap { .. } => {}
    }
}

/// Whether a received message's fate reaches [`Point::Stored`]: it was stored or quarantined.
fn reaches_stored(received: &Received) -> bool {
    matches!(received, Received::Stored(_) | Received::Quarantined(_))
}

// ---------------------------------------------------------------------------------------------
// Running a case.

/// A fault hook the test arms before a write.
#[derive(Clone, Default)]
struct Plan(Arc<Mutex<Option<(Point, u8, u8)>>>);

impl Plan {
    fn arm(&self, point: Point, nth: u8) {
        *self.0.lock().unwrap() = Some((point, nth, 0));
    }

    fn disarm(&self) {
        *self.0.lock().unwrap() = None;
    }
}

impl Faults for Plan {
    fn proceed(&mut self, point: Point) -> bool {
        let mut armed = self.0.lock().unwrap();
        if let Some((target, nth, seen)) = armed.as_mut()
            && *target == point
        {
            *seen += 1;
            if *seen == *nth {
                *armed = None;
                return false;
            }
        }
        true
    }
}

type TestStore = Store<keel_events::keys::SoftwareSigner, SeededEntropy>;

fn open(scratch: &Scratch, plan: &Plan, seed: u64) -> TestStore {
    Store::open_with_faults(
        scratch.db(),
        store_key(),
        config(),
        signer(OWN),
        SeededEntropy::new(seed),
        Box::new(plan.clone()),
    )
    .unwrap()
}

#[derive(Debug)]
enum WriteError {
    Store(StoreError),
    Test(String),
}

impl From<StoreError> for WriteError {
    fn from(error: StoreError) -> WriteError {
        WriteError::Store(error)
    }
}

fn check<T: PartialEq + core::fmt::Debug>(got: &T, want: &T, what: &str) -> Result<(), WriteError> {
    if got == want {
        Ok(())
    } else {
        Err(WriteError::Test(format!("{what}: got {got:?}, want {want:?}")))
    }
}

/// A case's running state: the store, the model, and the test's clock.
struct Case {
    scratch: Scratch,
    plan: Plan,
    /// The store, closed only while reopening.
    store: Option<TestStore>,
    model: Model,
    now_ms: i64,
    reopened: u64,
}

impl Case {
    fn new() -> Case {
        let scratch = Scratch::new("props");
        let plan = Plan::default();
        let store = Some(open(&scratch, &plan, 1_000));
        Case { scratch, plan, store, model: Model::new(), now_ms: 10_000, reopened: 0 }
    }

    fn store(&self) -> &TestStore {
        self.store.as_ref().unwrap()
    }

    /// Runs a write, and checks each step's outcome against the model.
    fn write(&mut self, steps: &[(Step, bool)], ending: Ending) -> Result<(), TestCaseError> {
        if let Ending::Interrupt(point, nth) = ending {
            self.plan.arm(point, nth);
        }
        let interrupt_stored = match ending {
            Ending::Interrupt(Point::Stored, nth) => Some(nth),
            _ => None,
        };
        let mut tentative = self.model.clone();
        let mut stored_points = 0_u8;
        // Whether a step is expected to be interrupted.
        let mut interrupted_at: Option<Point> = None;
        let registry = registry();
        let now_ms = &mut self.now_ms;
        let store = self.store.as_mut().unwrap();
        let written: Result<(), WriteError> = store.write(|w| {
            for (step, together) in steps {
                if !together {
                    *now_ms += 7;
                }
                let now = at(*now_ms);
                match step {
                    Step::Append { stream, note } => {
                        let appended = w.append(draft(*stream, u64::from(*note)), now);
                        stored_points += 1;
                        if interrupt_stored == Some(stored_points) {
                            interrupted_at = Some(Point::Stored);
                            return appended.map(|_| ()).map_err(WriteError::from);
                        }
                        let event = appended?;
                        check_appended(&tentative, &event, *stream, *note, now)?;
                        tentative.floor = tentative.floor.max(event.body().hlc);
                        tentative.logs.entry(device(OWN)).or_default().push(event);
                    }
                    Step::Receive(pick) => {
                        let made = make(&tentative, pick, now, *now_ms);
                        let want = expected(&tentative, &made);
                        let received = w.receive(&made.bytes, &registry, now);
                        if reaches_stored(&want) {
                            stored_points += 1;
                            if interrupt_stored == Some(stored_points) {
                                interrupted_at = Some(Point::Stored);
                                return received.map(|_| ()).map_err(WriteError::from);
                            }
                        }
                        let received = received?;
                        check(&received, &want, &format!("receiving {pick:?}"))?;
                        apply_received(&mut tentative, &made, &received, *now_ms);
                    }
                }
            }
            if ending == Ending::Fail {
                return Err(WriteError::Test("the caller fails".to_owned()));
            }
            Ok(())
        });
        self.plan.disarm();
        let expected_point = match ending {
            Ending::Interrupt(Point::Stored, _) => interrupted_at,
            Ending::Interrupt(Point::Committed, _) | Ending::Commit | Ending::Fail => None,
            Ending::Interrupt(point, _) => Some(point),
        };
        match (written, ending, expected_point) {
            (Ok(()), Ending::Fail, _) => prop_assert!(false, "a failing caller's write committed"),
            (Ok(()), _, None) => {
                self.model = tentative;
                self.model.committed_clock = self.store().clock();
            }
            (Ok(()), _, Some(point)) => prop_assert!(false, "not interrupted at {point:?}"),
            (Err(WriteError::Test(message)), Ending::Fail, None)
                if message == "the caller fails" => {}
            (Err(WriteError::Test(message)), _, _) => prop_assert!(false, "{}", message),
            (Err(WriteError::Store(StoreError::Interrupted(point))), _, Some(expected)) => {
                prop_assert_eq!(point, expected);
            }
            (Err(WriteError::Store(error)), _, _) => prop_assert!(false, "store error: {error:?}"),
        }
        prop_assert!(self.store().clock() >= self.model.floor, "the clock follows what was stored");
        prop_assert_eq!(
            self.store().clock(),
            self.model.committed_clock,
            "a failed write gives its clock back"
        );
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), TestCaseError> {
        self.reopened += 1;
        // Close the store before opening it again.
        self.store = None;
        self.store = Some(open(&self.scratch, &self.plan, 1_000 + self.reopened));
        prop_assert_eq!(self.store().clock(), self.model.committed_clock, "the clock is stored");
        Ok(())
    }

    /// Opens the store as `stranger`, which must be refused unless the store can't tell, then
    /// reopens it as the device.
    fn open_as(&mut self, stranger: Stranger) -> Result<(), TestCaseError> {
        self.store = None;
        let (key, config, signer) = match stranger {
            Stranger::Device => (
                store_key(),
                StoreConfig { device: device(PEERS[0]), ..config() },
                signer(PEERS[0]),
            ),
            Stranger::Location => {
                (store_key(), StoreConfig { location: elsewhere(), ..config() }, signer(OWN))
            }
            Stranger::Signer => (store_key(), config(), signer(PEERS[0])),
            Stranger::StoreKey => (store_key_of(0x4C), config(), signer(OWN)),
        };
        let opened = Store::open(self.scratch.db(), key, config, signer, SeededEntropy::new(99));
        // The signer is checked against the device's own events: a store without any can't
        // tell.
        let unsigned = self.model.log(OWN).is_empty();
        match (stranger, opened) {
            (Stranger::Device | Stranger::Location, Err(StoreError::NotThisDevice))
            | (Stranger::StoreKey, Err(StoreError::KeyRejected)) => {}
            (Stranger::Signer, Err(StoreError::WrongSigner)) if !unsigned => {}
            (Stranger::Signer, Ok(_)) if unsigned => {}
            (stranger, opened) => {
                prop_assert!(false, "{stranger:?} opening the store: {:?}", opened.map(|_| ()));
            }
        }
        self.reopen()
    }

    /// Checks everything the store reads back against the model.
    fn agrees(&self) -> Result<(), TestCaseError> {
        let store = self.store();
        let model = &self.model;
        let vector: BTreeMap<Id<Device>, u64> = model
            .logs
            .iter()
            .filter(|(_, log)| !log.is_empty())
            .map(|(device, log)| (*device, u64::try_from(log.len()).unwrap()))
            .collect();
        prop_assert_eq!(store.version_vector().unwrap(), vector);
        for n in [OWN, PEERS[0], PEERS[1], REVOKED, UNKNOWN, FOREIGN] {
            let log = model.log(n);
            prop_assert_eq!(store.head(device(n)).unwrap(), model.head(n));
            prop_assert_eq!(&store.log(device(n), 0, 1_000).unwrap()[..], log);
            let half = u64::try_from(log.len() / 2).unwrap();
            let expected: Vec<SignedEvent> =
                log.iter().skip(log.len() / 2).take(2).cloned().collect();
            prop_assert_eq!(store.log(device(n), half, 2).unwrap(), expected);
            let held = u64::try_from(log.len()).unwrap();
            for after in [held, 1 << 63, u64::MAX] {
                prop_assert_eq!(store.log(device(n), after, 10).unwrap(), vec![]);
            }
            if let Some(last) = log.last() {
                prop_assert_eq!(store.event(last.body().event_id).unwrap(), Some(last.clone()));
            }
        }
        prop_assert_eq!(store.event(support::id(0xDEAD)).unwrap(), None);
        for s in 1..=3 {
            let mut expected: Vec<SignedEvent> = model
                .logs
                .values()
                .flatten()
                .filter(|event| event.body().stream == support::stream(s))
                .cloned()
                .collect();
            expected.sort_by_key(|event| {
                let body = event.body();
                (body.hlc, body.origin_device, body.origin_seq)
            });
            prop_assert_eq!(store.stream(&support::stream(s)).unwrap(), expected);
        }
        let quarantine: Vec<(Reason, Vec<u8>)> = store
            .quarantine()
            .unwrap()
            .into_iter()
            .map(|Quarantined { reason, message }| (reason, message))
            .collect();
        let expected: Vec<(Reason, Vec<u8>)> =
            model.quarantine.iter().map(|(reason, made)| (*reason, made.bytes.clone())).collect();
        prop_assert_eq!(quarantine, expected);
        Ok(())
    }
}

/// Checks an appended event against the model: the device's next, for the store's location, on
/// the drafted stream, and signed with the device's key. Its clock is later than everything the
/// device's clock follows, and no later than it has to be: at physical time `now`, or just after
/// what its clock follows. A clock dragged along by a received HLC beyond the drift limit, or by
/// an event of a write that failed, would be ahead of both.
fn check_appended(
    model: &Model,
    event: &SignedEvent,
    stream: u8,
    note: u8,
    now: Timestamp,
) -> Result<(), WriteError> {
    let body = event.body();
    let head = model.head(OWN);
    check(&body.origin_device, &device(OWN), "an appended event's device")?;
    check(&body.origin_seq.get(), &(head.seq() + 1), "an appended event's position")?;
    check(&body.prev_hash, &head.hash(), "an appended event's link")?;
    check(&body.location, &here(), "an appended event's location")?;
    check(&body.stream, &support::stream(stream), "an appended event's stream")?;
    check(&body.payload, &draft(stream, u64::from(note)).payload, "an appended event's payload")?;
    check(
        &registry().verify(&event.to_bytes()).as_ref(),
        &Ok(event),
        "an appended event's signature",
    )?;
    if body.hlc <= model.floor {
        return Err(WriteError::Test(format!(
            "an appended event's clock {:?} isn't after {:?}",
            body.hlc, model.floor
        )));
    }
    let ceiling = model.floor.wall_ms().max(u64::try_from(now.as_millis()).unwrap());
    if body.hlc.wall_ms() > ceiling {
        return Err(WriteError::Test(format!(
            "an appended event's clock {:?} is ahead of {ceiling}",
            body.hlc
        )));
    }
    Ok(())
}

proptest! {
    /// Any sequence of writes and reopenings leaves the store exactly as the model says: every
    /// received event's fate, every appended event, rollbacks of failed and interrupted writes,
    /// and everything read back.
    #[test]
    fn the_store_does_what_the_model_says(ops in prop::collection::vec(any_op(), 1..10)) {
        let mut case = Case::new();
        for op in &ops {
            match op {
                Op::Write(steps, ending) => case.write(steps, *ending)?,
                Op::Reopen => case.reopen()?,
                Op::OpenAs(stranger) => case.open_as(*stranger)?,
            }
            case.agrees()?;
        }
    }
}
