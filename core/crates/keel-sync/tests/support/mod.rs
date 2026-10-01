//! What `keel-sync`'s tests share: devices and their keys, drafts, and a model replica that
//! receives events as `keel-store` does, in memory.

#![allow(
    dead_code,
    unreachable_pub,
    reason = "each test crate uses its own part of the support module"
)]

use core::num::{NonZeroU8, NonZeroU32};
use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};

use keel_domain::hub::{self, Claim, ClaimError, Claimed, HubEvent, Term};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, Run, SequenceEvent};
use keel_events::cbor::Value;
use keel_events::envelope::{
    Actor, Component, Device, Event, Location, Payload, SchemaName, SchemaRef, StreamKind,
    StreamRef,
};
use keel_events::event::SignedEvent;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_events::log::{ChainError, EventDraft, Link, LogConfig, LogHead, LogWriter};
use keel_events::verify::DeviceRegistry;
use keel_store::{Reason, Received};
use keel_sync::{Claiming, Replica, VersionVector};
use keel_types::{Hlc, Id, SeededEntropy, Timestamp};

pub fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

/// The location every replica keeps events for.
pub fn here() -> Id<Location> {
    id(0x10)
}

/// Another location.
pub fn elsewhere() -> Id<Location> {
    id(0x11)
}

/// Device `n`.
pub fn device(n: u8) -> Id<Device> {
    id(u64::from(n))
}

/// Device `n`'s signing key: its secret is `n` repeated.
pub fn signer(n: u8) -> SoftwareSigner {
    SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[n; 32]).unwrap()
}

/// Devices `devices`, enrolled here.
pub fn registry(devices: impl IntoIterator<Item = u8>) -> DeviceRegistry {
    let mut registry = DeviceRegistry::new();
    for n in devices {
        registry.enroll(device(n), here(), signer(n).public_key().clone()).unwrap();
    }
    registry
}

/// Milliseconds since the Unix epoch at the tests' time 0, on 2026-09-30.
pub const BASE: i64 = 1_790_762_400_000;

/// `ms` milliseconds past the tests' time 0.
pub fn at(ms: i64) -> Timestamp {
    Timestamp::from_millis(BASE.checked_add(ms).unwrap()).unwrap()
}

/// How far ahead of physical time a remote HLC may be.
pub const DRIFT: Duration = Duration::from_secs(60);

/// A draft with `note` as its payload.
pub fn draft(note: u64) -> EventDraft {
    EventDraft {
        stream: StreamRef { kind: StreamKind::new("order").unwrap(), id: id(0x5000) },
        schema: SchemaRef {
            name: SchemaName::new("order.noted").unwrap(),
            version: NonZeroU32::MIN,
        },
        business_date: "2026-09-30".parse().unwrap(),
        actor: Actor::TeamMember(id(0x300)),
        approval: None,
        causation: None,
        correlation: None,
        payload: Payload::new(&Value::Unsigned(note)).unwrap(),
    }
}

/// The model replica's error: its next receive was set to fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failed;

/// A replica in memory that appends to its device's log and receives events as `keel-store`
/// does: verified with the registry, for its location, each device's log in order, the first of
/// two events at one position kept, and what it refuses quarantined. It works out the chain of
/// terms from the claims it holds, claims the hub's role, and as the Store Hub numbers what it
/// holds in `sequence.assigned` records, as `keel-store` does (ADR-0020, ADR-0022).
#[derive(Debug)]
pub struct Model {
    location: Id<Location>,
    device: Id<Device>,
    registry: DeviceRegistry,
    writer: LogWriter<SoftwareSigner, SeededEntropy>,
    logs: BTreeMap<Id<Device>, Vec<SignedEvent>>,
    ids: BTreeSet<Id<Event>>,
    /// Every event it holds, in the order it came to: its device and position.
    arrivals: Vec<(Id<Device>, u64)>,
    /// How many times it was asked to sequence.
    pub sequenced: u64,
    /// How many times it was asked to answer requests for orders: it holds no orders, so it
    /// never has any to answer.
    pub answered: u64,
    /// What it refused, and why.
    pub quarantine: Vec<(Reason, Vec<u8>)>,
    /// Whether its next receive fails, storing nothing.
    pub fail_next: bool,
    /// Whether its next receive declines every event, as a replica that can't take them for now
    /// would: it stores none, and reports each as after a gap.
    pub decline_next: bool,
}

