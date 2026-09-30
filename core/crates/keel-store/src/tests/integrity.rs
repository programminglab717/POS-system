//! Known-answer tests for the full check, and for damage: a damaged store fails closed, and the
//! check finds exactly what was damaged or changed (ADR-0018).

use keel_domain::order::Order;
use keel_events::envelope::Event;
use rusqlite::params;

use super::projections::{created, line_added, order_draft};
use super::*;

/// What [`stocked`] put in a store.
struct Stock {
    /// The device's own events, the order's among them.
    own: Vec<SignedEvent>,
    /// Device 2's events.
    theirs: Vec<SignedEvent>,
    order: Id<Order>,
}

/// A store with something of everything: the device's own events, an order with a line,
/// another device's events, a quarantined message, and effects pending, running and done.
fn stocked(dir: &TempDir) -> (TestStore, Stock) {
    let mut store = open(&dir.db());
    let order: Id<Order> = id(0x7000);
    let theirs = log_of(2, here(), 2);
    let own = store
        .write(|w| {
            let mut own = vec![w.append(draft(1, 1), at(0))?];
            own.push(w.append(order_draft(order, &created()), at(0))?);
            own.push(w.append(order_draft(order, &line_added(0x900, 450)), at(0))?);
            for event in &theirs {
                w.receive(&event.to_bytes(), &registry(), at(0))?;
            }
            w.receive(b"not an event", &registry(), at(0))?;
            for (key, cause) in [(b"pending", 0), (b"running", 1), (b"done!!!", 2)] {
                let effect = Effect {
                    key: key.to_vec(),
                    kind: EffectKind::new("print.receipt").unwrap(),
                    payload: key.to_vec(),
                    cause: Some(own[cause].body().event_id),
                };
                w.enqueue(&effect, at(0))?;
            }
            w.start(b"running", at(1))?;
            w.start(b"done!!!", at(1))?;
            w.finish(b"done!!!")?;
            own.push(w.append(draft(2, 2), at(1))?);
            Ok::<_, StoreError>(own)
        })
        .unwrap();
    (store, Stock { own, theirs, order })
}

/// The events table's row holding `event`.
fn row_of(store: &TestStore, event: &SignedEvent) -> i64 {
    store
        .db()
        .query_row(
            "SELECT arrival FROM events WHERE event_id = ?1",
            [&event.body().event_id.to_bytes()[..]],
            |row| row.get(0),
        )
        .unwrap()
}

/// Flips a bit of the byte at `offset` of the file at `path`.
fn flip(path: &Path, offset: usize) {
    let mut file = std::fs::read(path).unwrap();
    file[offset] ^= 0x10;
    std::fs::write(path, file).unwrap();
}

#[test]
fn a_sound_store_checks_clean() {
    let dir = TempDir::new();
    let (mut store, _) = stocked(&dir);
    assert_eq!(store.check().unwrap(), []);
    // The check changed nothing, and the store goes on.
    append(&mut store, vec![draft(1, 9)], at(2));
    assert_eq!(store.check().unwrap(), []);
    drop(store);
    assert_eq!(open(&dir.db()).check().unwrap(), []);
}

