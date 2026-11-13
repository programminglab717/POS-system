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

pub(super) fn usd(minor: i64) -> Money {
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
    assert_eq!(store.payments_of(order).unwrap(), core::slice::from_ref(&summary));
    assert!(store.payments_of(id(0x7001)).unwrap().is_empty());
    // Found by its business date, whatever its order's (ADR-0031).
    assert_eq!(store.payments_on("2026-09-28".parse().unwrap()).unwrap(), [summary]);
    assert!(store.payments_on("2026-09-29".parse().unwrap()).unwrap().is_empty());
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
    assert_eq!(version, 2);
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
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(7), faults);
    assert!(matches!(opened, Err(StoreError::Interrupted(Point::Rebuilding))));
    let db = raw(&dir.db());
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
    Blob([1, 160, 229, 79, 176, 0, 0, 0]) | Blob([1, 160, 229, 79, 176, 0, 0, 3]) | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 0, 1]) | Integer(0) | Integer(0)";
const GOLDEN_PAYMENTS: &str = "Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 128, 0]) | \
    Text(\"initiated\") | Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 1, 0]) | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 112, 0]) | \
    Blob([1, 146, 240, 193, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 112, 0]) | Integer(0) | \
    Integer(450) | Text(\"USD\") | \
    Blob([1, 160, 229, 79, 176, 0, 117, 219, 191, 218, 190, 134, 203, 190, 170, 17]) | \
    Text(\"2026-09-28\") | Null | Null | Integer(0) | Integer(0) | Integer(1) | \
    Blob([1, 160, 229, 79, 176, 0, 0, 4]) | Blob([1, 160, 229, 79, 176, 0, 0, 4])";

