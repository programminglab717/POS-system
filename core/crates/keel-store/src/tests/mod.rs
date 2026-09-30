//! Known-answer tests: each rule of the store, worked through with a few devices.

use core::fmt::Write as _;
use core::num::{NonZeroU32, NonZeroU64};
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;
use std::path::{Path, PathBuf};

use keel_events::cbor::Value;
use keel_events::envelope::{
    Actor, Device, EventBody, Location, Payload, SchemaName, SchemaRef, StreamKind, StreamRef,
};
use keel_events::event::SignedEvent;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_events::log::{EventDraft, LogConfig, LogHead, LogWriter};
use keel_events::verify::{DeviceRegistry, Revocation};
use keel_types::{Hlc, Id, SeededEntropy, Timestamp};
use rusqlite::Connection;

use super::*;

mod encryption;
mod integrity;
mod outbox;
mod projections;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn here() -> Id<Location> {
    id(0x100)
}

fn elsewhere() -> Id<Location> {
    id(0x101)
}

/// A device and its key: device `n`'s secret is `n` repeated.
fn device(n: u8) -> (Id<Device>, SoftwareSigner) {
    let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[n; 32]).unwrap();
    (id(u64::from(n)), signer)
}

/// The device the store belongs to.
fn own() -> (Id<Device>, SoftwareSigner) {
    device(1)
}

fn config() -> StoreConfig {
    StoreConfig { device: own().0, location: here(), max_forward_drift: Duration::from_secs(60) }
}

/// Milliseconds past 2026-09-28, 00:00 UTC.
fn at(ms: i64) -> Timestamp {
    Timestamp::from_millis(1_790_553_600_000_i64.checked_add(ms).unwrap()).unwrap()
}

fn stream(n: u64) -> StreamRef {
    StreamRef {
        kind: StreamKind::new("order").unwrap(),
        id: id(0x5000_u64.checked_add(n).unwrap()),
    }
}

fn draft(stream_n: u64, note: u64) -> EventDraft {
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

/// A directory of its own, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("keel-store-unit-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn db(&self) -> PathBuf {
        self.0.join("store.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type TestStore = Store<SoftwareSigner, SeededEntropy>;

/// A key for the tests' stores: its bytes step from `n`, so a key given in another order is
/// another key.
fn key_of(n: u8) -> StoreKey {
    StoreKey::new(&mut key_bytes(n))
}

/// The bytes of [`key_of`]`(n)`.
fn key_bytes(n: u8) -> [u8; 32] {
    let mut bytes = [n; 32];
    for (step, byte) in (0_u8..).zip(bytes.iter_mut()) {
        *byte = n.wrapping_add(step.wrapping_mul(37));
    }
    bytes
}

fn key() -> StoreKey {
    key_of(0x4B)
}

fn open(path: &Path) -> TestStore {
    Store::open(path, key(), config(), own().1, SeededEntropy::new(7)).unwrap()
}

/// A connection of its own to the database at `path`, with the key [`key_of`]`(n)`.
fn raw_with(path: &Path, n: u8) -> Connection {
    let db = Connection::open(path).unwrap();
    let hex: String = key_bytes(n).iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02X}");
        hex
    });
    db.execute_batch(&format!(
        "PRAGMA cipher_log_level = NONE; PRAGMA key = \"x'{hex}'\"; \
         PRAGMA cipher_compatibility = 4;"
    ))
    .unwrap();
    db
}

/// A connection of its own to the database at `path`, with the tests' key, to look at it or
/// change it behind the store's back.
fn raw(path: &Path) -> Connection {
    raw_with(path, 0x4B)
}

/// Every device in these tests, enrolled here, and device 9, enrolled elsewhere.
fn registry() -> DeviceRegistry {
    let mut registry = DeviceRegistry::new();
    for n in 1..=4 {
        let (device, signer) = device(n);
        registry.enroll(device, here(), signer.public_key().clone()).unwrap();
    }
    let (device, signer) = device(9);
    registry.enroll(device, elsewhere(), signer.public_key().clone()).unwrap();
    registry
}