impl Model {
    /// Device `n`'s replica, verifying with `registry`, holding nothing.
    pub fn new(n: u8, registry: DeviceRegistry) -> Model {
        let config = LogConfig {
            device: device(n),
            location: here(),
            head: LogHead::EMPTY,
            latest_hlc: Hlc::ZERO,
            max_forward_drift: DRIFT,
        };
        Model {
            location: here(),
            device: device(n),
            registry,
            writer: LogWriter::new(config, signer(n), SeededEntropy::new(u64::from(n))),
            logs: BTreeMap::new(),
            ids: BTreeSet::new(),
            arrivals: Vec::new(),
            sequenced: 0,
            answered: 0,
            quarantine: Vec::new(),
            fail_next: false,
            decline_next: false,
        }
    }

    /// Appends the device's next event, with `note` as its payload, at physical time `now`.
    pub fn append(&mut self, note: u64, now: Timestamp) -> SignedEvent {
        let event = self.writer.prepare(draft(note), now).unwrap().commit();
        self.hold(event.clone());
        event
    }

    /// Every device's log it holds.
    pub const fn logs(&self) -> &BTreeMap<Id<Device>, Vec<SignedEvent>> {
        &self.logs
    }

    /// Every event it holds, device by device, each log in order.
    pub fn events(&self) -> Vec<SignedEvent> {
        self.logs.values().flatten().cloned().collect()
    }

    /// Receives one event, as a batch of its own would, at the tests' time 0.
    pub fn receive_one(&mut self, bytes: &[u8]) -> Received {
        self.take(bytes, at(0))
    }

    fn hold(&mut self, event: SignedEvent) {
        let body = event.body();
        self.ids.insert(body.event_id);
        self.arrivals.push((body.origin_device, body.origin_seq.get()));
        self.logs.entry(body.origin_device).or_default().push(event);
    }

    /// The events it holds that no record that counts covers, except records, in the order it
    /// came to hold them: for each device, those after the last position a record that counts
    /// covers; and for its own log, as the hub, after its own last record of its epoch too.
    pub fn unsequenced(&self) -> Vec<(Id<Device>, u64)> {
        let terms = self.chain();
        let epoch = terms.first().filter(|term| term.device == self.device).map(|term| term.epoch);
        let mut covered: BTreeMap<Id<Device>, u64> = BTreeMap::new();
        for (author, position, record) in self.counting(&terms) {
            for run in &record.runs {
                let to = covered.entry(run.device).or_insert(0);
                *to = (*to).max(run.to);
            }
            if author == self.device && Some(record.epoch) == epoch.map(hub::Epoch::get) {
                let to = covered.entry(author).or_insert(0);
                *to = (*to).max(position);
            }
        }
        self.arrivals
            .iter()
            .copied()
            .filter(|&(device, position)| {
                let event = &self.logs[&device][usize::try_from(position - 1).unwrap()];
                position > covered.get(&device).copied().unwrap_or(0) && !is_record(event)
            })
            .collect()
    }

    /// The records it holds, with their authors.
    pub fn records(&self) -> Vec<(Id<Device>, Assigned)> {
        self.logs
            .values()
            .flatten()
            .filter(|event| is_record(event))
            .map(|event| {
                let body = event.body();
                let Ok(SequenceEvent::Assigned(record)) =
                    SequenceEvent::decode(&body.schema, &body.payload)
                else {
                    panic!("an unreadable record");
                };
                (body.origin_device, record)
            })
            .collect()
    }

    /// The claims it holds.
    pub fn claims(&self) -> Vec<Claim> {
        self.logs
            .values()
            .flatten()
            .filter(|event| is_claim(event))
            .filter_map(|event| {
                let body = event.body();
                let Ok(HubEvent::Claimed(claimed)) = HubEvent::decode(&body.schema, &body.payload)
                else {
                    return None;
                };
                Some(Claim {
                    event: body.event_id,
                    device: body.origin_device,
                    position: body.origin_seq.get(),
                    claimed,
                })
            })
            .collect()
    }

    /// The chain of terms its claims make.
    pub fn chain(&self) -> Vec<Term> {
        hub::chain(&self.claims())
    }

    /// The records that count on its chain: each with its author and position.
    pub fn counting_records(&self) -> Vec<(Id<Device>, u64, Assigned)> {
        self.counting(&self.chain())
    }