/// A drawer session's events are projected (ADR-0031): found by its device and state, and by
/// its business date.
#[test]
fn drawers_are_projected_and_found_by_device_and_date() {
    use keel_domain::drawer::Drawer;
    use keel_domain::drawer::{
        CashMoved, CashRefunded, CashTaken, DrawerClosed, DrawerEvent, Movement, Sums,
    };
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let drawer: Id<Drawer> = id(0x9000);
    let draft = |event: &DrawerEvent| domain_draft(drawer.cast(), event);
    let opened = DrawerEvent::Opened { float: usd(10_000) };
    let taken = DrawerEvent::CashTaken(CashTaken { payment: id(0x8000), amount: usd(1_089) });
    let paid_out = DrawerEvent::CashMoved(CashMoved {
        kind: Movement::PaidOut,
        amount: usd(500),
        reason: Reason { code: ReasonCode::new("supplier").unwrap(), note: None },
    });
    // A refund's cash handed back (ADR-0032).
    let refunded = DrawerEvent::CashRefunded(CashRefunded { refund: id(0x8100), amount: usd(354) });
    let drafts = vec![draft(&opened), draft(&taken), draft(&paid_out), draft(&refunded)];
    let events = append(&mut store, drafts, at(0));
    // The payment's cash is found in a drawer; another's isn't; and the refund's.
    assert!(store.in_drawer(id(0x8000)).unwrap());
    assert!(!store.in_drawer(id(0x8001)).unwrap());
    assert!(store.refund_in_drawer(id(0x8100)).unwrap());
    assert!(!store.refund_in_drawer(id(0x8101)).unwrap());
    let summary = store.drawer(drawer).unwrap().unwrap();
    assert_eq!(summary.state, DrawerState::Open);
    let info = summary.info.as_ref().unwrap();
    assert_eq!((info.float, info.opened_by), (usd(10_000), events[0].body().event_id));
    assert_eq!(
        summary.sums,
        Some(Sums {
            taken: usd(1_089),
            paid_in: usd(0),
            paid_out: usd(500),
            dropped: usd(0),
            refunded: usd(354),
        })
    );
    assert_eq!(summary.business_date, Some("2026-09-28".parse().unwrap()));
    let device = info.device;
    assert_eq!(
        store.drawers_of(device, DrawerState::Open).unwrap(),
        core::slice::from_ref(&summary)
    );
    assert!(store.drawers_of(device, DrawerState::Closed).unwrap().is_empty());
    assert!(store.drawers_of(id(0xd9), DrawerState::Open).unwrap().is_empty());
    assert_eq!(store.drawers_on("2026-09-28".parse().unwrap()).unwrap(), [summary]);
    assert!(store.drawers_on("2026-09-29".parse().unwrap()).unwrap().is_empty());
    // Closed, it is found closed, with its count.
    let closed = DrawerEvent::Closed(DrawerClosed { counted: usd(10_580), expected: usd(10_589) });
    // Closed after midnight, on the next business day: the session stays on the day it opened.
    // Found by a planted bug: every event here was on one day.
    let mut late = draft(&closed);
    late.business_date = "2026-09-29".parse().unwrap();
    append(&mut store, vec![late], at(1));
    assert_eq!(store.drawers_on("2026-09-28".parse().unwrap()).unwrap().len(), 1);
    assert!(store.drawers_on("2026-09-29".parse().unwrap()).unwrap().is_empty());
    let summary = store.drawer(drawer).unwrap().unwrap();
    assert_eq!(summary.state, DrawerState::Closed);
    assert_eq!(summary.closed.map(|close| close.variance()), Some(Some(usd(-9))));
    assert!(store.drawers_of(device, DrawerState::Open).unwrap().is_empty());
    assert_eq!(store.drawers_of(device, DrawerState::Closed).unwrap(), [summary]);
    // Rebuilt from the events, it is the same.
    store.rebuild_projections().unwrap();
    assert_eq!(store.drawer(drawer).unwrap().unwrap().state, DrawerState::Closed);
    assert!(store.in_drawer(id(0x8000)).unwrap());
    assert!(store.refund_in_drawer(id(0x8100)).unwrap());
    // Cash before the opening: unopened, with a conflict.
    let early: Id<Drawer> = id(0x9001);
    append(&mut store, vec![domain_draft(early.cast(), &taken)], at(2));
    let summary = store.drawer(early).unwrap().unwrap();
    assert_eq!((summary.state, summary.info, summary.sums), (DrawerState::Unopened, None, None));
    assert_eq!(summary.conflicts, 1);
}

/// A refund's events are projected (ADR-0032): found by its order and by its business date.
#[test]
fn refunds_are_projected_and_found_by_order_and_date() {
    use keel_domain::refund::{Refund, RefundCompleted, RefundEvent, RefundInitiated};
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let order = order_id();
    let refund: Id<Refund> = id(0x8100);
    let draft = |event: &RefundEvent| domain_draft(refund.cast(), event);
    let initiated =
        RefundEvent::Initiated(RefundInitiated { order, tender: Tender::Cash, amount: usd(708) });
    let events = append(&mut store, vec![draft(&initiated)], at(0));
    let summary = store.refund(refund).unwrap().unwrap();
    assert_eq!(summary.state, RefundState::Initiated);
    let info = summary.info.unwrap();
    assert_eq!((info.order, info.tender, info.amount), (order, Tender::Cash, usd(708)));
    assert_eq!(info.initiated_by, events[0].body().event_id);
    assert_eq!(
        (summary.completed, summary.business_date),
        (None, Some("2026-09-28".parse().unwrap()))
    );
    // Paid back, with two cents of rounding.
    let paid = RefundCompleted { amount: usd(708), rounding: Some(usd(2)) };
    append(&mut store, vec![draft(&RefundEvent::Completed(paid))], at(1));
    let summary = store.refund(refund).unwrap().unwrap();
    assert_eq!((summary.state, summary.completed), (RefundState::Completed, Some(paid)));
    assert_eq!(store.refunds_of(order).unwrap(), core::slice::from_ref(&summary));
    assert!(store.refunds_of(id(0x7001)).unwrap().is_empty());
    assert_eq!(
        store.refunds_on("2026-09-28".parse().unwrap()).unwrap(),
        core::slice::from_ref(&summary)
    );
    assert!(store.refunds_on("2026-09-29".parse().unwrap()).unwrap().is_empty());
    store.rebuild_projections().unwrap();
    assert_eq!(store.refund(refund).unwrap(), Some(summary));
    // A completion before the start: unstarted, with a conflict.
    let early: Id<Refund> = id(0x8101);
    append(&mut store, vec![domain_draft(early.cast(), &RefundEvent::Completed(paid))], at(2));
    let summary = store.refund(early).unwrap().unwrap();
    assert_eq!(
        (summary.state, summary.info, summary.completed),
        (RefundState::Unstarted, None, None)
    );
    assert_eq!(summary.conflicts, 1);
}