/// Device `n`'s log, written at `location`, starting at 1 second past the base time.
fn log_of(n: u8, location: Id<Location>, count: usize) -> Vec<SignedEvent> {
    let (device, signer) = device(n);
    let config = LogConfig {
        device,
        location,
        head: LogHead::EMPTY,
        latest_hlc: Hlc::ZERO,
        max_forward_drift: Duration::from_secs(60),
    };
    let mut writer = LogWriter::new(config, signer, SeededEntropy::new(u64::from(n)));
    (0..count)
        .map(|k| {
            let k = i64::try_from(k).unwrap();
            let at = at(1_000_i64.checked_add(k).unwrap());
            writer.prepare(draft(1, u64::from(n)), at).unwrap().commit()
        })
        .collect()
}

fn append(store: &mut TestStore, drafts: Vec<EventDraft>, now: Timestamp) -> Vec<SignedEvent> {
    store.write(|w| drafts.into_iter().map(|draft| w.append(draft, now)).collect()).unwrap()
}

fn receive(store: &mut TestStore, bytes: &[u8]) -> Received {
    store.write(|w| w.receive(bytes, &registry(), at(5_000))).unwrap()
}

/// `event`'s body, changed by `change`, signed by device `n`'s key.
fn resigned(event: &SignedEvent, n: u8, change: impl FnOnce(&mut EventBody)) -> Vec<u8> {
    let mut body = event.body().clone();
    change(&mut body);
    SignedEvent::sign(body, &device(n).1).unwrap().to_bytes()
}

#[test]
fn a_new_store_runs_durably_and_holds_nothing() {
    let dir = TempDir::new();
    let store = open(&dir.db());
    let journal: String =
        store.db().query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
    let synchronous: i64 =
        store.db().query_row("PRAGMA synchronous", [], |row| row.get(0)).unwrap();
    let trusted: i64 = store.db().query_row("PRAGMA trusted_schema", [], |row| row.get(0)).unwrap();
    let version: i64 = store.db().query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!((journal.as_str(), synchronous, trusted, version), ("wal", 2, 0, 2));
    assert_eq!(store.device(), own().0);
    assert_eq!(store.location(), here());
    assert_eq!(store.head(own().0).unwrap(), LogHead::EMPTY);
    assert!(store.version_vector().unwrap().is_empty());
    assert!(store.quarantine().unwrap().is_empty());
    assert_eq!(store.clock(), Hlc::ZERO);
}

#[test]
fn a_row_that_doesnt_read_back_as_stored_is_corrupt() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let events = append(&mut store, vec![draft(1, 1), draft(1, 2)], at(0));
    // A stored hash that isn't the message's.
    let changed =
        store.db().execute("UPDATE events SET hash = zeroblob(32) WHERE origin_seq = 1", []);
    assert_eq!(changed.unwrap(), 1);
    assert!(matches!(store.log(own().0, 0, 10), Err(StoreError::Corrupt(_))));
    assert!(matches!(store.event(events[0].body().event_id), Err(StoreError::Corrupt(_))));
    assert!(matches!(store.stream(&stream(1)), Err(StoreError::Corrupt(_))));
    // The other event still reads back.
    assert_eq!(store.log(own().0, 1, 10).unwrap(), events[1..]);
    // A message that isn't an event.
    let changed = store.db().execute("UPDATE events SET message = x'00' WHERE origin_seq = 2", []);
    assert_eq!(changed.unwrap(), 1);
    assert!(matches!(store.log(own().0, 1, 10), Err(StoreError::Corrupt(_))));
}

