//! Property tests for projections, against a model of what they should hold.
//!
//! A case works on two orders and two payments. The store's device appends events to them, and
//! two other devices wrote logs of their own, which the store receives a few events at a time, so
//! events often arrive after others with later HLCs. Writes commit or fail; the store is reopened
//! and its projections rebuilt along the way. After every step, each order's and payment's row is
//! checked against the model: the fold, written here independently of the store's, of the
//! stream's stored events in canonical order. So are the lists by state and by order, and the
//! streams each write reports it touched.
//!
//! And stores fed the same events in different orders, in different writes, some of which fail,
//! end with byte-identical projections: the simulator's convergence invariant.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_domain::aggregate::fold;
use keel_domain::codec::IdSet;
use keel_domain::order::{Epoch, Lease, Order, OrderEvent, OrderStatus, OwnershipGranted, Refusal};
use keel_domain::payment::{
    Payment, PaymentAuthorized, PaymentCaptured, PaymentEnded, PaymentEvent, PaymentInitiated,
    PaymentStatus, Tender,
};
use keel_events::cbor::Value;
use keel_events::envelope::{Aggregate, Payload, SchemaName, SchemaRef, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::log::{EventDraft, LogConfig, LogHead, LogWriter};
use keel_store::{
    OrderState, OrderSummary, PaymentState, PaymentSummary, Received, Store, StoreError,
};
use keel_types::{Hlc, Id, SeededEntropy};
use proptest::prelude::*;
use support::{
    DRIFT, OWN, PEERS, REPLICA, Scratch, at, check_closed, config_of, created, device,
    domain_draft, here, id, line_added, projection_rows, reason, registry, signer, store_key, usd,
};

// ---------------------------------------------------------------------------------------------
// What a case does.

const ORDERS: [u64; 2] = [0x7001, 0x7002];
const PAYMENTS: [u64; 2] = [0x8001, 0x8002];

fn order(n: usize) -> Id<Order> {
    id(ORDERS[n])
}

fn payment(n: usize) -> Id<Payment> {
    id(PAYMENTS[n])
}

/// A check of order `n`: 0 is its main check, whose identifier is the order's; 1 and 2 are
/// checks a device may open.
fn check(n: usize, c: u8) -> Id<keel_domain::order::Check> {
    if c == 0 { order(n).cast() } else { id(0x2000 + 0x10 * ORDERS[n] + u64::from(c)) }
}

fn line(n: usize, l: u8) -> Id<keel_domain::order::Line> {
    id(0x1000 + 0x10 * ORDERS[n] + u64::from(l))
}

/// An event on an order, by how to make it.
#[derive(Clone, Debug)]
enum OrderSpec {
    Created {
        table: u8,
        guests: u8,
    },
    LineAdded(u8),
    LineRemoved(u8),
    Fired(u8),
    LineVoided(u8),
    CheckOpened(u8),
    CheckClosed {
        check: u8,
        line: u8,
        paid_by: usize,
    },
    Closed,
    Voided,
    Abandoned,
    Reopened,
    /// Ownership (ADR-0021): a request or an override from a lease, a grant of a lease, or a
    /// refusal, answering a request that may not exist.
    Requested(u8),
    Overridden(u8),
    Granted(u8),
    Refused,
    /// An event of a schema this kernel doesn't know: a newer kernel wrote it.
    Unknown,
}

/// An event on a payment, by how to make it.
#[derive(Clone, Debug)]
enum PaymentSpec {
    Initiated { order: usize, check: u8, amount: u16, card: bool },
    Authorized,
    Captured { tip: bool },
    Failed,
    Voided,
}

#[derive(Clone, Debug)]
enum Spec {
    Order(usize, OrderSpec),
    Payment(usize, PaymentSpec),
}

fn any_order_spec() -> impl Strategy<Value = OrderSpec> {
    let l = 0_u8..3;
    prop_oneof![
        3 => (0_u8..3, 1_u8..5).prop_map(|(table, guests)| OrderSpec::Created { table, guests }),
        4 => l.clone().prop_map(OrderSpec::LineAdded),
        1 => l.clone().prop_map(OrderSpec::LineRemoved),
        2 => l.clone().prop_map(OrderSpec::Fired),
        1 => l.clone().prop_map(OrderSpec::LineVoided),
        1 => (1_u8..3).prop_map(OrderSpec::CheckOpened),
        2 => (0_u8..3, l, 0_usize..2)
            .prop_map(|(check, line, paid_by)| OrderSpec::CheckClosed { check, line, paid_by }),
        1 => Just(OrderSpec::Closed),
        1 => Just(OrderSpec::Voided),
        1 => Just(OrderSpec::Abandoned),
        1 => Just(OrderSpec::Reopened),
        1 => (0_u8..3).prop_map(OrderSpec::Requested),
        1 => (0_u8..3).prop_map(OrderSpec::Overridden),
        1 => (1_u8..4).prop_map(OrderSpec::Granted),
        1 => Just(OrderSpec::Refused),
        1 => Just(OrderSpec::Unknown),
    ]
}

fn any_payment_spec() -> impl Strategy<Value = PaymentSpec> {
    prop_oneof![
        3 => (0_usize..2, 0_u8..3, 1_u16..1_000, any::<bool>())
            .prop_map(|(order, check, amount, card)| PaymentSpec::Initiated { order, check, amount, card }),
        1 => Just(PaymentSpec::Authorized),
        2 => any::<bool>().prop_map(|tip| PaymentSpec::Captured { tip }),
        1 => Just(PaymentSpec::Failed),
        1 => Just(PaymentSpec::Voided),
    ]
}

fn any_spec() -> impl Strategy<Value = (Spec, u8)> {
    let spec = prop_oneof![
        3 => (0_usize..2, any_order_spec()).prop_map(|(n, spec)| Spec::Order(n, spec)),
        1 => (0_usize..2, any_payment_spec()).prop_map(|(n, spec)| Spec::Payment(n, spec)),
    ];
    (spec, 0_u8..2)
}

/// The draft of the event `spec` describes, reported on business day `day`.
fn draft(spec: &Spec, day: u8) -> EventDraft {
    match spec {
        Spec::Order(n, spec) => {
            let n = *n;
            let event = match spec {
                OrderSpec::Created { table, guests } => {
                    created(0x200 + u64::from(*table), u16::from(*guests))
                }
                OrderSpec::LineAdded(l) => line_added(line(n, *l), 450),
                OrderSpec::LineRemoved(l) => OrderEvent::LineRemoved { line: line(n, *l) },
                OrderSpec::Fired(l) => {
                    OrderEvent::LinesFired { lines: IdSet::new([line(n, *l)]).unwrap() }
                }
                OrderSpec::LineVoided(l) => {
                    OrderEvent::LineVoided { line: line(n, *l), reason: reason("mistake") }
                }
                OrderSpec::CheckOpened(c) => OrderEvent::CheckOpened { check: check(n, *c) },
                OrderSpec::CheckClosed { check: c, line: l, paid_by } => {
                    check_closed(check(n, *c), line(n, *l), 450, payment(*paid_by))
                }
                OrderSpec::Closed => OrderEvent::Closed,
                OrderSpec::Voided => OrderEvent::Voided { reason: reason("mistake") },
                OrderSpec::Abandoned => OrderEvent::Abandoned,
                OrderSpec::Reopened => OrderEvent::Reopened { reason: reason("mistake") },
                OrderSpec::Requested(n) => {
                    OrderEvent::OwnershipRequested { lease: Lease::new(u64::from(*n)).unwrap() }
                }
                OrderSpec::Overridden(n) => OrderEvent::OwnershipOverridden {
                    lease: Lease::new(u64::from(*n)).unwrap(),
                    reason: reason("island"),
                },
                OrderSpec::Granted(n) => OrderEvent::OwnershipGranted(OwnershipGranted {
                    request: id(0xE1),
                    device: device(PEERS[0]),
                    lease: Lease::new(u64::from(*n)).unwrap(),
                    epoch: Epoch::new(1).unwrap(),
                }),
                OrderSpec::Refused => {
                    OrderEvent::OwnershipRefused { request: id(0xE1), refusal: Refusal::LeaseMoved }
                }
                OrderSpec::Unknown => {
                    let mut draft = domain_draft(order(n).cast(), &OrderEvent::Closed, day);
                    draft.schema = SchemaRef {
                        name: SchemaName::new("order.noted").unwrap(),
                        version: core::num::NonZeroU32::MIN,
                    };
                    draft.payload = Payload::new(&Value::Unsigned(1)).unwrap();
                    return draft;
                }
            };
            domain_draft(order(n).cast(), &event, day)
        }
        Spec::Payment(n, spec) => {
            let event = match spec {
                PaymentSpec::Initiated { order: o, check: c, amount, card } => {
                    PaymentEvent::Initiated(PaymentInitiated {
                        order: order(*o),
                        check: check(*o, *c),
                        tender: if *card { Tender::Card } else { Tender::Cash },
                        amount: usd(i64::from(*amount)),
                    })
                }
                PaymentSpec::Authorized => PaymentEvent::Authorized(PaymentAuthorized {
                    amount: usd(300),
                    reference: None,
                }),
                PaymentSpec::Captured { tip } => PaymentEvent::Captured(PaymentCaptured {
                    amount: usd(300),
                    tip: tip.then(|| usd(50)),
                    reference: None,
                    cash: None,
                }),
                PaymentSpec::Failed => PaymentEvent::Failed(PaymentEnded {
                    reason: reason("declined"),
                    reference: None,
                }),
                PaymentSpec::Voided => PaymentEvent::Voided(PaymentEnded {
                    reason: reason("cancelled"),
                    reference: None,
                }),
            };
            domain_draft(payment(*n).cast(), &event, day)
        }
    }
}

/// The logs of the two other devices: each event made at its own time on its own clock.
fn peer_logs(specs: &[Vec<(Spec, u8)>; 2]) -> [Vec<SignedEvent>; 2] {
    core::array::from_fn(|p| {
        let n = PEERS[p];
        let config = LogConfig {
            device: device(n),
            location: here(),
            head: LogHead::EMPTY,
            latest_hlc: Hlc::ZERO,
            max_forward_drift: DRIFT,
        };
        let mut writer = LogWriter::new(config, signer(n), SeededEntropy::new(u64::from(n)));
        specs[p]
            .iter()
            .enumerate()
            .map(|(k, (spec, day))| {
                let now = at(20 + 60 * i64::try_from(k).unwrap() + 7 * i64::try_from(p).unwrap());
                writer.prepare(draft(spec, *day), now).unwrap().commit()
            })
            .collect()
    })
}

#[derive(Clone, Debug)]
enum Step {
    /// The store's device appends an event.
    Append(Spec, u8),
    /// The store receives the next event of another device's log.
    Deliver(usize),
}

#[derive(Clone, Debug)]
enum Op {
    /// A write's steps, and whether it commits.
    Write(Vec<Step>, bool),
    Reopen,
    Rebuild,
}

fn any_op() -> impl Strategy<Value = Op> {
    let step = prop_oneof![
        2 => any_spec().prop_map(|(spec, day)| Step::Append(spec, day)),
        3 => (0_usize..2).prop_map(Step::Deliver),
    ];
    prop_oneof![
        10 => (prop::collection::vec(step, 1..6), prop::bool::weighted(0.85))
            .prop_map(|(steps, commits)| Op::Write(steps, commits)),
        1 => Just(Op::Reopen),
        1 => Just(Op::Rebuild),
    ]
}

fn any_peer_specs() -> impl Strategy<Value = [Vec<(Spec, u8)>; 2]> {
    let log = || prop::collection::vec(any_spec(), 0..14);
    (log(), log()).prop_map(|(a, b)| [a, b])
}

// ---------------------------------------------------------------------------------------------
// The model: the folds, and the rows they make, written from the domain's own accessors.

/// The stream `stream`'s events among `events`, in canonical order.
fn stream_of(events: &[SignedEvent], stream: Id<Aggregate>) -> Vec<SignedEvent> {
    let mut of: Vec<SignedEvent> =
        events.iter().filter(|event| event.body().stream.id == stream).cloned().collect();
    of.sort_by_key(|event| {
        let body = event.body();
        (body.hlc, body.origin_device, body.origin_seq)
    });
    of
}

fn business_date_of(
    events: &[SignedEvent],
    id: Id<keel_events::envelope::Event>,
) -> Option<keel_types::BusinessDate> {
    events.iter().find(|event| event.body().event_id == id).map(|event| event.body().business_date)
}

fn expected_order(events: &[SignedEvent], id: Id<Order>) -> Option<OrderSummary> {
    let events = stream_of(events, id.cast());
    let (first, last) = (events.first()?.body().hlc, events.last()?.body().hlc);
    let mut order = Order::new(id);
    for event in &events {
        fold(&mut order, event);
    }
    let state = match (order.info(), order.status()) {
        (None, _) => OrderState::Uncreated,
        (Some(_), OrderStatus::Active) => OrderState::Active,
        (Some(_), OrderStatus::Closed) => OrderState::Closed,
        (Some(_), OrderStatus::Voided(_)) => OrderState::Voided,
        (Some(_), OrderStatus::Abandoned) => OrderState::Abandoned,
    };
    let closed = order.checks().iter().filter(|check| check.closed().is_some()).count();
    Some(OrderSummary {
        id,
        state,
        stage: order.stage(),
        info: order.info().cloned(),
        business_date: order.info().and_then(|info| business_date_of(&events, info.created_by)),
        live_lines: u64::try_from(order.live_lines().count()).unwrap(),
        open_checks: u64::try_from(order.checks().len() - closed).unwrap(),
        closed_checks: u64::try_from(closed).unwrap(),
        conflicts: u64::try_from(order.conflicts().len()).unwrap(),
        unreadable: u64::try_from(order.skipped().len()).unwrap(),
        events: u64::try_from(events.len()).unwrap(),
        first,
        last,
        ownership: order.ownership(),
        requests: u64::try_from(order.requests().len()).unwrap(),
    })
}

fn expected_payment(events: &[SignedEvent], id: Id<Payment>) -> Option<PaymentSummary> {
    let events = stream_of(events, id.cast());
    let (first, last) = (events.first()?.body().hlc, events.last()?.body().hlc);
    let mut payment = Payment::new(id);
    for event in &events {
        fold(&mut payment, event);
    }
    let state = match (payment.info(), payment.status()) {
        (None, _) => PaymentState::Uninitiated,
        (Some(_), PaymentStatus::Initiated) => PaymentState::Initiated,
        (Some(_), PaymentStatus::Authorized(_)) => PaymentState::Authorized,
        (Some(_), PaymentStatus::Captured(_)) => PaymentState::Captured,
        (Some(_), PaymentStatus::Failed(_)) => PaymentState::Failed,
        (Some(_), PaymentStatus::Voided(_)) => PaymentState::Voided,
    };
    let captured = match payment.status() {
        PaymentStatus::Captured(captured) if payment.info().is_some() => Some(captured),
        _ => None,
    };
    Some(PaymentSummary {
        id,
        state,
        info: payment.info().cloned(),
        business_date: payment.info().and_then(|info| business_date_of(&events, info.initiated_by)),
        captured: captured.map(|captured| captured.amount),
        tip: captured.and_then(|captured| captured.tip),
        conflicts: u64::try_from(payment.conflicts().len()).unwrap(),
        unreadable: u64::try_from(payment.skipped().len()).unwrap(),
        events: u64::try_from(events.len()).unwrap(),
        first,
        last,
    })
}

/// Checks every projection the store reads back against the model's stored `events`.
fn agrees(store: &TestStore, events: &[SignedEvent]) -> Result<(), TestCaseError> {
    let orders: Vec<OrderSummary> =
        (0..2).filter_map(|n| expected_order(events, order(n))).collect();
    let payments: Vec<PaymentSummary> =
        (0..2).filter_map(|n| expected_payment(events, payment(n))).collect();
    for n in 0..2 {
        prop_assert_eq!(store.order(order(n)).unwrap(), expected_order(events, order(n)));
        prop_assert_eq!(store.payment(payment(n)).unwrap(), expected_payment(events, payment(n)));
        // Loading an aggregate folds its stream in canonical order.
        let mut folded = Order::new(order(n));
        for event in stream_of(events, order(n).cast()) {
            fold(&mut folded, &event);
        }
        prop_assert_eq!(store.load(Order::new(order(n))).unwrap(), folded);
        let mut folded = Payment::new(payment(n));
        for event in stream_of(events, payment(n).cast()) {
            fold(&mut folded, &event);
        }
        prop_assert_eq!(store.load(Payment::new(payment(n))).unwrap(), folded);
        let mut of: Vec<PaymentSummary> = payments
            .iter()
            .filter(|summary| summary.info.as_ref().is_some_and(|info| info.order == order(n)))
            .cloned()
            .collect();
        of.sort_by_key(|summary| (summary.first, summary.id.to_bytes()));
        prop_assert_eq!(store.payments_of(order(n)).unwrap(), of);
    }
    for state in OrderState::ALL {
        let mut in_state: Vec<OrderSummary> =
            orders.iter().filter(|summary| summary.state == state).cloned().collect();
        in_state.sort_by_key(|summary| (summary.first, summary.id.to_bytes()));
        prop_assert_eq!(store.orders(state).unwrap(), in_state, "orders {:?}", state);
    }
    prop_assert_eq!(store.order(id(0x7FFF)).unwrap(), None);
    prop_assert_eq!(store.payment(id(0x8FFF)).unwrap(), None);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Running a case.

type TestStore = Store<keel_events::keys::SoftwareSigner, SeededEntropy>;

#[derive(Debug)]
enum WriteError {
    Store(StoreError),
    Failed,
}

impl From<StoreError> for WriteError {
    fn from(error: StoreError) -> WriteError {
        WriteError::Store(error)
    }
}

/// The streams of `events`, in the order they first appear.
fn first_touched(events: &[SignedEvent]) -> Vec<StreamRef> {
    let mut streams: Vec<StreamRef> = Vec::new();
    for event in events {
        if !streams.contains(&event.body().stream) {
            streams.push(event.body().stream.clone());
        }
    }
    streams
}

fn open(scratch: &Scratch, n: u8, seed: u64) -> TestStore {
    Store::open(scratch.db(), store_key(), config_of(n), signer(n), SeededEntropy::new(seed))
        .unwrap()
}

proptest! {
    /// Every write, rollback, reopening and rebuild leaves each projection exactly as the model's
    /// fold of the stored events says, whatever order they arrived in.
    #[test]
    fn projections_follow_the_stored_events(
        peer_specs in any_peer_specs(),
        ops in prop::collection::vec(any_op(), 1..12),
    ) {
        let logs = peer_logs(&peer_specs);
        let registry = registry();
        let scratch = Scratch::new("projections");
        let mut store = open(&scratch, OWN, 1);
        let mut stored: Vec<SignedEvent> = Vec::new();
        let mut delivered = [0_usize; 2];
        let mut now_ms = 0_i64;
        let mut reopened = 0;
        for op in &ops {
            match op {
                Op::Write(steps, commits) => {
                    now_ms += 50;
                    let now = at(now_ms);
                    let mut next = delivered;
                    let written = store.write(|w| {
                        let mut events = Vec::new();
                        for step in steps {
                            match step {
                                Step::Append(spec, day) => events.push(w.append(draft(spec, *day), now)?),
                                Step::Deliver(p) => {
                                    let Some(event) = logs[*p].get(next[*p]) else { continue };
                                    match w.receive(&event.to_bytes(), &registry, now)? {
                                        Received::Stored(event) => events.push(*event),
                                        other => panic!("delivering {event:?}: {other:?}"),
                                    }
                                    next[*p] += 1;
                                }
                            }
                        }
                        let touched = w.touched().to_vec();
                        if !*commits {
                            return Err(WriteError::Failed);
                        }
                        Ok((events, touched))
                    });
                    match written {
                        Ok((events, touched)) => {
                            prop_assert_eq!(touched, first_touched(&events));
                            stored.extend(events);
                            delivered = next;
                        }
                        Err(WriteError::Failed) => prop_assert!(!*commits),
                        Err(WriteError::Store(error)) => prop_assert!(false, "store error: {error:?}"),
                    }
                }
                Op::Reopen => {
                    reopened += 1;
                    drop(store);
                    store = open(&scratch, OWN, 100 + reopened);
                }
                Op::Rebuild => {
                    let before = projection_rows(&scratch, 0x4B);
                    store.rebuild_projections().unwrap();
                    prop_assert_eq!(projection_rows(&scratch, 0x4B), before, "a rebuild changed the projections");
                }
            }
            agrees(&store, &stored)?;
        }
    }

    /// Two stores that receive the same events, each in its own order and its own writes, some of
    /// them failing, hold byte-identical projections, and the model's.
    #[test]
    fn stores_fed_the_same_events_converge(
        peer_specs in any_peer_specs(),
        schedules in prop::array::uniform2(prop::collection::vec((0_usize..2, 1_usize..4, prop::bool::weighted(0.85)), 1..40)),
    ) {
        let logs = peer_logs(&peer_specs);
        let registry = registry();
        let mut dumps = Vec::new();
        let all: Vec<SignedEvent> = logs.iter().flatten().cloned().collect();
        for (replica, schedule) in [OWN, REPLICA].into_iter().zip(&schedules) {
            let scratch = Scratch::new("converge");
            let mut store = open(&scratch, replica, 7);
            let mut delivered = [0_usize; 2];
            let mut now_ms = 1_000_i64;
            // Deliver in the order the schedule picks, a few at a time, until both logs are in.
            // After two failed writes in a row, the next commits, so that every case ends.
            let mut picks = schedule.iter().cycle();
            let mut failed = 0;
            while delivered[0] < logs[0].len() || delivered[1] < logs[1].len() {
                let &(first, count, commits) = picks.next().unwrap();
                let commits = commits || failed >= 2;
                now_ms += 10;
                let mut next = delivered;
                let written = store.write(|w| {
                    for k in 0..count {
                        // Alternate from the peer picked, taking whichever still has events.
                        let p = (first + k) % 2;
                        let p = if next[p] < logs[p].len() { p } else { 1 - p };
                        let Some(event) = logs[p].get(next[p]) else { break };
                        w.receive(&event.to_bytes(), &registry, at(now_ms))?;
                        next[p] += 1;
                    }
                    if commits { Ok(()) } else { Err(WriteError::Failed) }
                });
                match written {
                    Ok(()) => {
                        delivered = next;
                        failed = 0;
                    }
                    Err(WriteError::Failed) => failed += 1,
                    Err(WriteError::Store(error)) => prop_assert!(false, "store error: {error:?}"),
                }
            }
            agrees(&store, &all)?;
            dumps.push(projection_rows(&scratch, 0x4B));
        }
        prop_assert_eq!(&dumps[0], &dumps[1]);
    }
}
