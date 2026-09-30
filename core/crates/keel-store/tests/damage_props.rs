//! Property tests for damage and the full check (ADR-0018).
//!
//! A case stocks a store with a few random writes: the device's own events, some big enough to
//! spill onto pages of their own, on order and payment streams; two other devices' events; a
//! malformed message or two; and effects, enqueued, started and finished. Then it damages the
//! store in one of two ways:
//!
//! - **the file**, as a failing disk, or someone without the key, would: bits flipped anywhere in
//!   it, and bytes added to its end. The store must never return what was damaged: every read
//!   gives what the sound store gave, or `Damaged`, and `Damaged` from then on. The check must
//!   report exactly the pages damaged, and a damaged first page must refuse the key;
//! - **the rows**, as a bug would, with the key: events changed, filed wrongly or deleted,
//!   projection rows changed, added or deleted, effects' causes and attempts changed, the clock
//!   put back, quarantined messages misfiled, and the store's identity changed. A model says
//!   which problems the check must report, and it must report exactly those.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use std::collections::{BTreeMap, BTreeSet};

use keel_domain::order::Order;
use keel_domain::payment::Payment;
use keel_events::cbor::Value;
use keel_events::envelope::{Aggregate, Device, Event, Payload, SchemaName, StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::SoftwareSigner;
use keel_events::log::{EventDraft, LogHead};
use keel_store::{Effect, EffectKind, EffectState, OrderState, Problem, Queued, Store, StoreError};
use keel_types::{Id, SeededEntropy};
use proptest::prelude::*;
use proptest::sample::Index;
use rusqlite::params;
use support::{
    OWN, PEERS, Scratch, at, config, device, draft, elsewhere, here, id, next_event, raw, registry,
    resigned, signer, store_key,
};

type TestStore = Store<SoftwareSigner, SeededEntropy>;

// ---------------------------------------------------------------------------------------------
// Stocking a store.

/// One of the device's own events.
#[derive(Clone, Copy, Debug)]
struct Note {
    /// On a payment's stream, or else an order's.
    payment: bool,
    stream: u8,
    /// Big enough to spill onto pages of its own.
    big: bool,
}

#[derive(Clone, Debug)]
struct WriteSpec {
    notes: Vec<Note>,
    /// How many of each other device's next events to receive.
    peers: [u8; 2],
    malformed: bool,
    /// Whether to enqueue an effect, and the event that caused it, one of those stored so far.
    enqueue: bool,
    cause: Option<Index>,
    /// Effects to start and to finish, if they can be.
    start: Option<Index>,
    finish: Option<Index>,
}

fn any_note() -> impl Strategy<Value = Note> {
    (prop::bool::weighted(0.3), 1_u8..4, prop::bool::weighted(0.4))
        .prop_map(|(payment, stream, big)| Note { payment, stream, big })
}

fn any_write() -> impl Strategy<Value = WriteSpec> {
    (
        prop::collection::vec(any_note(), 0..4),
        [0_u8..3, 0_u8..3],
        prop::bool::weighted(0.35),
        prop::bool::weighted(0.6),
        prop::option::weighted(0.8, any::<Index>()),
        prop::option::of(any::<Index>()),
        prop::option::of(any::<Index>()),
    )
        .prop_map(|(notes, peers, malformed, enqueue, cause, start, finish)| WriteSpec {
            notes,
            peers,
            malformed,
            enqueue,
            cause,
            start,
            finish,
        })
}

/// The device's own event `note`, the `k`-th.
fn note_draft(note: Note, k: u64) -> EventDraft {
    let kind = if note.payment { "payment" } else { "order" };
    // Three pages of its own, and a little more.
    let size = if note.big { 12_500 } else { 8 };
    EventDraft {
        stream: StreamRef {
            kind: StreamKind::new(kind).unwrap(),
            id: id(0x5000 + u64::from(note.stream) + if note.payment { 0x100 } else { 0 }),
        },
        schema: keel_events::envelope::SchemaRef {
            name: SchemaName::new(&format!("{kind}.noted")).unwrap(),
            version: core::num::NonZeroU32::MIN,
        },
        payload: Payload::new(&Value::Bytes(vec![u8::try_from(k % 251).unwrap(); size])).unwrap(),
        ..draft(1, k)
    }
}

/// Stocks the store in `scratch` with `writes`, and returns it open.
fn stock(scratch: &Scratch, writes: &[WriteSpec]) -> TestStore {
    let mut store =
        Store::open(scratch.db(), store_key(), config(), signer(OWN), SeededEntropy::new(1))
            .unwrap();
    let registry = registry();
    let mut heads = [LogHead::EMPTY; 2];
    let mut events: Vec<Id<Event>> = Vec::new();
    let mut effects = 0_u64;
    let mut k = 0_u64;
    for (i, spec) in writes.iter().enumerate() {
        let now = at(10_000 + i64::try_from(i).unwrap() * 100);
        store
            .write(|w| {
                for note in &spec.notes {
                    k += 1;
                    events.push(w.append(note_draft(*note, k), now)?.body().event_id);
                }
                for (p, count) in spec.peers.iter().enumerate() {
                    for _ in 0..*count {
                        let n = PEERS[p];
                        let at = at(1_000 + i64::try_from(heads[p].seq()).unwrap());
                        let event =
                            next_event(n, here(), heads[p], at, 1 + u8::try_from(p).unwrap(), 0);
                        heads[p] = LogHead::of(&event);
                        w.receive(&event.to_bytes(), &registry, now)?;
                        events.push(event.body().event_id);
                    }
                }
                if spec.malformed {
                    w.receive(format!("malformed {i}").as_bytes(), &registry, now)?;
                }
                if spec.enqueue {
                    let cause =
                        spec.cause.as_ref().filter(|_| !events.is_empty()).map(|c| *c.get(&events));
                    let key = format!("effect {effects}").into_bytes();
                    effects += 1;
                    let effect = Effect {
                        key: key.clone(),
                        kind: EffectKind::new("print.kitchen").unwrap(),
                        payload: key,
                        cause,
                    };
                    w.enqueue(&effect, now)?;
                }
                // Starting or finishing an effect that can't be refuses, which changes nothing.
                if let Some(pick) = spec.start.filter(|_| effects > 0) {
                    let key = format!("effect {}", pick.index(usize::try_from(effects).unwrap()));
                    let _ = w.start(key.as_bytes(), now);
                }
                if let Some(pick) = spec.finish.filter(|_| effects > 0) {
                    let key = format!("effect {}", pick.index(usize::try_from(effects).unwrap()));
                    let _ = w.finish(key.as_bytes());
                }
                Ok::<(), StoreError>(())
            })
            .unwrap();
    }
    store
}

/// A stored event, as its row files it.
#[derive(Clone, Debug)]
struct Stored {
    row: i64,
    device: Vec<u8>,
    seq: u64,
    id: Vec<u8>,
    kind: String,
    stream: Vec<u8>,
}

/// Every stored event, by device and position.
fn stored_events(scratch: &Scratch) -> Vec<Stored> {
    let db = raw(&scratch.db());
    let mut statement = db
        .prepare(
            "SELECT arrival, origin_device, origin_seq, event_id, stream_kind, stream_id \
             FROM events ORDER BY origin_device, origin_seq",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok(Stored {
                row: row.get(0)?,
                device: row.get(1)?,
                seq: u64::try_from(row.get::<_, i64>(2)?).unwrap(),
                id: row.get(3)?,
                kind: row.get(4)?,
                stream: row.get(5)?,
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Damage to the file.

#[derive(Clone, Debug)]
struct FileDamage {
    /// Bits flipped: where, and which.
    flips: Vec<(Index, u8)>,
    /// Bytes added to the end.
    added: usize,
    /// Bytes cut from the end.
    cut: usize,
    /// Where the reads start, so that each read can be the first to meet the damage.
    first_read: Index,
}

fn any_file_damage() -> impl Strategy<Value = FileDamage> {
    (
        prop::collection::vec((any::<Index>(), 1_u8..=255), 0..4),
        prop_oneof![
            4 => Just((0_usize, 0_usize)),
            1 => (1_usize..9_000).prop_map(|added| (added, 0)),
            1 => (1_usize..9_000).prop_map(|cut| (0, cut)),
        ],
        any::<Index>(),
    )
        .prop_map(|(flips, (added, cut), first_read)| FileDamage {
            flips,
            added,
            cut,
            first_read,
        })
}

/// A read of the store.
#[derive(Clone, Debug)]
enum Read {
    Log(u8),
    Head(u8),
    VersionVector,
    Event(Id<Event>),
    Stream(StreamRef),
    Orders(OrderState),
    Effects,
    Running,
    Due,
    Quarantine,
    /// A write that loads a stream's aggregate, then fails on purpose, so it changes nothing: a
    /// write can meet the damage first too.
    Load(StreamRef),
}

/// Why a write that loads an aggregate failed: on purpose, with what it loaded, or not.
enum Loaded {
    Aggregate(String),
    Store(StoreError),
}

impl From<StoreError> for Loaded {
    fn from(error: StoreError) -> Loaded {
        Loaded::Store(error)
    }
}

/// Every read of what the store holds.
fn every_read(events: &[Stored]) -> Vec<Read> {
    let mut reads = Vec::new();
    for n in [OWN, PEERS[0], PEERS[1]] {
        reads.extend([Read::Log(n), Read::Head(n)]);
    }
    reads.push(Read::VersionVector);
    for event in events {
        reads.push(Read::Event(Id::from_bytes(event.id.clone().try_into().unwrap()).unwrap()));
        let id = Id::from_bytes(event.stream.clone().try_into().unwrap()).unwrap();
        reads.push(Read::Stream(StreamRef { kind: StreamKind::new(&event.kind).unwrap(), id }));
    }
    reads.extend(OrderState::ALL.map(Read::Orders));
    reads.extend([Read::Effects, Read::Running, Read::Due, Read::Quarantine]);
    let streams: BTreeSet<(String, Vec<u8>)> =
        events.iter().map(|event| (event.kind.clone(), event.stream.clone())).collect();
    for (kind, id) in streams {
        let id = Id::from_bytes(id.try_into().unwrap()).unwrap();
        reads.push(Read::Load(StreamRef { kind: StreamKind::new(&kind).unwrap(), id }));
    }
    reads
}

/// What `read` gives, as text.
fn read(store: &mut TestStore, read: &Read) -> Result<String, String> {
    fn text<T: core::fmt::Debug>(read: Result<T, StoreError>) -> Result<String, String> {
        read.map(|value| format!("{value:?}")).map_err(|error| format!("{error:?}"))
    }
    match read {
        Read::Log(n) => text(store.log(device(*n), 0, 1_000)),
        Read::Head(n) => text(store.head(device(*n))),
        Read::VersionVector => text(store.version_vector()),
        Read::Event(id) => text(store.event(*id)),
        Read::Stream(stream) => text(store.stream(stream)),
        Read::Orders(state) => text(store.orders(*state)),
        Read::Effects => text(store.effects()),
        Read::Running => text(store.running_effects()),
        Read::Due => text(store.due_effects(at(1_000_000), 100)),
        Read::Quarantine => text(store.quarantine()),
        Read::Load(stream) => {
            let loaded: Result<(), Loaded> = store.write(|w| {
                let aggregate = if stream.kind.as_str() == "payment" {
                    format!("{:?}", w.load(Payment::new(stream.id.cast()))?)
                } else {
                    format!("{:?}", w.load(Order::new(stream.id.cast()))?)
                };
                Err(Loaded::Aggregate(aggregate))
            });
            match loaded {
                Err(Loaded::Aggregate(aggregate)) => Ok(aggregate),
                Err(Loaded::Store(error)) => Err(format!("{error:?}")),
                Ok(()) => panic!("the write fails on purpose"),
            }
        }
    }
}

proptest! {
    /// Damage to the file is never silent: every read gives what the sound store gave, or
    /// `Damaged`, and `Damaged` from then on; and the check reports exactly the damaged pages.
    #[test]
    fn damage_to_the_file_is_never_silent(
        writes in prop::collection::vec(any_write(), 1..6),
        damage in any_file_damage(),
    ) {
        let scratch = Scratch::new("file-damage");
        let mut store = stock(&scratch, &writes);
        let reads = every_read(&stored_events(&scratch));
        let sound: Vec<Result<String, String>> = reads.iter().map(|each| read(&mut store, each)).collect();
        drop(store);
        // Damage the file: the net change at each byte, then bytes added or cut.
        let mut file = std::fs::read(scratch.db()).unwrap();
        let length = file.len();
        let kept = length.saturating_sub(damage.cut);
        let mut changes: BTreeMap<usize, u8> = BTreeMap::new();
        for (at, bits) in &damage.flips {
            *changes.entry(at.index(length)).or_default() ^= bits;
        }
        let mut damaged: BTreeSet<u32> = BTreeSet::new();
        for (&offset, &bits) in &changes {
            file[offset] ^= bits;
            if bits != 0 && offset < kept {
                damaged.insert(u32::try_from(offset / 4096 + 1).unwrap());
            }
        }
        file.extend((0..damage.added).map(|n| u8::try_from(n % 251).unwrap() ^ 0xA5));
        file.truncate(kept + damage.added);
        let pages = |bytes: usize| u32::try_from(bytes.div_ceil(4096)).unwrap();
        // Pages added, the last of them perhaps in part; or the last page cut short.
        damaged.extend(pages(length) + 1..=pages(length + damage.added));
        if damage.cut > 0 && kept % 4096 != 0 {
            damaged.insert(pages(kept));
        }
        // A whole page cut off: the file is shorter than its header says.
        let lost = pages(kept) < pages(length);
        std::fs::write(scratch.db(), &file).unwrap();
        let opened = Store::open(scratch.db(), store_key(), config(), signer(OWN), SeededEntropy::new(2));
        let mut store = match opened {
            // A damaged first page is a key that doesn't open the store.
            Err(StoreError::KeyRejected) => {
                prop_assert!(damaged.contains(&1) || kept < 4096, "refused, with {damaged:?} damaged");
                return Ok(());
            }
            // Opening read a damaged page, or found the file short.
            Err(StoreError::Damaged) => {
                prop_assert!((lost || !damaged.is_empty()) && !damaged.contains(&1), "{damaged:?}");
                return Ok(());
            }
            Err(error) => panic!("opening: {error:?}"),
            Ok(store) => store,
        };
        prop_assert!(!damaged.contains(&1) && !lost);
        let start = damage.first_read.index(reads.len());
        let mut met: Option<&Read> = None;
        for (each, before) in reads.iter().zip(&sound).skip(start).chain(reads.iter().zip(&sound).take(start)) {
            match (met, read(&mut store, each)) {
                (None, now) if now == *before => {}
                (_, Err(error)) if error == "Damaged" => met = met.or(Some(each)),
                (met, now) => prop_assert!(false, "{each:?}: {now:?}, where the sound store gave {before:?}, after damage met at {met:?}"),
            }
        }
        let found = store.check().unwrap();
        let expected: Vec<Problem> = damaged.iter().map(|&page| Problem::Page(page)).collect();
        prop_assert_eq!(found, expected);
        // A write goes through, or meets the damage; after damage, nothing does.
        let written = store.write(|w| w.append(draft(1, 9_999), at(9_000_000)));
        match (met, written) {
            (None, Ok(_)) | (_, Err(StoreError::Damaged)) => {}
            (met, written) => prop_assert!(false, "a write: {written:?}, after damage met at {met:?}"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Changes behind the store's back.

/// A change a bug could make, with the key, to what an index picks.
#[derive(Clone, Copy, Debug)]
enum Change {
    /// An event's bytes, so it doesn't decode or hash to its stored hash.
    Bytes(Index),
    /// An event's stored hash.
    Hash(Index),
    /// The identifier an event is filed under.
    Refile(Index),
    /// An event, deleted.
    Delete(Index),
    /// Every event of a stream, deleted, and the stream's projection row with them.
    DropStream(Index),
    /// An event replaced with a forgery: signed by its device, filed as it was, but with another
    /// payload, so the event after it no longer links to it.
    Forge(Index),
    /// A projection row's count of events.
    Row(Index),
    /// A projection row, deleted.
    RowDeleted(Index),
    /// A projection row for a stream with no events.
    RowAdded(u8),
    /// An effect's cause, to an event the store doesn't hold.
    Cause(Index),
    /// An effect's attempts, so they disagree with its start time.
    Attempts(Index),
    /// The device's clock, put back to zero.
    Clock,
    /// A quarantined message's digest.
    Digest(Index),
    /// The store's location.
    Identity,
}

fn any_change() -> impl Strategy<Value = Change> {
    let i = any::<Index>;
    prop_oneof![
        2 => i().prop_map(Change::Bytes),
        1 => i().prop_map(Change::Hash),
        1 => i().prop_map(Change::Refile),
        3 => i().prop_map(Change::Delete),
        1 => i().prop_map(Change::DropStream),
        2 => i().prop_map(Change::Forge),
        2 => i().prop_map(Change::Row),
        1 => i().prop_map(Change::RowDeleted),
        1 => (0_u8..3).prop_map(Change::RowAdded),
        1 => i().prop_map(Change::Cause),
        1 => i().prop_map(Change::Attempts),
        1 => Just(Change::Clock),
        2 => i().prop_map(Change::Digest),
        1 => Just(Change::Identity),
    ]
}

/// What the model knows of the store, and what the changes made so far did to it.
#[derive(Debug, Default)]
struct Model {
    events: Vec<Stored>,
    /// Events that no longer read back: their rows.
    unreadable: BTreeSet<i64>,
    /// Events filed wrongly: their rows.
    misfiled: BTreeSet<i64>,
    /// Events replaced with forgeries: their rows.
    forged: BTreeSet<i64>,
    /// Events deleted: their rows.
    deleted: BTreeSet<i64>,
    /// Projection rows changed, deleted or added: by projection and stream, how each is now.
    rows: BTreeMap<(&'static str, Vec<u8>), RowState>,
    effects: Vec<Queued>,
    /// Effects changed: their keys.
    effects_changed: BTreeSet<Vec<u8>>,
    clock: bool,
    quarantine: Vec<i64>,
    digests: BTreeSet<i64>,
    identity: bool,
}

/// How a projection row was changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowState {
    /// Its count of events is one more than it was.
    Changed,
    Deleted,
    /// Added, for a stream with no events.
    Added,
}

/// The projection a stream of `kind` is in.
fn projection_of(kind: &str) -> &'static str {
    if kind == "payment" { "payments" } else { "orders" }
}

impl Model {
    /// The events that are still stored.
    fn present(&self) -> impl Iterator<Item = &Stored> {
        self.events.iter().filter(|event| !self.deleted.contains(&event.row))
    }

    /// The problems the check must find.
    fn problems(&self) -> BTreeSet<String> {
        let mut problems = BTreeSet::new();
        let mut add = |problem: Problem| problems.insert(format!("{problem:?}"));
        // Events, and each device's log: a break wherever the event before one is missing, or
        // is a forgery the event doesn't link to. A link is checked only between events that
        // read back and are filed as their bodies say.
        let good = |event: &Stored| {
            !self.unreadable.contains(&event.row) && !self.misfiled.contains(&event.row)
        };
        let mut last: Option<&Stored> = None;
        for event in self.present() {
            let previous = last.filter(|last| last.device == event.device);
            let device: Id<Device> =
                Id::from_bytes(event.device.clone().try_into().unwrap()).unwrap();
            if event.seq != previous.map_or(0, |last| last.seq) + 1 {
                add(Problem::Chain { device, seq: event.seq });
            } else if let Some(previous) = previous
                && good(event)
                && good(previous)
                && self.forged.contains(&previous.row)
            {
                add(Problem::Chain { device, seq: event.seq });
            }
            if !good(event) {
                add(Problem::Event { row: event.row });
            }
            last = Some(event);
        }
        if self.identity {
            add(Problem::Identity);
        }
        let own = device(OWN).to_bytes();
        if self.clock && self.present().any(|event| event.device[..] == own[..]) {
            add(Problem::Clock);
        }
        // A projection's row is what the stream's events make, or none if it has none left: a
        // row changed, deleted or added, or a stream that lost events, differs. It is judged
        // unless its stream has an event that doesn't read back.
        let mut streams: BTreeSet<(&'static str, Vec<u8>)> = self.rows.keys().cloned().collect();
        for event in &self.events {
            if self.deleted.contains(&event.row) {
                streams.insert((projection_of(&event.kind), event.stream.clone()));
            }
        }
        for (projection, stream) in streams {
            let of_stream = |event: &&Stored| {
                projection_of(&event.kind) == projection && event.stream == stream
            };
            let lost =
                self.events.iter().filter(of_stream).any(|event| self.deleted.contains(&event.row));
            let left = self.present().filter(of_stream).count();
            let unjudged =
                self.present().filter(of_stream).any(|event| self.unreadable.contains(&event.row));
            let differs = match self.rows.get(&(projection, stream.clone())) {
                Some(RowState::Changed | RowState::Added) => true,
                Some(RowState::Deleted) => left > 0,
                None => lost,
            };
            if differs && !unjudged {
                add(Problem::Projection { projection, stream });
            }
        }
        // An effect whose cause no longer has a row under its identifier.
        for queued in &self.effects {
            let caused = queued.effect.cause.is_none_or(|cause| {
                self.present().any(|event| {
                    event.id[..] == cause.to_bytes()[..] && !self.misfiled.contains(&event.row)
                })
            });
            if !caused || self.effects_changed.contains(&queued.effect.key) {
                add(Problem::Effect { key: queued.effect.key.clone() });
            }
        }
        for row in &self.digests {
            add(Problem::Quarantined { row: *row });
        }
        problems
    }
}

/// Makes `change`, to the stored event `index` picks, through `db`, and records it in `model`.
fn change_event(db: &rusqlite::Connection, model: &mut Model, change: Change, index: Index) {
    let present: Vec<&Stored> =
        model.events.iter().filter(|event| !model.deleted.contains(&event.row)).collect();
    if present.is_empty() {
        return;
    }
    let picked = present[index.index(present.len())].clone();
    let row = picked.row;
    match change {
        Change::Bytes(_) => {
            db.execute("UPDATE events SET message = substr(message, 2) WHERE arrival = ?1", [row])
                .unwrap();
            model.unreadable.insert(row);
        }
        Change::Hash(_) => {
            db.execute("UPDATE events SET hash = zeroblob(32) WHERE arrival = ?1", [row]).unwrap();
            model.unreadable.insert(row);
        }
        Change::Refile(_) => {
            let other = id::<Event>(0xF_0000 + u64::try_from(row).unwrap()).to_bytes();
            db.execute(
                "UPDATE events SET event_id = ?1 WHERE arrival = ?2",
                params![&other[..], row],
            )
            .unwrap();
            model.misfiled.insert(row);
        }
        Change::Forge(_) => {
            let message: Vec<u8> = db
                .query_row("SELECT message FROM events WHERE arrival = ?1", [row], |r| r.get(0))
                .unwrap();
            // An event whose bytes were already changed can't be forged from.
            let Ok(event) = SignedEvent::from_stored(&message) else { return };
            let n = [OWN, PEERS[0], PEERS[1]]
                .into_iter()
                .find(|&n| device(n).to_bytes()[..] == picked.device[..])
                .unwrap();
            let forged = resigned(&event, n, |body| {
                body.payload = Payload::new(&Value::Text(format!("forged {row}"))).unwrap();
            });
            db.execute(
                "UPDATE events SET message = ?1, hash = ?2 WHERE arrival = ?3",
                params![forged.to_bytes(), &forged.hash().as_bytes()[..], row],
            )
            .unwrap();
            model.unreadable.remove(&row);
            model.forged.insert(row);
        }
        Change::DropStream(_) => {
            let projection = projection_of(&picked.kind);
            let key = if projection == "orders" { "order_id" } else { "payment_id" };
            db.execute(
                "DELETE FROM events WHERE stream_kind = ?1 AND stream_id = ?2",
                params![picked.kind, picked.stream],
            )
            .unwrap();
            db.execute(&format!("DELETE FROM {projection} WHERE {key} = ?1"), [&picked.stream])
                .unwrap();
            let rows: Vec<i64> = present
                .iter()
                .filter(|event| event.kind == picked.kind && event.stream == picked.stream)
                .map(|event| event.row)
                .collect();
            model.deleted.extend(rows);
            model.rows.insert((projection, picked.stream), RowState::Deleted);
        }
        _ => {
            db.execute("DELETE FROM events WHERE arrival = ?1", [row]).unwrap();
            model.deleted.insert(row);
        }
    }
}

/// Makes `change` through `db`, and records it in `model`.
fn make(db: &rusqlite::Connection, model: &mut Model, change: Change) {
    match change {
        Change::Bytes(index)
        | Change::Hash(index)
        | Change::Refile(index)
        | Change::Delete(index)
        | Change::DropStream(index)
        | Change::Forge(index) => {
            change_event(db, model, change, index);
        }
        Change::Row(index) | Change::RowDeleted(index) => {
            let rows = projection_rows(db);
            if rows.is_empty() {
                return;
            }
            let (projection, key, stream) = rows[index.index(rows.len())].clone();
            let sql = if matches!(change, Change::Row(_)) {
                format!("UPDATE {projection} SET events = events + 1 WHERE {key} = ?1")
            } else {
                format!("DELETE FROM {projection} WHERE {key} = ?1")
            };
            db.execute(&sql, [&stream]).unwrap();
            let state = if matches!(change, Change::Row(_)) {
                RowState::Changed
            } else {
                RowState::Deleted
            };
            model.rows.insert((projection, stream), state);
        }
        Change::RowAdded(n) => {
            let stream = id::<Aggregate>(0x6000 + u64::from(n)).to_bytes().to_vec();
            let added = db
                .execute(
                    "INSERT OR IGNORE INTO orders (order_id, state, stage, live_lines, open_checks, \
                     closed_checks, conflicts, unreadable, events, first_hlc, last_hlc) \
                     VALUES (?1, 'uncreated', 'draft', 0, 0, 0, 0, 0, 1, zeroblob(8), zeroblob(8))",
                    [&stream],
                )
                .unwrap();
            if added == 1 {
                model.rows.insert(("orders", stream), RowState::Added);
            }
        }
        Change::Cause(index) | Change::Attempts(index) => {
            if model.effects.is_empty() {
                return;
            }
            let key = model.effects[index.index(model.effects.len())].effect.key.clone();
            let sql = if matches!(change, Change::Cause(_)) {
                "UPDATE outbox SET cause = zeroblob(16) WHERE key = ?1"
            } else {
                // Never started, it now has a start time; started, it now has no attempts.
                "UPDATE outbox SET started = CASE WHEN started IS NULL THEN 0 ELSE started END, \
                 attempts = CASE WHEN started IS NULL THEN attempts ELSE 0 END WHERE key = ?1"
            };
            db.execute(sql, [&key]).unwrap();
            model.effects_changed.insert(key);
        }
        Change::Clock => {
            db.execute("UPDATE store SET clock = zeroblob(8)", []).unwrap();
            model.clock = true;
        }
        Change::Digest(index) => {
            if model.quarantine.is_empty() {
                return;
            }
            let row = model.quarantine[index.index(model.quarantine.len())];
            // A digest of its own: digests are unique.
            let digest = [u8::try_from(row % 251).unwrap(); 32];
            db.execute(
                "UPDATE quarantine SET digest = ?1 WHERE arrival = ?2",
                params![&digest[..], row],
            )
            .unwrap();
            model.digests.insert(row);
        }
        Change::Identity => {
            db.execute("UPDATE store SET location = ?1", [&elsewhere().to_bytes()[..]]).unwrap();
            model.identity = true;
        }
    }
}

/// Every projection row: its projection, key column and stream.
fn projection_rows(db: &rusqlite::Connection) -> Vec<(&'static str, &'static str, Vec<u8>)> {
    let mut rows = Vec::new();
    for (projection, key) in [("orders", "order_id"), ("payments", "payment_id")] {
        let mut statement =
            db.prepare(&format!("SELECT {key} FROM {projection} ORDER BY {key}")).unwrap();
        let streams: Vec<Vec<u8>> =
            statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
        rows.extend(streams.into_iter().map(|stream| (projection, key, stream)));
    }
    rows
}

proptest! {
    /// Changes a bug could make behind the store's back: the check reports exactly the problems
    /// the model says they make, and nothing else.
    #[test]
    fn the_check_reports_exactly_the_changes_made(
        writes in prop::collection::vec(any_write(), 1..6),
        changes in prop::collection::vec(any_change(), 0..4),
    ) {
        let scratch = Scratch::new("changes");
        let mut store = stock(&scratch, &writes);
        prop_assert_eq!(store.check().unwrap(), []);
        let db = raw(&scratch.db());
        let mut model = Model {
            events: stored_events(&scratch),
            effects: store.effects().unwrap(),
            quarantine: {
                let mut statement = db.prepare("SELECT arrival FROM quarantine ORDER BY arrival").unwrap();
                statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
            },
            ..Model::default()
        };
        for change in &changes {
            make(&db, &mut model, *change);
        }
        let found: BTreeSet<String> =
            store.check().unwrap().iter().map(|problem| format!("{problem:?}")).collect();
        prop_assert_eq!(&found, &model.problems(), "{:?}", changes);
        // The check changed nothing: again, it finds the same.
        let again: BTreeSet<String> =
            store.check().unwrap().iter().map(|problem| format!("{problem:?}")).collect();
        prop_assert_eq!(again, found);
    }
}

#[test]
fn a_stocked_store_has_something_of_everything() {
    let writes = [WriteSpec {
        notes: vec![
            Note { payment: false, stream: 1, big: true },
            Note { payment: true, stream: 2, big: false },
        ],
        peers: [2, 1],
        malformed: true,
        enqueue: true,
        cause: None,
        start: None,
        finish: None,
    }];
    let scratch = Scratch::new("stocked");
    let mut store = stock(&scratch, &writes);
    assert_eq!(stored_events(&scratch).len(), 5);
    assert_eq!(store.effects().unwrap().len(), 1);
    assert_eq!(store.quarantine().unwrap().len(), 1);
    assert_eq!(store.check().unwrap(), []);
    assert!(matches!(store.effects().unwrap()[0].state, EffectState::Pending));
}