#[test]
fn appended_events_chain_and_read_back() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let events = append(&mut store, vec![draft(1, 1), draft(2, 2), draft(1, 3)], at(0));
    let seqs: Vec<u64> = events.iter().map(|event| event.body().origin_seq.get()).collect();
    assert_eq!(seqs, [1, 2, 3]);
    assert_eq!(events[1].body().prev_hash, events[0].hash());
    assert_eq!(store.head(own().0).unwrap(), LogHead::of(&events[2]));
    assert_eq!(store.version_vector().unwrap().into_iter().collect::<Vec<_>>(), [(own().0, 3)]);
    assert_eq!(store.log(own().0, 0, 10).unwrap(), events);
    assert_eq!(store.log(own().0, 1, 1).unwrap(), &events[1..2]);
    assert_eq!(store.log(own().0, 3, 10).unwrap(), []);
    assert_eq!(store.log(own().0, u64::MAX, 10).unwrap(), []);
    assert_eq!(store.stream(&stream(1)).unwrap(), [events[0].clone(), events[2].clone()]);
    assert_eq!(store.event(events[1].body().event_id).unwrap(), Some(events[1].clone()));
    assert_eq!(store.event(id(0xDEAD)).unwrap(), None);
    // Every stored event verifies with the device's key.
    for event in store.log(own().0, 0, 10).unwrap() {
        assert_eq!(registry().verify(&event.to_bytes()), Ok(event));
    }
    assert_eq!(store.clock(), events[2].body().hlc);
}

#[test]
fn a_failed_write_stores_nothing_and_rewinds_the_writer() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let first = append(&mut store, vec![draft(1, 1)], at(0));
    let failed: Result<(), StoreError> = store.write(|w| {
        w.append(draft(1, 2), at(1))?;
        w.append(draft(1, 3), at(2))?;
        Err(StoreError::Corrupt("the caller changed its mind"))
    });
    assert!(failed.is_err());
    assert_eq!(store.log(own().0, 0, 10).unwrap(), first);
    assert_eq!(store.head(own().0).unwrap(), LogHead::of(&first[0]));
    // The writer continues the stored log, not the lost events.
    let next = append(&mut store, vec![draft(1, 4)], at(3));
    assert_eq!(next[0].body().origin_seq.get(), 2);
    assert_eq!(next[0].body().prev_hash, first[0].hash());
}

#[test]
fn a_reopened_store_continues_its_log_and_clock() {
    let dir = TempDir::new();
    let (first, clock) = {
        let mut store = open(&dir.db());
        let first = append(&mut store, vec![draft(1, 1), draft(1, 2)], at(0));
        // A received event far ahead moves the clock, within the drift limit.
        let theirs = log_of(2, here(), 1);
        let later = resigned(&theirs[0], 2, |body| {
            body.hlc = Hlc::new(u64::try_from(at(50_000).as_millis()).unwrap(), 3).unwrap();
        });
        assert!(matches!(receive(&mut store, &later), Received::Stored(_)));
        (first, store.clock())
    };
    // A restarted device draws fresh entropy: the same seed at the same time would mint the
    // same event identifiers, which the store refuses to store twice.
    let mut store = Store::open(dir.db(), key(), config(), own().1, SeededEntropy::new(8)).unwrap();
    assert_eq!(store.clock(), clock);
    assert_eq!(store.head(own().0).unwrap(), LogHead::of(&first[1]));
    let next = append(&mut store, vec![draft(1, 3)], at(0));
    assert_eq!(LogHead::of(&first[1]).link(&next[0]), Ok(keel_events::log::Link::Next));
    assert!(next[0].body().hlc > clock, "the device's next event follows what it received");
}