/// An item's availability is projected (ADR-0033): whether it is sold out at a time, the items
/// that are, and a version that moves with every change.
#[test]
fn availability_is_projected_and_sold_out_items_found() {
    use keel_domain::availability::{
        Availability, AvailabilityChanged, AvailabilityEvent, ItemKind,
    };
    use keel_types::Timestamp;
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let at = |minutes: i64| {
        Timestamp::from_micros(1_794_560_400_000_000 + minutes * 60_000_000).unwrap()
    };
    let change = |available, until| {
        AvailabilityEvent::Changed(AvailabilityChanged {
            kind: ItemKind::Variant,
            available,
            until,
            reason: None,
        })
    };
    let (espresso, latte): (Id<Availability>, Id<Availability>) = (id(0x401), id(0x402));
    assert_eq!(store.availability_version().unwrap(), 0);
    assert_eq!(store.availability(espresso).unwrap(), None);
    // The espresso 86'd until 9:00, the latte until put back.
    append(
        &mut store,
        vec![
            domain_draft(espresso.cast(), &change(false, Some(at(60)))),
            domain_draft(latte.cast(), &change(false, None)),
        ],
        at(0),
    );
    assert_eq!(store.availability_version().unwrap(), 2);
    let summary = store.availability(espresso).unwrap().unwrap();
    assert_eq!(summary.latest, Some((ItemKind::Variant, false, Some(at(60)))));
    assert!(summary.sold_out_at(at(59)) && !summary.sold_out_at(at(60)));
    let sold_out =
        |now| store.sold_out(now).unwrap().into_iter().map(|item| item.id).collect::<Vec<_>>();
    assert_eq!(sold_out(at(30)), [espresso, latte]);
    assert_eq!(sold_out(at(60)), [latte], "the espresso is back at 9:00");
    // The latte put back: nothing sold out, and the version moved.
    append(&mut store, vec![domain_draft(latte.cast(), &change(true, None))], at(1));
    assert_eq!(store.availability_version().unwrap(), 3);
    assert_eq!(store.sold_out(at(90)).unwrap(), []);
    store.rebuild_projections().unwrap();
    assert_eq!(store.availability(espresso).unwrap(), Some(summary));
    assert_eq!(store.availability_version().unwrap(), 3);
}

