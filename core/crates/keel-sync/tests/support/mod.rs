//! What `keel-sync`'s tests share: devices and their keys, drafts, and a model replica that
//! receives events as `keel-store` does, in memory.

#![allow(
    dead_code,
    unreachable_pub,
    reason = "each test crate uses its own part of the support module"
)]

use core::num::NonZeroU32;
use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};

use keel_events::cbor::Value;
use keel_events::envelope::{
    Actor, Device, Event, Location, Payload, SchemaName, SchemaRef, StreamKind, StreamRef,
};
use keel_events::event::SignedEvent;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_events::log::{ChainError, EventDraft, Link, LogConfig, LogHead, LogWriter};
use keel_events::verify::DeviceRegistry;
use keel_store::{Reason, Received};
use keel_sync::{Replica, VersionVector};
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
/// two events at one position kept, and what it refuses quarantined.
#[derive(Debug)]
pub struct Model {
    location: Id<Location>,
    device: Id<Device>,
    registry: DeviceRegistry,
    writer: LogWriter<SoftwareSigner, SeededEntropy>,
    logs: BTreeMap<Id<Device>, Vec<SignedEvent>>,
    ids: BTreeSet<Id<Event>>,
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
        self.ids.insert(event.body().event_id);
        self.logs.entry(event.body().origin_device).or_default().push(event);
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