#[test]
fn received_events_extend_their_devices_logs() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let theirs = log_of(2, here(), 3);
    assert_eq!(
        receive(&mut store, &theirs[0].to_bytes()),
        Received::Stored(Box::new(theirs[0].clone()))
    );
    assert_eq!(receive(&mut store, &theirs[0].to_bytes()), Received::Duplicate);
    assert_eq!(receive(&mut store, &theirs[2].to_bytes()), Received::Gap { head: 1 });
    assert_eq!(
        receive(&mut store, &theirs[1].to_bytes()),
        Received::Stored(Box::new(theirs[1].clone()))
    );
    // An earlier event, received again.
    assert_eq!(receive(&mut store, &theirs[0].to_bytes()), Received::Duplicate);
    assert_eq!(store.log(device(2).0, 0, 10).unwrap(), &theirs[..2]);
    assert_eq!(store.version_vector().unwrap().into_iter().collect::<Vec<_>>(), [(device(2).0, 2)]);
    assert!(store.quarantine().unwrap().is_empty());
    // The device's clock observed them.
    assert!(store.clock() > theirs[1].body().hlc);
}

#[test]
fn refused_events_are_quarantined_once_and_the_first_event_stays() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let theirs = log_of(2, here(), 2);
    receive(&mut store, &theirs[0].to_bytes());
    receive(&mut store, &theirs[1].to_bytes());
    let fork =
        resigned(&theirs[1], 2, |body| body.payload = Payload::new(&Value::Unsigned(9)).unwrap());
    let early_fork =
        resigned(&theirs[0], 2, |body| body.payload = Payload::new(&Value::Unsigned(8)).unwrap());
    let next = log_of(2, here(), 3).remove(2);
    let broken = resigned(&next, 2, |body| body.prev_hash = theirs[0].hash());
    let regressed = resigned(&next, 2, |body| body.hlc = theirs[1].body().hlc);
    let unsigned = resigned(&next, 3, |_| {});
    let unknown = resigned(&next, 5, |body| body.origin_device = id(5));
    let misplaced = resigned(&next, 2, |body| body.location = elsewhere());
    let other_location = log_of(9, elsewhere(), 1).remove(0).to_bytes();
    let duplicate_id = resigned(&next, 2, |body| body.event_id = theirs[0].body().event_id);
    let out_of_range =
        resigned(&next, 2, |body| body.origin_seq = NonZeroU64::new(1 << 63).unwrap());
    let cases: [(&[u8], Reason); 11] = [
        (&fork, Reason::Fork),
        (&early_fork, Reason::Fork),
        (&broken, Reason::BrokenLink),
        (&regressed, Reason::ClockRegressed),
        (&unsigned, Reason::BadSignature),
        (&unknown, Reason::UnknownDevice),
        (&misplaced, Reason::WrongLocation),
        (&other_location, Reason::OtherLocation),
        (&duplicate_id, Reason::DuplicateId),
        (&out_of_range, Reason::OutOfRange),
        (b"not an event", Reason::Malformed),
    ];
    for (bytes, reason) in cases {
        assert_eq!(receive(&mut store, bytes), Received::Quarantined(reason), "{reason:?}");
        // Twice: still kept once.
        assert_eq!(receive(&mut store, bytes), Received::Quarantined(reason), "{reason:?}");
    }
    let kept: Vec<(Reason, Vec<u8>)> = store
        .quarantine()
        .unwrap()
        .into_iter()
        .map(|quarantined| (quarantined.reason, quarantined.message))
        .collect();
    let expected: Vec<(Reason, Vec<u8>)> =
        cases.iter().map(|(bytes, reason)| (*reason, bytes.to_vec())).collect();
    assert_eq!(kept, expected);
    // The first events stay, and the log goes on.
    assert_eq!(store.log(device(2).0, 0, 10).unwrap(), theirs);
    assert!(matches!(receive(&mut store, &next.to_bytes()), Received::Stored(_)));
}