/// A damaged page fails the store closed: nothing is read from it, and nothing more from the
/// store until it is reopened, while the check still finds the page.
#[test]
fn a_damaged_page_fails_the_store_closed_and_the_check_finds_it() {
    let dir = TempDir::new();
    let (mut store, stock) = stocked(&dir);
    // An event big enough to spill onto pages of its own.
    let big = EventDraft {
        payload: Payload::new(&Value::Bytes(vec![0x42; 10_000])).unwrap(),
        ..draft(3, 0)
    };
    let big = append(&mut store, vec![big], at(2)).remove(0);
    let last = append(&mut store, vec![draft(1, 3)], at(3)).remove(0);
    let spilled: Vec<u32> = {
        let mut statement = store
            .db()
            .prepare(
                "SELECT pageno FROM dbstat WHERE name = 'events' AND pagetype = 'overflow' \
                 ORDER BY pageno",
            )
            .unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert!(spilled.len() >= 2, "{spilled:?}");
    drop(store);
    let page = spilled[0];
    flip(&dir.db(), usize::try_from(page - 1).unwrap() * 4096 + 1_000);
    let mut store = open(&dir.db());
    assert_eq!(store.event(last.body().event_id).unwrap(), Some(last.clone()));
    assert!(matches!(store.event(big.body().event_id), Err(StoreError::Damaged)));
    // Nothing more, not even what read a moment ago.
    assert!(matches!(store.event(last.body().event_id), Err(StoreError::Damaged)));
    assert!(matches!(store.head(own().0), Err(StoreError::Damaged)));
    assert!(matches!(store.order(stock.order), Err(StoreError::Damaged)));
    let written: Result<SignedEvent, StoreError> = store.write(|w| w.append(draft(1, 4), at(4)));
    assert!(matches!(written, Err(StoreError::Damaged)));
    assert!(matches!(store.rekey(key_of(0x4C)), Err(StoreError::Damaged)));
    // The check finds the page, and the store stays damaged.
    assert_eq!(store.check().unwrap(), [Problem::Page(page)]);
    assert!(matches!(store.head(own().0), Err(StoreError::Damaged)));
    drop(store);
    // Reopened, it reads what isn't damaged, and the check finds the page again.
    let mut store = open(&dir.db());
    assert_eq!(store.log(device(2).0, 0, 10).unwrap(), stock.theirs);
    assert_eq!(store.check().unwrap(), [Problem::Page(page)]);
}

#[test]
fn a_damaged_first_page_is_a_key_that_doesnt_open_the_store() {
    // The salt, in the first 16 bytes, and the rest of the first page.
    for offset in [3, 40, 4_000] {
        let dir = TempDir::new();
        drop(stocked(&dir));
        flip(&dir.db(), offset);
        let opened = Store::open(dir.db(), key(), config(), own().1, SeededEntropy::new(1));
        assert!(matches!(opened, Err(StoreError::KeyRejected)), "{offset}");
    }
}

/// A file cut short by whole pages is damaged when it opens; cut inside its last page, it opens,
/// and the check finds the page that is too short.
#[test]
fn a_store_cut_short_is_damaged() {
    let dir = TempDir::new();
    drop(stocked(&dir));
    let file = std::fs::read(dir.db()).unwrap();
    let pages = u32::try_from(file.len() / 4096).unwrap();
    std::fs::write(dir.db(), &file[..file.len() - 4096]).unwrap();
    let opened = Store::open(dir.db(), key(), config(), own().1, SeededEntropy::new(1));
    assert!(matches!(opened, Err(StoreError::Damaged)), "{:?}", opened.map(|_| ()));
    std::fs::write(dir.db(), &file[..file.len() - 100]).unwrap();
    let mut store = open(&dir.db());
    assert_eq!(store.check().unwrap(), [Problem::Page(pages)]);
}

#[test]
fn pages_added_to_the_file_are_found() {
    let dir = TempDir::new();
    drop(stocked(&dir));
    let mut file = std::fs::read(dir.db()).unwrap();
    let pages = u32::try_from(file.len() / 4096).unwrap();
    file.extend(core::iter::repeat_n(0x5A, 4096 + 100));
    std::fs::write(dir.db(), file).unwrap();
    let mut store = open(&dir.db());
    // A whole page that doesn't authenticate, and a part of one.
    assert_eq!(store.check().unwrap(), [Problem::Page(pages + 1), Problem::Page(pages + 2)]);
}

/// A change behind the store's back: it makes the change, and says what the check finds.
type Change = fn(&TestStore, &Stock) -> Vec<Problem>;

/// Changes a bug could make, behind the store's back, with the key.
fn changes() -> [(&'static str, Change); 10] {
    [
        ("an event's bytes", |store, stock| {
            let row = row_of(store, &stock.own[0]);
            let mut bytes = stock.own[0].to_bytes();
            bytes.push(0);
            store
                .db()
                .execute("UPDATE events SET message = ?1 WHERE arrival = ?2", params![bytes, row])
                .unwrap();
            vec![Problem::Event { row }]
        }),
        ("an event's stored hash", |store, stock| {
            let row = row_of(store, &stock.theirs[1]);
            store
                .db()
                .execute("UPDATE events SET hash = zeroblob(32) WHERE arrival = ?1", [row])
                .unwrap();
            vec![Problem::Event { row }]
        }),
        ("the identifier an event is filed under", |store, stock| {
            let row = row_of(store, &stock.own[3]);
            let other = id::<Event>(0xABC).to_bytes();
            store
                .db()
                .execute(
                    "UPDATE events SET event_id = ?1 WHERE arrival = ?2",
                    params![&other[..], row],
                )
                .unwrap();
            vec![Problem::Event { row }]
        }),
        ("an event missing from a log", |store, stock| {
            // The order's line: the order's row and the effect it caused change too.
            let row = row_of(store, &stock.own[2]);
            store.db().execute("DELETE FROM events WHERE arrival = ?1", [row]).unwrap();
            vec![
                Problem::Chain { device: own().0, seq: 4 },
                Problem::Projection {
                    projection: "orders",
                    stream: stock.order.to_bytes().to_vec(),
                },
                Problem::Effect { key: b"done!!!".to_vec() },
            ]
        }),
        ("a projection's row", |store, stock| {
            let order = stock.order.to_bytes();
            store
                .db()
                .execute("UPDATE orders SET live_lines = 7 WHERE order_id = ?1", [&order[..]])
                .unwrap();
            vec![Problem::Projection {
                projection: "orders",
                stream: stock.order.to_bytes().to_vec(),
            }]
        }),
        ("a projection's row for a stream with no events", |store, stock| {
            store.db().execute(
                "INSERT INTO orders SELECT ?1, state, stage, location, channel, mode, currency, \
                 created_by, revenue_center, table_id, guest_count, customer, owner, \
                 business_date, live_lines, open_checks, closed_checks, conflicts, unreadable, \
                 events, first_hlc, last_hlc FROM orders WHERE order_id = ?2",
                [&id::<Order>(0x7001).to_bytes()[..], &stock.order.to_bytes()[..]],
            ).unwrap();
            vec![Problem::Projection {
                projection: "orders",
                stream: id::<Order>(0x7001).to_bytes().to_vec(),
            }]
        }),
        ("an effect's cause", |store, _| {
            let key = &b"pending"[..];
            store
                .db()
                .execute("UPDATE outbox SET cause = zeroblob(16) WHERE key = ?1", [key])
                .unwrap();
            vec![Problem::Effect { key: b"pending".to_vec() }]
        }),
        ("an effect's attempts", |store, _| {
            store
                .db()
                .execute("UPDATE outbox SET attempts = 0 WHERE key = ?1", [&b"running"[..]])
                .unwrap();
            vec![Problem::Effect { key: b"running".to_vec() }]
        }),
        ("the device's clock", |store, _| {
            store.db().execute("UPDATE store SET clock = zeroblob(8)", []).unwrap();
            vec![Problem::Clock]
        }),
        ("a quarantined message's digest", |store, _| {
            store.db().execute("UPDATE quarantine SET digest = zeroblob(32)", []).unwrap();
            vec![Problem::Quarantined { row: 1 }]
        }),
    ]
}

/// The check reports exactly each change behind the store's back.
#[test]
fn the_check_finds_exactly_what_changed_behind_the_stores_back() {
    for (what, change) in changes() {
        let dir = TempDir::new();
        let (mut store, stock) = stocked(&dir);
        let expected = change(&store, &stock);
        assert_eq!(store.check().unwrap(), expected, "{what}");
    }
}

/// The store's identity, which the store also checks when it opens.
#[test]
fn the_check_finds_a_changed_identity() {
    let dir = TempDir::new();
    let (mut store, _) = stocked(&dir);
    store.db().execute("UPDATE store SET location = ?1", [&elsewhere().to_bytes()[..]]).unwrap();
    assert_eq!(store.check().unwrap(), [Problem::Identity]);
}

/// A constraint broken behind SQLite's back: SQLite's own check finds it, and the check stops
/// there.
#[test]
fn a_malformed_database_stops_the_check() {
    let dir = TempDir::new();
    let (mut store, _) = stocked(&dir);
    store
        .db()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON; UPDATE outbox SET attempts = -1; \
             UPDATE orders SET live_lines = 7; PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let problems = store.check().unwrap();
    assert!(!problems.is_empty());
    assert!(
        problems.iter().all(|problem| matches!(problem, Problem::Structure(_))),
        "{problems:?}"
    );
}

/// Found by the damage property test: SQLCipher gives SQLite a damaged page as zeros, and fails
/// only the reads after it. The last page of a long value holds nothing but the value's bytes, so
/// a read of it gave zeros in the value, without an error: here, an effect's payload.
#[test]
fn a_damaged_page_that_reads_as_zeros_is_still_damaged() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let payload = vec![0x42; 10_000];
    let effect = Effect {
        key: b"long".to_vec(),
        kind: EffectKind::new("print.receipt").unwrap(),
        payload: payload.clone(),
        cause: None,
    };
    store.write(|w| w.enqueue(&effect, at(0))).unwrap();
    let last: u32 = store
        .db()
        .query_row(
            "SELECT max(pageno) FROM dbstat WHERE name = 'outbox' AND pagetype = 'overflow'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(store);
    flip(&dir.db(), usize::try_from(last - 1).unwrap() * 4096 + 2_000);
    let store = open(&dir.db());
    assert!(matches!(store.effect(b"long"), Err(StoreError::Damaged)));
    assert!(matches!(store.effects(), Err(StoreError::Damaged)));
    // Inside a write, the write fails.
    drop(store);
    let mut store = open(&dir.db());
    let started = store.write(|w| w.start(b"long", at(1)));
    assert!(matches!(started, Err(StoreError::Damaged)), "{started:?}");
    // Read afresh, the payload is damaged all the same.
    drop(store);
    let mut store = open(&dir.db());
    assert!(matches!(store.effects(), Err(StoreError::Damaged)));
    assert_eq!(store.check().unwrap(), [Problem::Page(last)]);
}

/// An event after which the device's clock runs backward, forged with the device's key: the
/// check finds the log broken there, and again at the next event, which links to the original.
#[test]
fn a_log_whose_clock_runs_backward_is_broken() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let theirs = log_of(2, here(), 3);
    for event in &theirs {
        receive(&mut store, &event.to_bytes());
    }
    let forged = resigned(&theirs[1], 2, |body| body.hlc = theirs[0].body().hlc);
    let forged = SignedEvent::from_stored(&forged).unwrap();
    store
        .db()
        .execute(
            "UPDATE events SET message = ?1, hash = ?2, hlc = ?3 WHERE event_id = ?4",
            params![
                forged.to_bytes(),
                &forged.hash().as_bytes()[..],
                &schema::hlc_bytes(forged.body().hlc)[..],
                &forged.body().event_id.to_bytes()[..],
            ],
        )
        .unwrap();
    assert_eq!(
        store.check().unwrap(),
        [
            Problem::Chain { device: device(2).0, seq: 2 },
            Problem::Chain { device: device(2).0, seq: 3 }
        ]
    );
}

/// An event from another location, filed in the store as its body says, as a bug could: the
/// store keeps its own location's events only.
#[test]
fn an_event_from_another_location_is_found() {
    let dir = TempDir::new();
    let (mut store, _) = stocked(&dir);
    let foreign = log_of(9, elsewhere(), 1).remove(0);
    let body = foreign.body();
    store
        .db()
        .execute(
            "INSERT INTO events (origin_device, origin_seq, event_id, hash, hlc, stream_kind, \
             stream_id, message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &body.origin_device.to_bytes()[..],
                1,
                &body.event_id.to_bytes()[..],
                &foreign.hash().as_bytes()[..],
                &schema::hlc_bytes(body.hlc)[..],
                body.stream.kind.as_str(),
                &body.stream.id.to_bytes()[..],
                foreign.to_bytes(),
            ],
        )
        .unwrap();
    let row = row_of(&store, &foreign);
    // The stream it names is an order's: its projection row, missing, is found too.
    let problems = store.check().unwrap();
    assert_eq!(problems[0], Problem::Event { row });
    assert!(problems[1..].iter().all(|problem| matches!(problem, Problem::Projection { .. })));
}

/// Another connection reading the store keeps the WAL from moving into the database file, and a
/// check of the file alone would miss what is still in the WAL: the check says so instead.
#[test]
fn a_check_another_reader_holds_up_is_busy() {
    let dir = TempDir::new();
    let (mut store, _) = stocked(&dir);
    let reader = raw(&dir.db());
    reader.execute_batch("BEGIN").unwrap();
    let _: i64 = reader.query_row("SELECT count(*) FROM events", [], |row| row.get(0)).unwrap();
    // SQLite waits out its busy timeout, five seconds, first.
    assert!(matches!(store.check(), Err(StoreError::Busy)));
    reader.execute_batch("COMMIT").unwrap();
    drop(reader);
    assert_eq!(store.check().unwrap(), []);
}
