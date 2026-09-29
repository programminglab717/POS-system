//! Known-answer tests for projections: each worked through with a few orders and payments.

use core::num::NonZeroU16;

use keel_domain::aggregate::fold;
use keel_domain::codec::{CatalogVersion, IdSet, Name, ReasonCode};
use keel_domain::order::{
    Channel, ItemSnapshot, LineAdded, Mode, Order, OrderCreated, OrderEvent, Reason, Stage,
};
use keel_domain::payment::{Payment, PaymentCaptured, PaymentEvent, PaymentInitiated, Tender};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{Actor, StreamKind, StreamRef};
use keel_types::{Currency, Money, Quantity, Unit};

use super::*;

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::from_code("USD").unwrap())
}

/// A draft of `event`, on the stream of the aggregate `stream` of kind `kind`.
fn domain_draft<D: DomainEvent>(
    stream: Id<keel_events::envelope::Aggregate>,
    event: &D,
) -> EventDraft {
    let (schema, payload) = event.encode().unwrap();
    EventDraft {
        stream: StreamRef { kind: StreamKind::new(D::STREAM).unwrap(), id: stream },
        schema,
        business_date: "2026-09-28".parse().unwrap(),
        actor: Actor::TeamMember(id(0x300)),
        approval: None,
        causation: None,
        correlation: None,
        payload,
    }
}

pub(super) fn order_draft(order: Id<Order>, event: &OrderEvent) -> EventDraft {
    domain_draft(order.cast(), event)
}

pub(super) fn payment_draft(payment: Id<Payment>, event: &PaymentEvent) -> EventDraft {
    domain_draft(payment.cast(), event)
}

pub(super) fn created() -> OrderEvent {
    OrderEvent::Created(OrderCreated {
        channel: Channel::Pos,
        mode: Mode::DineIn,
        currency: Currency::from_code("USD").unwrap(),
        revenue_center: None,
        table: Some(id(0x200)),
        guest_count: NonZeroU16::new(2),
        customer: None,
        owner: Some(id(0x300)),
    })
}

pub(super) fn line_added(line: u64, price: i64) -> OrderEvent {
    OrderEvent::LineAdded(LineAdded {
        line: id(line),
        item: ItemSnapshot {
            variant: id(0x400),
            catalog_version: CatalogVersion::from_bytes([7; 32]),
            name: Name::new("Flat white").unwrap(),
            tax_category: id(0x500),
            unit_price: usd(price),
        },
        quantity: Quantity::from_micros(1_000_000, Unit::Each),
        modifiers: Vec::new(),
        seat: None,
        course: None,
        notes: None,
    })
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

fn order_id() -> Id<Order> {
    id(0x7000)
}

/// Folds `events` into the order `id`, in the order given.
fn folded(id: Id<Order>, events: &[SignedEvent]) -> Order {
    let mut order = Order::new(id);
    for event in events {
        fold(&mut order, event);
    }
    order
}

#[test]
fn an_order_is_projected_as_it_folds() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    let events = append(
        &mut store,
        vec![
            order_draft(order, &created()),
            order_draft(order, &line_added(0x10, 450)),
            order_draft(order, &line_added(0x11, 300)),
            order_draft(order, &OrderEvent::LinesFired { lines: IdSet::new([id(0x10)]).unwrap() }),
            order_draft(order, &OrderEvent::CheckOpened { check: id(0x20) }),
        ],
        at(0),
    );
    let summary = store.order(order).unwrap().unwrap();
    let state = folded(order, &events);
    assert_eq!(summary.id, order);
    assert_eq!(summary.state, OrderState::Active);
    // One live line isn't fired yet.
    assert_eq!(summary.stage, Stage::Open);
    assert_eq!(summary.info.as_ref(), state.info());
    assert_eq!(summary.business_date, Some("2026-09-28".parse().unwrap()));
    assert_eq!((summary.live_lines, summary.open_checks, summary.closed_checks), (2, 2, 0));
    assert_eq!((summary.conflicts, summary.unreadable, summary.events), (0, 0, 5));
    assert_eq!(summary.first, events[0].body().hlc);
    assert_eq!(summary.last, events[4].body().hlc);
    assert_eq!(store.orders(OrderState::Active).unwrap(), core::slice::from_ref(&summary));
    assert!(store.orders(OrderState::Closed).unwrap().is_empty());
    // Voiding the order moves it.
    let void = OrderEvent::Voided { reason: reason("mistake") };
    append(&mut store, vec![order_draft(order, &void)], at(1));
    let voided = store.order(order).unwrap().unwrap();
    assert_eq!((voided.state, voided.events), (OrderState::Voided, 6));
    assert!(store.orders(OrderState::Active).unwrap().is_empty());
    assert_eq!(store.orders(OrderState::Voided).unwrap(), [voided]);
    // An order the store holds no event of has no row.
    assert_eq!(store.order(id(0x7001)).unwrap(), None);
}