    /// The records that count, given the chain `terms`: each with its author and position.
    fn counting(&self, terms: &[Term]) -> Vec<(Id<Device>, u64, Assigned)> {
        self.records_at()
            .into_iter()
            .filter(|(author, position, record)| {
                terms.iter().any(|term| {
                    term.device == *author
                        && term.epoch.get() == record.epoch
                        && term.counts(*position)
                })
            })
            .collect()
    }

    /// The records it holds, with their authors and positions.
    fn records_at(&self) -> Vec<(Id<Device>, u64, Assigned)> {
        self.logs
            .values()
            .flatten()
            .filter(|event| is_record(event))
            .map(|event| {
                let body = event.body();
                let Ok(SequenceEvent::Assigned(record)) =
                    SequenceEvent::decode(&body.schema, &body.payload)
                else {
                    panic!("an unreadable record");
                };
                (body.origin_device, body.origin_seq.get(), record)
            })
            .collect()
    }

    /// Appends the device's next event of `draft` at physical time `now`, as the replica does
    /// for what it writes of its own.
    fn write(&mut self, draft: EventDraft, now: Timestamp) -> SignedEvent {
        let event = self.writer.prepare(draft, now).unwrap().commit();
        self.hold(event.clone());
        event
    }

    fn refuse(&mut self, bytes: &[u8], reason: Reason) -> Received {
        self.quarantine.push((reason, bytes.to_vec()));
        Received::Quarantined(reason)
    }

    fn take(&mut self, bytes: &[u8], now: Timestamp) -> Received {
        let Ok(event) = self.registry.verify(bytes) else {
            return self.refuse(bytes, Reason::Rejected);
        };
        let body = event.body();
        if body.location != self.location {
            return self.refuse(bytes, Reason::OtherLocation);
        }
        let device = body.origin_device;
        let log = self.logs.get(&device).map_or(&[][..], Vec::as_slice);
        let head = log.last().map_or(LogHead::EMPTY, LogHead::of);
        match head.link(&event) {
            Ok(Link::Next) if self.ids.contains(&body.event_id) => {
                self.refuse(bytes, Reason::DuplicateId)
            }
            Ok(Link::Next) => {
                if device == self.device {
                    let latest = self.writer.latest_hlc();
                    self.writer.restore(LogHead::of(&event), latest);
                } else {
                    let _ = self.writer.observe(body.hlc, now);
                }
                self.hold(event.clone());
                Received::Stored(Box::new(event))
            }
            Ok(Link::Duplicate) => Received::Duplicate,
            Ok(Link::Earlier) => {
                let position = usize::try_from(body.origin_seq.get()).unwrap();
                if log[position.checked_sub(1).unwrap()].hash() == event.hash() {
                    Received::Duplicate
                } else {
                    self.refuse(bytes, Reason::Fork)
                }
            }
            Err(ChainError::Gap { head, .. }) => Received::Gap { head },
            Err(ChainError::Fork { .. }) => self.refuse(bytes, Reason::Fork),
            Err(_) => self.refuse(bytes, Reason::BrokenLink),
        }
    }
}

impl Replica for Model {
    type Error = Failed;

    fn location(&self) -> Id<Location> {
        self.location
    }

    fn device(&self) -> Id<Device> {
        self.device
    }

    fn version_vector(&mut self) -> Result<VersionVector, Failed> {
        Ok(self
            .logs
            .iter()
            .map(|(device, log)| (*device, u64::try_from(log.len()).unwrap()))
            .collect())
    }

    fn events_after(
        &mut self,
        device: Id<Device>,
        after: u64,
        limit: u32,
    ) -> Result<Vec<Vec<u8>>, Failed> {
        let log = self.logs.get(&device).map_or(&[][..], Vec::as_slice);
        let after = usize::try_from(after).unwrap();
        Ok(log
            .iter()
            .skip(after)
            .take(usize::try_from(limit).unwrap())
            .map(SignedEvent::to_bytes)
            .collect())
    }

    fn terms(&mut self) -> Result<Vec<Term>, Failed> {
        Ok(self.chain())
    }