#[test]
fn a_revoked_devices_later_events_are_quarantined() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let theirs = log_of(2, here(), 3);
    let mut registry = registry();
    let revocation = Revocation { after_seq: 1, last_trusted: theirs[0].hash() };
    registry.revoke(device(2).0, revocation).unwrap();
    let received = store
        .write(|w| {
            theirs
                .iter()
                .map(|event| w.receive(&event.to_bytes(), &registry, at(5_000)))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap();
    assert_eq!(
        received,
        [
            Received::Stored(Box::new(theirs[0].clone())),
            Received::Quarantined(Reason::Revoked),
            Received::Quarantined(Reason::Revoked)
        ]
    );
}

#[test]
fn the_devices_own_events_from_elsewhere_move_its_writer_on() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let mine = append(&mut store, vec![draft(1, 1)], at(0));
    // The same device, restored from a backup elsewhere, wrote on: its events come back.
    let config = LogConfig {
        device: own().0,
        location: here(),
        head: LogHead::of(&mine[0]),
        latest_hlc: Hlc::ZERO,
        max_forward_drift: Duration::from_secs(60),
    };
    let mut twin = LogWriter::new(config, own().1, SeededEntropy::new(99));
    let far = at(3_600_000);
    let twins: Vec<SignedEvent> =
        (0..2).map(|k| twin.prepare(draft(3, k), far).unwrap().commit()).collect();
    for event in &twins {
        assert!(matches!(receive(&mut store, &event.to_bytes()), Received::Stored(_)));
    }
    let next = append(&mut store, vec![draft(3, 9)], at(1));
    assert_eq!(next[0].body().origin_seq.get(), 4);
    assert_eq!(next[0].body().prev_hash, twins[1].hash());
    assert!(next[0].body().hlc > twins[1].body().hlc);
}

#[test]
fn streams_read_in_canonical_order() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    // Device 2's first event is on stream 1, at 1 second; the store's own come later in time,
    // but arrive first.
    let mine = append(&mut store, vec![draft(1, 1)], at(2_000));
    let theirs = log_of(2, here(), 2);
    receive(&mut store, &theirs[0].to_bytes());
    receive(&mut store, &theirs[1].to_bytes());
    let order: Vec<SignedEvent> = store.stream(&stream(1)).unwrap();
    assert_eq!(order, [theirs[0].clone(), theirs[1].clone(), mine[0].clone()]);
    assert!(store.stream(&stream(7)).unwrap().is_empty());
}

#[test]
fn a_store_opens_only_for_its_device_location_and_key() {
    let dir = TempDir::new();
    append(&mut open(&dir.db()), vec![draft(1, 1)], at(0));
    let other_device = StoreConfig { device: device(2).0, ..config() };
    let other_location = StoreConfig { location: elsewhere(), ..config() };
    for config in [other_device, other_location] {
        let opened = Store::open(dir.db(), key(), config, own().1, SeededEntropy::new(1));
        assert!(matches!(opened, Err(StoreError::NotThisDevice)), "{config:?}");
    }
    let wrong_signer = Store::open(dir.db(), key(), config(), device(3).1, SeededEntropy::new(1));
    assert!(matches!(wrong_signer, Err(StoreError::WrongSigner)));
    // A newer kernel's database is refused.
    open(&dir.db()).db().execute_batch("PRAGMA user_version = 3").unwrap();
    let newer = Store::open(dir.db(), key(), config(), own().1, SeededEntropy::new(1));
    assert!(matches!(newer, Err(StoreError::NewerSchema { found: 3, known: 2 })));
}

/// Refuses at the `nth` time it reaches `point`, counting from 1.
struct RefuseAt {
    point: Point,
    nth: u32,
    seen: u32,
}

impl Faults for RefuseAt {
    fn proceed(&mut self, point: Point) -> bool {
        if point == self.point {
            self.seen = self.seen.checked_add(1).unwrap();
            return self.seen != self.nth;
        }
        true
    }
}