#[test]
fn a_late_event_is_folded_in_its_place() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    // This device creates the order and, a second later, voids it.
    let mine = append(&mut store, vec![order_draft(order, &created())], at(0));
    // Another device added a line half a second after the creation, but its event arrives
    // after the void.
    let config = LogConfig {
        device: device(2).0,
        location: here(),
        head: LogHead::EMPTY,
        latest_hlc: mine[0].body().hlc,
        max_forward_drift: Duration::from_secs(60),
    };
    let mut other = LogWriter::new(config, device(2).1, SeededEntropy::new(2));
    let late = other.prepare(order_draft(order, &line_added(0x10, 450)), at(500)).unwrap().commit();
    let void = OrderEvent::Voided { reason: reason("mistake") };
    let voided = append(&mut store, vec![order_draft(order, &void)], at(1_000));
    assert!(matches!(receive(&mut store, &late.to_bytes()), Received::Stored(_)));
    // Folded in canonical order, the line comes before the void; in the order the events
    // arrived, after it, and the two differ.
    let canonical = folded(order, &[mine[0].clone(), late.clone(), voided[0].clone()]);
    let arrived = folded(order, &[mine[0].clone(), voided[0].clone(), late.clone()]);
    assert_ne!(canonical, arrived);
    let summary = store.order(order).unwrap().unwrap();
    assert_eq!(summary.live_lines, u64::try_from(canonical.live_lines().count()).unwrap());
    assert_eq!(summary.conflicts, u64::try_from(canonical.conflicts().len()).unwrap());
    assert_eq!(summary.last, voided[0].body().hlc);
    assert_eq!(store.load(Order::new(order)).unwrap(), canonical);
}

#[test]
fn payments_are_projected_and_found_by_their_order() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    let payment: Id<Payment> = id(0x8000);
    let initiated = PaymentEvent::Initiated(PaymentInitiated {
        order,
        check: order.cast(),
        tender: Tender::Card,
        amount: usd(750),
    });
    let captured = PaymentEvent::Captured(PaymentCaptured {
        amount: usd(750),
        tip: Some(usd(100)),
        reference: None,
        cash: None,
    });
    let events = append(
        &mut store,
        vec![payment_draft(payment, &initiated), payment_draft(payment, &captured)],
        at(0),
    );
    let summary = store.payment(payment).unwrap().unwrap();
    assert_eq!(summary.id, payment);
    assert_eq!(summary.state, PaymentState::Captured);
    let info = summary.info.as_ref().unwrap();
    assert_eq!(
        (info.order, info.check, info.tender, info.amount),
        (order, order.cast(), Tender::Card, usd(750))
    );
    assert_eq!(info.initiated_by, events[0].body().event_id);
    assert_eq!((summary.captured, summary.tip), (Some(usd(750)), Some(usd(100))));
    assert_eq!(summary.business_date, Some("2026-09-28".parse().unwrap()));
    assert_eq!((summary.conflicts, summary.unreadable, summary.events), (0, 0, 2));
    assert_eq!(store.payments_of(order).unwrap(), [summary]);
    assert!(store.payments_of(id(0x7001)).unwrap().is_empty());
    // A capture before the initiation arrives leaves the payment uninitiated, with a conflict.
    let early: Id<Payment> = id(0x8001);
    append(&mut store, vec![payment_draft(early, &captured)], at(1));
    let summary = store.payment(early).unwrap().unwrap();
    assert_eq!(
        (summary.state, summary.info, summary.captured),
        (PaymentState::Uninitiated, None, None)
    );
    assert_eq!(summary.conflicts, 1);
}