/// Kitchen tickets are projected (ADR-0034): the open ones, a day's, an order's, those whose
/// fallback is due, the rerouted, and a version that moves with every event.
#[test]
fn kitchen_tickets_are_projected_and_found() {
    use keel_domain::kitchen::{KitchenEvent, KitchenItem, KitchenTicket, RerouteTo, TicketSent};
    use keel_types::Timestamp;
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let at =
        |seconds: i64| Timestamp::from_micros(1_794_560_400_000_000 + seconds * 1_000_000).unwrap();
    let sent = |station: u64| {
        KitchenEvent::Sent(TicketSent {
            order: id(0x800),
            station: id(station),
            mode: Mode::DineIn,
            items: vec![KitchenItem {
                line: id(0x900),
                name: Name::new("Latte").unwrap(),
                quantity: Quantity::from_whole(1, Unit::Each).unwrap(),
                modifiers: Vec::new(),
                notes: None,
                allergens: std::collections::BTreeSet::new(),
                component_of: None,
            }],
        })
    };
    let (bar, kitchen): (Id<KitchenTicket>, Id<KitchenTicket>) = (id(0xA01), id(0xA02));
    assert_eq!(store.kitchen_version().unwrap(), 0);
    append(
        &mut store,
        vec![domain_draft(bar.cast(), &sent(0x701)), domain_draft(kitchen.cast(), &sent(0x702))],
        at(0),
    );
    let ids = |tickets: Vec<KitchenSummary>| {
        tickets.into_iter().map(|ticket| ticket.id).collect::<Vec<_>>()
    };
    assert_eq!(ids(store.open_kitchen_tickets().unwrap()), [bar, kitchen]);
    let summary = store.kitchen_ticket(bar).unwrap().unwrap();
    assert_eq!(summary.station, Some(id(0x701)));
    assert_eq!(summary.order, Some(id(0x800)));
    assert_eq!(summary.sent.map(|(when, _)| when), Some(at(0)));
    let device = summary.sent.unwrap().1;
    // The bar's screen shows its ticket; neither is due its fallback until 10 s on.
    append(&mut store, vec![domain_draft(bar.cast(), &KitchenEvent::Shown)], at(2));
    assert_eq!(store.kitchen_ticket(bar).unwrap().unwrap().shown_at, Some(at(2)));
    assert_eq!(ids(store.unshown_kitchen_tickets(device, at(-1)).unwrap()), []);
    assert_eq!(ids(store.unshown_kitchen_tickets(device, at(0)).unwrap()), [kitchen]);
    assert_eq!(
        ids(store.unshown_kitchen_tickets(id(0xD9), at(10)).unwrap()),
        [],
        "another device's"
    );
    // The kitchen's is rerouted, the bar's bumped.
    append(
        &mut store,
        vec![
            domain_draft(kitchen.cast(), &KitchenEvent::Rerouted(RerouteTo::StationPrinter)),
            domain_draft(bar.cast(), &KitchenEvent::Bumped),
        ],
        at(10),
    );
    assert_eq!(ids(store.unshown_kitchen_tickets(device, at(10)).unwrap()), []);
    let rerouted = store.rerouted_kitchen_tickets(at(5)).unwrap();
    assert_eq!(
        rerouted.iter().map(|ticket| (ticket.id, ticket.rerouted)).collect::<Vec<_>>(),
        [(kitchen, Some((RerouteTo::StationPrinter, at(10))))]
    );
    assert_eq!(ids(store.rerouted_kitchen_tickets(at(11)).unwrap()), []);
    assert_eq!(ids(store.open_kitchen_tickets().unwrap()), [kitchen]);
    let bumped = store.kitchen_ticket(bar).unwrap().unwrap();
    assert!(bumped.bumped);
    assert_eq!(bumped.bumped_at, Some(at(10)));
    // Recalled, it is open again; the day's and the order's tickets are both.
    append(&mut store, vec![domain_draft(bar.cast(), &KitchenEvent::Recalled)], at(12));
    assert_eq!(ids(store.open_kitchen_tickets().unwrap()), [bar, kitchen]);
    let date = summary.business_date.unwrap();
    assert_eq!(ids(store.day_kitchen_tickets(date).unwrap()), [bar, kitchen]);
    assert_eq!(ids(store.order_kitchen_tickets(id(0x800)).unwrap()), [bar, kitchen]);
    assert_eq!(ids(store.order_kitchen_tickets(id(0x801)).unwrap()), []);
    assert_eq!(store.kitchen_version().unwrap(), 6);
    let before = store.kitchen_ticket(bar).unwrap();
    store.rebuild_projections().unwrap();
    assert_eq!(store.kitchen_ticket(bar).unwrap(), before);
    assert_eq!(store.kitchen_version().unwrap(), 6);
}
