//! What the property and crash tests share: devices and their keys, the registry, drafts, and
//! directories to keep stores in.

#![allow(
    dead_code,
    unreachable_pub,
    reason = "each test crate uses its own part of the support module"
)]

use core::num::NonZeroU32;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;
use std::path::PathBuf;

use keel_events::cbor::Value;
use keel_events::envelope::{
    Actor, Device, EventBody, Location, Payload, SchemaName, SchemaRef, StreamKind, StreamRef,
};
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_events::log::{EventDraft, LogConfig, LogHead, LogWriter};
use keel_events::verify::{DeviceRegistry, Revocation};
use keel_store::StoreConfig;
use keel_types::{Hlc, Id, SeededEntropy, Timestamp};

pub fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

/// The location the store keeps events for.
pub fn here() -> Id<Location> {
    id(0x100)
}

pub fn elsewhere() -> Id<Location> {
    id(0x101)
}

/// Device `n`'s identifier.
pub fn device(n: u8) -> Id<Device> {
    id(u64::from(n))
}

/// Device `n`'s key: its secret is `n` repeated.
pub fn key(n: u8) -> SoftwareSigner {
    SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[n; 32]).unwrap()
}

/// The device the store belongs to.
pub const OWN: u8 = 1;
/// Devices enrolled here, whose events the store takes.
pub const PEERS: [u8; 2] = [2, 3];
/// A device enrolled here and revoked after its first event, [`trusted_first`].
pub const REVOKED: u8 = 4;
/// A device nobody enrolled.
pub const UNKNOWN: u8 = 5;
/// A device enrolled at another location.
pub const FOREIGN: u8 = 9;

pub const DRIFT: Duration = Duration::from_secs(60);

pub fn config() -> StoreConfig {
    StoreConfig { device: device(OWN), location: here(), max_forward_drift: DRIFT }
}

/// `ms` milliseconds past 2026-09-28, 00:00 UTC.
pub fn at(ms: i64) -> Timestamp {
    Timestamp::from_millis(1_790_553_600_000_i64.checked_add(ms).unwrap()).unwrap()
}

/// The HLC of a wall time `ms` past the base, with counter `logical`.
pub fn hlc_at(ms: i64, logical: u16) -> Hlc {
    Hlc::new(u64::try_from(at(ms).as_millis()).unwrap(), logical).unwrap()
}

pub fn stream(n: u8) -> StreamRef {
    StreamRef { kind: StreamKind::new("order").unwrap(), id: id(0x5000_u64 | u64::from(n)) }
}

/// A draft on stream `stream_n`, with `note` as its payload.
pub fn draft(stream_n: u8, note: u64) -> EventDraft {
    EventDraft {
        stream: stream(stream_n),
        schema: SchemaRef {
            name: SchemaName::new("order.noted").unwrap(),
            version: NonZeroU32::MIN,
        },
        business_date: "2026-09-28".parse().unwrap(),
        actor: Actor::TeamMember(id(0x300)),
        approval: None,
        causation: None,
        correlation: None,
        payload: Payload::new(&Value::Unsigned(note)).unwrap(),
    }
}

/// The next event of device `n`'s log after `head`, written at `location` at physical time `now`
/// on stream `stream_n`. `variant` picks its identifier and payload, so different variants are
/// different events at the same position.
pub fn next_event(
    n: u8,
    location: Id<Location>,
    head: LogHead,
    now: Timestamp,
    stream_n: u8,
    variant: u64,
) -> SignedEvent {
    let config = LogConfig {
        device: device(n),
        location,
        head,
        latest_hlc: Hlc::ZERO,
        max_forward_drift: DRIFT,
    };
    let seed = variant.wrapping_mul(0x9E37_79B9).wrapping_add(u64::from(n));
    let mut writer = LogWriter::new(config, key(n), SeededEntropy::new(seed));
    writer.prepare(draft(stream_n, variant), now).unwrap().commit()
}

/// The revoked device's only trusted event: its first, as [`next_event`] writes it with variant 0
/// at time 0 on stream 1.
pub fn trusted_first() -> SignedEvent {
    next_event(REVOKED, here(), LogHead::EMPTY, at(0), 1, 0)
}

/// Every device, enrolled where it belongs; [`REVOKED`] revoked after [`trusted_first`].
pub fn registry() -> DeviceRegistry {
    let mut registry = DeviceRegistry::new();
    for n in [OWN, PEERS[0], PEERS[1], REVOKED] {
        registry.enroll(device(n), here(), key(n).public_key().clone()).unwrap();
    }
    registry.enroll(device(FOREIGN), elsewhere(), key(FOREIGN).public_key().clone()).unwrap();
    let revocation = Revocation { after_seq: 1, last_trusted: trusted_first().hash() };
    registry.revoke(device(REVOKED), revocation).unwrap();
    registry
}

/// `event`'s body, changed by `change`, signed with device `n`'s key.
pub fn resigned(event: &SignedEvent, n: u8, change: impl FnOnce(&mut EventBody)) -> SignedEvent {
    let mut body = event.body().clone();
    change(&mut body);
    SignedEvent::sign(body, &key(n)).unwrap()
}

/// A hash no event has.
pub fn stray_hash(n: u8) -> EventHash {
    EventHash::from_bytes([n; 32])
}

/// A directory of its own for a store, removed when dropped. It is in memory (`/dev/shm`) where
/// there is one, so that the many commits of property tests don't each wait for a disk.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let shm = std::path::Path::new("/dev/shm");
        let base = if shm.is_dir() { shm.to_path_buf() } else { std::env::temp_dir() };
        let dir = base.join(format!("keel-store-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    /// The store's database file.
    pub fn db(&self) -> PathBuf {
        self.0.join("store.db")
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