#[test]
fn a_failed_write_leaves_the_projections_as_they_were() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    append(&mut store, vec![order_draft(order, &created())], at(0));
    let before = store.order(order).unwrap();
    let failed: Result<(), StoreError> = store.write(|w| {
        w.append(order_draft(order, &line_added(0x10, 450)), at(1))?;
        w.append(order_draft(id(0x7001), &created()), at(1))?;
        Err(StoreError::Corrupt("the caller changed its mind"))
    });
    assert!(failed.is_err());
    assert_eq!(store.order(order).unwrap(), before);
    assert_eq!(store.order(id(0x7001)).unwrap(), None);
}

#[test]
fn a_write_loads_what_it_stored_and_reports_the_streams_it_touched() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    let payment: Id<Payment> = id(0x8000);
    let (touched, loaded) = store
        .write(|w| {
            w.append(order_draft(order, &created()), at(0))?;
            w.append(order_draft(order, &line_added(0x10, 450)), at(0))?;
            let loaded = w.load(Order::new(order))?;
            w.append(
                payment_draft(
                    payment,
                    &PaymentEvent::Initiated(PaymentInitiated {
                        order,
                        check: order.cast(),
                        tender: Tender::Cash,
                        amount: usd(450),
                    }),
                ),
                at(0),
            )?;
            w.append(order_draft(order, &line_added(0x11, 300)), at(0))?;
            Ok::<_, StoreError>((w.touched().to_vec(), loaded))
        })
        .unwrap();
    // Loaded after the first line: the write's own events so far.
    assert_eq!(loaded.live_lines().count(), 1);
    let kinds: Vec<&str> = touched.iter().map(|stream| stream.kind.as_str()).collect();
    assert_eq!(kinds, ["order", "payment"]);
    assert_eq!(touched[0].id, order.cast());
    assert_eq!(touched[1].id, payment.cast());
    assert_eq!(store.load(Order::new(order)).unwrap().live_lines().count(), 2);
}

#[test]
fn projections_are_rebuilt_when_their_version_changes() {
    let dir = TempDir::new();
    let order = order_id();
    {
        let mut store = open(&dir.db());
        append(
            &mut store,
            vec![order_draft(order, &created()), order_draft(order, &line_added(0x10, 450))],
            at(0),
        );
        // A row that disagrees with the events, and a version this kernel didn't build.
        store.db().execute("UPDATE orders SET live_lines = 99", []).unwrap();
        store.db().execute("UPDATE projections SET version = 0 WHERE name = 'orders'", []).unwrap();
    }
    let store = open(&dir.db());
    assert_eq!(store.order(order).unwrap().unwrap().live_lines, 1);
    let version: i64 = store
        .db()
        .query_row("SELECT version FROM projections WHERE name = 'orders'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn a_rebuild_on_request_matches_what_writes_kept() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    append(
        &mut store,
        vec![order_draft(order, &created()), order_draft(order, &line_added(0x10, 450))],
        at(0),
    );
    let kept = (store.order(order).unwrap(), store.orders(OrderState::Active).unwrap());
    store.db().execute("DELETE FROM orders", []).unwrap();
    store.rebuild_projections().unwrap();
    assert_eq!((store.order(order).unwrap(), store.orders(OrderState::Active).unwrap()), kept);
}

#[test]
fn a_version_1_store_migrates_and_builds_its_projections() {
    let dir = TempDir::new();
    let order = order_id();
    {
        let mut store = open(&dir.db());
        append(&mut store, vec![order_draft(order, &created())], at(0));
        // Take the store back to version 1.
        store
            .db()
            .execute_batch(
                "DROP TABLE outbox; DROP TABLE projections; DROP TABLE orders; \
                 DROP TABLE payments; PRAGMA user_version = 1;",
            )
            .unwrap();
    }
    let store = open(&dir.db());
    let version: i64 = store.db().query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!(version, 2);
    assert_eq!(store.order(order).unwrap().unwrap().state, OrderState::Active);
    assert!(store.effects().unwrap().is_empty());
}