#[test]
fn an_interrupted_write_rolls_back_but_a_committed_one_stands() {
    for (point, nth) in [
        (Point::Began, 2),
        (Point::Stored, 2),
        (Point::Stored, 3),
        (Point::Committing, 2),
        (Point::Committed, 2),
    ] {
        let dir = TempDir::new();
        let faults = Box::new(RefuseAt { point, nth, seen: 0 });
        let mut store = Store::open_with_faults(
            dir.db(),
            key(),
            config(),
            own().1,
            SeededEntropy::new(7),
            faults,
        )
        .unwrap();
        let first = append(&mut store, vec![draft(1, 1)], at(0));
        let theirs = log_of(2, here(), 1);
        let written: Result<SignedEvent, StoreError> = store.write(|w| {
            let event = w.append(draft(1, 2), at(1))?;
            w.receive(&theirs[0].to_bytes(), &registry(), at(1))?;
            Ok(event)
        });
        let stored = store.log(own().0, 0, 10).unwrap();
        if point == Point::Committed {
            let event = written.unwrap();
            assert_eq!(stored, [first[0].clone(), event]);
            assert_eq!(store.log(device(2).0, 0, 10).unwrap(), theirs);
        } else {
            assert!(matches!(written, Err(StoreError::Interrupted(p)) if p == point));
            assert_eq!(stored, first, "{point:?} {nth}");
            assert!(store.log(device(2).0, 0, 10).unwrap().is_empty());
        }
        // Either way, the writer continues the stored log.
        let next = append(&mut store, vec![draft(1, 3)], at(2));
        assert_eq!(next[0].body().prev_hash, stored.last().unwrap().hash());
    }
}

/// Found by the property test: a write that received the device's own event from an hour
/// ahead, and was then interrupted, left the device's clock an hour ahead.
#[test]
fn a_failed_write_gives_back_the_clock_it_moved() {
    let dir = TempDir::new();
    let faults = Box::new(RefuseAt { point: Point::Stored, nth: 1, seen: 0 });
    let mut store =
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(7), faults)
            .unwrap();
    // The device's own first event, written elsewhere an hour ahead.
    let config = LogConfig {
        device: own().0,
        location: here(),
        head: LogHead::EMPTY,
        latest_hlc: Hlc::ZERO,
        max_forward_drift: Duration::from_secs(60),
    };
    let mut twin = LogWriter::new(config, own().1, SeededEntropy::new(99));
    let ahead = twin.prepare(draft(1, 1), at(3_600_000)).unwrap().commit();
    let interrupted = store.write(|w| w.receive(&ahead.to_bytes(), &registry(), at(0)));
    assert!(matches!(interrupted, Err(StoreError::Interrupted(Point::Stored))));
    assert_eq!(store.clock(), Hlc::ZERO);
    // The next event is the device's first, at physical time.
    let next = append(&mut store, vec![draft(1, 2)], at(1));
    assert_eq!(next[0].body().origin_seq.get(), 1);
    assert_eq!(next[0].body().hlc.wall_ms(), u64::try_from(at(1).as_millis()).unwrap());
}

#[test]
fn an_interrupted_migration_leaves_no_store() {
    let dir = TempDir::new();
    let faults = Box::new(RefuseAt { point: Point::Migrating, nth: 1, seen: 0 });
    let opened =
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(7), faults);
    assert!(matches!(opened, Err(StoreError::Interrupted(Point::Migrating))));
    // Nothing of the schema was kept.
    let db = raw(&dir.db());
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    let tables: i64 =
        db.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0)).unwrap();
    assert_eq!((version, tables), (0, 0));
    drop(db);
    // Opening again creates it.
    let store = open(&dir.db());
    assert_eq!(store.head(own().0).unwrap(), LogHead::EMPTY);
}

#[test]
fn reasons_have_distinct_codes() {
    for reason in Reason::ALL {
        assert_eq!(Reason::from_code(reason.code()), Some(reason));
    }
    let mut codes: Vec<&str> = Reason::ALL.iter().map(|reason| reason.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), Reason::ALL.len());
    assert_eq!(Reason::from_code("nonsense"), None);
}