    fn claim(&mut self, priority: NonZeroU8, now: Timestamp) -> Result<Claiming, Failed> {
        let terms = self.chain();
        if terms.first().is_some_and(|term| term.device == self.device) {
            return Ok(Claiming::Hub);
        }
        let held = |device: Id<Device>| {
            self.logs.get(&device).map_or(0, |log| u64::try_from(log.len()).unwrap())
        };
        let claimed = match Claimed::succeeding(&terms, priority, held) {
            Ok(claimed) => claimed,
            Err(ClaimError::Behind) => return Ok(Claiming::Behind),
            Err(error) => panic!("a claim that can't be made: {error}"),
        };
        let (schema, payload) = HubEvent::Claimed(claimed).encode().unwrap();
        let stream = StreamRef {
            kind: StreamKind::new(hub::STREAM).unwrap(),
            id: self.writer.generate_id(now).unwrap(),
        };
        let claim = EventDraft {
            stream,
            schema,
            payload,
            actor: Actor::System(Component::new("hub").unwrap()),
            ..draft(0)
        };
        Ok(Claiming::Claimed(Box::new(self.write(claim, now))))
    }

    fn answer_requests(&mut self, _: Timestamp) -> Result<Vec<SignedEvent>, Failed> {
        // Only the hub answers, and it answers before it numbers.
        assert!(self.is_hub(), "a replica that isn't the hub answered");
        assert_eq!(self.answered, self.sequenced, "the hub numbered before it answered");
        self.answered += 1;
        Ok(Vec::new())
    }

    fn sequence(&mut self, now: Timestamp) -> Result<Vec<SignedEvent>, Failed> {
        assert!(self.is_hub(), "a replica that isn't the hub numbered");
        assert_eq!(self.answered, self.sequenced + 1, "the hub numbered before it answered");
        self.sequenced += 1;
        let terms = self.chain();
        let epoch = terms[0].epoch.get();
        let mut next = self
            .counting(&terms)
            .iter()
            .filter(|(author, _, record)| *author == self.device && record.epoch == epoch)
            .map(|(_, _, record)| record.last() + 1)
            .max()
            .unwrap_or(1);
        // Runs in the order the events came, a record ending where a device's next event
        // doesn't follow on from its run in it, as the store's do.
        let mut records: Vec<Vec<Run>> = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        for (device, position) in self.unsequenced() {
            let last = self.logs[&device][usize::try_from(position - 1).unwrap()].hash();
            match runs.last_mut() {
                Some(run) if run.device == device && run.to + 1 == position => {
                    run.to = position;
                    run.last = last;
                }
                _ => {
                    let before = runs.iter().rev().find(|run| run.device == device);
                    if before.is_some_and(|run| run.to + 1 != position) {
                        records.push(core::mem::take(&mut runs));
                    }
                    runs.push(Run { device, from: position, to: position, last });
                }
            }
        }
        if !runs.is_empty() {
            records.push(runs);
        }
        let mut written = Vec::new();
        for runs in records {
            let record = Assigned::new(epoch, next, runs).unwrap();
            next = record.last() + 1;
            let (schema, payload) = SequenceEvent::Assigned(record).encode().unwrap();
            let stream = StreamRef {
                kind: StreamKind::new("sequence").unwrap(),
                id: self.writer.generate_id(now).unwrap(),
            };
            let draft = EventDraft {
                stream,
                schema,
                payload,
                actor: Actor::System(Component::new("sequencer").unwrap()),
                ..draft(0)
            };
            written.push(self.write(draft, now));
        }
        Ok(written)
    }

    fn receive(&mut self, events: &[Vec<u8>], now: Timestamp) -> Result<Vec<Received>, Failed> {
        if core::mem::take(&mut self.fail_next) {
            return Err(Failed);
        }
        if core::mem::take(&mut self.decline_next) {
            let head = |bytes: &Vec<u8>| {
                let device =
                    SignedEvent::from_stored(bytes).map(|event| event.body().origin_device);
                let log = device.ok().and_then(|device| self.logs.get(&device));
                log.map_or(0, |log| u64::try_from(log.len()).unwrap())
            };
            return Ok(events.iter().map(|bytes| Received::Gap { head: head(bytes) }).collect());
        }
        Ok(events.iter().map(|bytes| self.take(bytes, now)).collect())
    }
}

/// Whether `event` is a sequencing record.
pub fn is_record(event: &SignedEvent) -> bool {
    event.body().stream.kind.as_str() == "sequence"
}

/// Whether `event` is a claim of the hub's role.
pub fn is_claim(event: &SignedEvent) -> bool {
    event.body().stream.kind.as_str() == hub::STREAM
}

impl Model {
    /// Whether it holds the winning claim: it is the hub.
    pub fn is_hub(&self) -> bool {
        self.chain().first().is_some_and(|term| term.device == self.device)
    }
}