#[test]
fn an_interrupted_rebuild_changes_nothing_and_opening_again_finishes_it() {
    let dir = TempDir::new();
    let order = order_id();
    {
        let mut store = open(&dir.db());
        append(&mut store, vec![order_draft(order, &created())], at(0));
        store.db().execute("UPDATE projections SET version = 0 WHERE name = 'orders'", []).unwrap();
        store.db().execute("UPDATE orders SET live_lines = 99", []).unwrap();
    }
    let faults = Box::new(RefuseAt { point: Point::Rebuilding, nth: 1, seen: 0 });
    let opened =
        Store::open_with_faults(dir.db(), config(), own().1, SeededEntropy::new(7), faults);
    assert!(matches!(opened, Err(StoreError::Interrupted(Point::Rebuilding))));
    let db = Connection::open(dir.db()).unwrap();
    let lines: i64 = db.query_row("SELECT live_lines FROM orders", [], |row| row.get(0)).unwrap();
    assert_eq!(lines, 99);
    drop(db);
    assert_eq!(open(&dir.db()).order(order).unwrap().unwrap().live_lines, 0);
}

/// The golden rows: a projection's rows for a fixed history. If this test fails, the projection's
/// rows changed, because its columns or a fold it uses changed: bump the projection's version,
/// so that stores rebuild it, then update the expected rows.
#[test]
fn projection_rows_are_pinned() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    let payment: Id<Payment> = id(0x8000);
    append(
        &mut store,
        vec![
            order_draft(order, &created()),
            order_draft(order, &line_added(0x10, 450)),
            order_draft(order, &OrderEvent::LinesFired { lines: IdSet::new([id(0x10)]).unwrap() }),
            order_draft(order, &OrderEvent::Abandoned),
            payment_draft(
                payment,
                &PaymentEvent::Initiated(PaymentInitiated {
                    order,
                    check: order.cast(),
                    tender: Tender::Cash,
                    amount: usd(450),
                }),
            ),
        ],
        at(0),
    );
    let dump = |table: &str| -> String {
        let mut statement = store.db().prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| {
                        row.get::<_, rusqlite::types::Value>(column).map(|v| format!("{v:?}"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap();
        rows.map(|row| row.unwrap().join(" | ")).collect::<Vec<_>>().join("\n")
    };
    let orders = dump("orders");
    let payments = dump("payments");
    // Identifiers and HLCs are the test's own; the rest is what the folds make of the history.
    assert_eq!(orders, GOLDEN_ORDERS, "{orders}");
    assert_eq!(payments, GOLDEN_PAYMENTS, "{payments}");
}

const GOLDEN_ORDERS: &str = "Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 112, 0]) | \
    Text(\"abandoned\") | Text(\"submitted\") | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 1, 0]) | Integer(0) | Integer(0) | \
    Text(\"USD\") | \
    Blob([1, 160, 229, 79, 176, 0, 117, 215, 132, 76, 60, 215, 244, 60, 102, 28]) | Null | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 2, 0]) | Integer(2) | Null | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 3, 0]) | Text(\"2026-09-28\") | \
    Integer(1) | Integer(1) | Integer(0) | Integer(1) | Integer(0) | Integer(4) | \
    Blob([1, 160, 229, 79, 176, 0, 0, 0]) | Blob([1, 160, 229, 79, 176, 0, 0, 3])";
const GOLDEN_PAYMENTS: &str = "Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 128, 0]) | \
    Text(\"initiated\") | Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 1, 0]) | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 112, 0]) | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 112, 0]) | Integer(0) | \
    Integer(450) | Text(\"USD\") | \
    Blob([1, 160, 229, 79, 176, 0, 117, 219, 191, 218, 190, 134, 203, 190, 170, 17]) | \
    Text(\"2026-09-28\") | Null | Null | Integer(0) | Integer(0) | Integer(1) | \
    Blob([1, 160, 229, 79, 176, 0, 0, 4]) | Blob([1, 160, 229, 79, 176, 0, 0, 4])";
