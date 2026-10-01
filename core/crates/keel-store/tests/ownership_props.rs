//! Property tests for the hub's answers to requests for orders (ADR-0021), against a model.
//!
//! Two devices write logs about two orders, each creating one, on a business date of its own:
//! requests for either order from a lease, overrides, payments started on either and their
//! outcomes, and lines added. The hub, which claimed the role in an epoch of its own, succeeding
//! a former hub's claims (ADR-0022), takes them in a few at a time, makes requests of its own,
//! and answers whatever waits, now and then interrupted as it commits. At every answer, what the
//! hub writes must be the model's:
//! for each order with requests waiting, by identifier, the order's own rules
//! ([`Order::answers`]) applied to the order as the hub holds it, with a payment in progress
//! when the hub holds one of the order's payments started and with no outcome. Each answer is
//! on the order's stream, recorded by the hub, caused by its request, under the order's
//! business date; afterwards no request waits. At the end, every request the hub holds has
//! exactly one answer, and another replica taking in every log in any order holds the same
//! owners, leases and waiting requests.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU8;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use keel_domain::aggregate::fold;
use keel_domain::hub::{Claimed, Cut, HubEvent, Succession};
use keel_domain::order::{Epoch, Lease, Order, OrderEvent};
use keel_domain::payment::{
    Payment, PaymentAuthorized, PaymentCaptured, PaymentEvent, PaymentInitiated, PaymentStatus,
    Tender,
};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{Actor, Component, Event};
use keel_events::event::SignedEvent;
use keel_events::keys::SoftwareSigner;
use keel_events::log::{LogConfig, LogHead, LogWriter};
use keel_store::{Faults, Point, Received, Store, StoreError};
use keel_types::{Hlc, Id, SeededEntropy};
use proptest::prelude::*;
use support::{
    DRIFT, FORMER, OWN, PEERS, REPLICA, Scratch, at, config_of, created, device, domain_draft,
    here, id, line_added, reason, registry, signer, store_key, usd,
};

type TestStore = Store<SoftwareSigner, SeededEntropy>;

/// The order device `PEERS[n]` creates.
fn order(n: usize) -> Id<Order> {
    id(0x7000 + u64::try_from(n).unwrap())
}

/// Refuses the write in progress as it is about to commit, once, when armed.
#[derive(Clone, Default)]
struct Plan(Arc<Mutex<bool>>);

impl Faults for Plan {
    fn proceed(&mut self, point: Point) -> bool {
        let mut armed = self.0.lock().unwrap();
        if point == Point::Committing && *armed {
            *armed = false;
            return false;
        }
        true
    }
}

/// An event of a device's log about an order: a request or an override from a lease, a payment
/// started on the order, the outcome of the device's last payment, or a line added.
#[derive(Clone, Debug)]
enum Spec {
    Request { order: usize, lease: u8 },
    Override { order: usize, lease: u8 },
    Pay { order: usize },
    Authorize,
    Capture,
    Line { order: usize },
}

fn any_spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        4 => (0_usize..2, 0_u8..3).prop_map(|(order, lease)| Spec::Request { order, lease }),
        1 => (0_usize..2, 0_u8..3).prop_map(|(order, lease)| Spec::Override { order, lease }),
        2 => (0_usize..2).prop_map(|order| Spec::Pay { order }),
        1 => Just(Spec::Authorize),
        1 => Just(Spec::Capture),
        1 => (0_usize..2).prop_map(|order| Spec::Line { order }),
    ]
}

/// A step at the hub.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// Takes in the next `count` events of device `PEERS[peer]`'s log, in one write.
    Receive { peer: usize, count: usize },
    /// Asks for order `order`, from lease `lease`.
    Ask { order: usize, lease: u8 },
    /// Answers what waits; interrupted as it commits, if so.
    Answer { interrupted: bool },
}

fn any_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        5 => (0_usize..2, 1_usize..4).prop_map(|(peer, count)| Step::Receive { peer, count }),
        1 => (0_usize..2, 0_u8..3).prop_map(|(order, lease)| Step::Ask { order, lease }),
        3 => Just(Step::Answer { interrupted: false }),
        1 => Just(Step::Answer { interrupted: true }),
    ]
}

/// Device `PEERS[p]`'s log: the creation of its order, on day `p`, then its specs, each a minute
/// apart on its own clock.
fn peer_log(p: usize, specs: &[Spec]) -> Vec<SignedEvent> {
    let n = PEERS[p];
    let config = LogConfig {
        device: device(n),
        location: here(),
        head: LogHead::EMPTY,
        latest_hlc: Hlc::ZERO,
        max_forward_drift: DRIFT,
    };
    let mut writer = LogWriter::new(config, signer(n), SeededEntropy::new(u64::from(n)));
    let mut payments: Vec<Id<Payment>> = Vec::new();
    let day = u8::try_from(p).unwrap();
    let mut drafts = vec![domain_draft(order(p).cast(), &created(0, 2), day)];
    for (k, spec) in specs.iter().enumerate() {
        let lease = |n: u8| Lease::new(u64::from(n)).unwrap();
        let draft = match *spec {
            Spec::Request { order: o, lease: n } => domain_draft(
                order(o).cast(),
                &OrderEvent::OwnershipRequested { lease: lease(n) },
                0,
            ),
            Spec::Override { order: o, lease: n } => domain_draft(
                order(o).cast(),
                &OrderEvent::OwnershipOverridden { lease: lease(n), reason: reason("island") },
                0,
            ),
            Spec::Pay { order: o } => {
                let payment: Id<Payment> =
                    id(0x8000 + (u64::from(n) << 8) + u64::try_from(k).unwrap());
                payments.push(payment);
                let initiated = PaymentEvent::Initiated(PaymentInitiated {
                    order: order(o),
                    check: order(o).cast(),
                    tender: Tender::Card,
                    amount: usd(450),
                });
                domain_draft(payment.cast(), &initiated, 0)
            }
            Spec::Authorize | Spec::Capture => {
                let Some(&payment) = payments.last() else { continue };
                let outcome = if matches!(spec, Spec::Authorize) {
                    PaymentEvent::Authorized(PaymentAuthorized {
                        amount: usd(450),
                        reference: None,
                    })
                } else {
                    PaymentEvent::Captured(PaymentCaptured {
                        amount: usd(450),
                        tip: None,
                        reference: None,
                        cash: None,
                    })
                };
                domain_draft(payment.cast(), &outcome, 0)
            }
            Spec::Line { order: o } => {
                let line = id(0x9000 + (u64::from(n) << 8) + u64::try_from(k).unwrap());
                domain_draft(order(o).cast(), &line_added(line, 450), 0)
            }
        };
        drafts.push(draft);
    }
    drafts
        .into_iter()
        .enumerate()
        .map(|(k, draft)| {
            let now = at(20 + 60 * i64::try_from(k).unwrap() + 7 * i64::try_from(p).unwrap());
            writer.prepare(draft, now).unwrap().commit()
        })
        .collect()
}

/// The events of `stream` among `held`, in canonical order.
fn stream_of(
    held: &[SignedEvent],
    stream: Id<keel_events::envelope::Aggregate>,
) -> Vec<SignedEvent> {
    let mut of: Vec<SignedEvent> =
        held.iter().filter(|event| event.body().stream.id == stream).cloned().collect();
    of.sort_by_key(|event| {
        let body = event.body();
        (body.hlc, body.origin_device, body.origin_seq)
    });
    of
}

/// Order `id` folded from the events `held`.
fn folded(held: &[SignedEvent], id: Id<Order>) -> Order {
    let mut order = Order::new(id);
    for event in stream_of(held, id.cast()) {
        fold(&mut order, &event);
    }
    order
}

/// Whether a payment of order `id` is in progress among `held`: started, with no outcome yet.
fn paying(held: &[SignedEvent], id: Id<Order>) -> bool {
    let mut payments: BTreeMap<[u8; 16], Payment> = BTreeMap::new();
    for event in held.iter().filter(|event| event.body().stream.kind.as_str() == "payment") {
        let stream = event.body().stream.id;
        payments.entry(stream.to_bytes()).or_insert_with(|| Payment::new(stream.cast()));
    }
    payments.values_mut().any(|payment| {
        for event in stream_of(held, payment.id().cast()) {
            fold(payment, &event);
        }
        payment.info().is_some_and(|info| info.order == id)
            && matches!(payment.status(), PaymentStatus::Initiated)
    })
}

/// The answers the hub must write in `epoch`, holding `held`: for each order with requests
/// waiting, by identifier, what its rules answer.
fn expected_answers(held: &[SignedEvent], epoch: u64) -> Vec<(Id<Order>, OrderEvent)> {
    let mut orders: Vec<Id<Order>> = held
        .iter()
        .filter(|event| event.body().stream.kind.as_str() == "order")
        .map(|event| event.body().stream.id.cast())
        .collect();
    orders.sort_by_key(|order| order.to_bytes());
    orders.dedup();
    let epoch = Epoch::new(epoch).unwrap();
    orders
        .into_iter()
        .flat_map(|id| {
            let order = folded(held, id);
            order.answers(paying(held, id), epoch).into_iter().map(move |answer| (id, answer))
        })
        .collect()
}

fn decoded(event: &SignedEvent) -> OrderEvent {
    let body = event.body();
    OrderEvent::decode(&body.schema, &body.payload).unwrap()
}

/// The request an answer names.
fn answered(event: &OrderEvent) -> Option<Id<Event>> {
    match event {
        OrderEvent::OwnershipGranted(granted) => Some(granted.request),
        OrderEvent::OwnershipRefused { request, .. } => Some(*request),
        _ => None,
    }
}

fn open(scratch: &Scratch, n: u8, plan: &Plan) -> TestStore {
    Store::open_with_faults(
        scratch.db(),
        store_key(),
        config_of(n),
        signer(n),
        SeededEntropy::new(u64::from(n)),
        Box::new(plan.clone()),
    )
    .unwrap()
}

fn receive(store: &mut TestStore, events: &[SignedEvent], now: i64) {
    store
        .write(|w| {
            for event in events {
                let received = w.receive(&event.to_bytes(), &registry(), at(now))?;
                assert!(matches!(received, Received::Stored(_)), "{received:?}");
            }
            Ok::<_, StoreError>(())
        })
        .unwrap();
}

/// The claims of a former hub that the hub succeeds to claim `epoch`: none for epoch 1; a first
/// claim for epoch 2; and for a later one, a first claim and a claim of the epoch before it.
fn former_claims(epoch: u64) -> Vec<SignedEvent> {
    let config = LogConfig {
        device: device(FORMER),
        location: here(),
        head: LogHead::EMPTY,
        latest_hlc: Hlc::ZERO,
        max_forward_drift: DRIFT,
    };
    let mut writer = LogWriter::new(config, signer(FORMER), SeededEntropy::new(7));
    let mut claims: Vec<SignedEvent> = Vec::new();
    let epochs: Vec<u64> = match epoch {
        1 => Vec::new(),
        2 => vec![1],
        _ => vec![1, epoch - 1],
    };
    for (k, of) in epochs.into_iter().enumerate() {
        let succeeds = claims.last().map(|previous| Succession {
            previous: previous.body().event_id,
            cuts: vec![Cut { device: device(FORMER), position: 1 }],
        });
        let claimed = Claimed::new(Epoch::new(of).unwrap(), NonZeroU8::MIN, succeeds).unwrap();
        let draft =
            domain_draft(id(0x9500 + u64::try_from(k).unwrap()), &HubEvent::Claimed(claimed), 0);
        claims.push(writer.prepare(draft, at(100)).unwrap().commit());
    }
    claims
}

/// Checks the answers `written` in `epoch` against what the model expected, holding `held`
/// before them.
fn check_answers(
    written: &[SignedEvent],
    held: &[SignedEvent],
    epoch: u64,
) -> Result<(), TestCaseError> {
    let expected = expected_answers(held, epoch);
    let found: Vec<(Id<Order>, OrderEvent)> =
        written.iter().map(|event| (event.body().stream.id.cast(), decoded(event))).collect();
    prop_assert_eq!(&found, &expected);
    for event in written {
        let body = event.body();
        prop_assert_eq!(body.origin_device, device(OWN));
        prop_assert_eq!(body.stream.kind.as_str(), "order");
        prop_assert_eq!(body.actor.clone(), Actor::System(Component::new("hub").unwrap()));
        let request = answered(&decoded(event)).unwrap();
        prop_assert_eq!(body.causation, Some(request.cast()));
        let order = folded(held, body.stream.id.cast());
        let created_on = held
            .iter()
            .find(|held| Some(held.body().event_id) == order.info().map(|info| info.created_by))
            .map(|held| held.body().business_date);
        prop_assert_eq!(Some(body.business_date), created_on);
    }
    Ok(())
}

proptest! {
    /// The hub answers what waits, exactly as the order's rules do with what it holds; every
    /// request ends with one answer; and a replica holding the same events agrees.
    #[test]
    fn the_hub_answers_as_the_model_does(
        specs in [prop::collection::vec(any_spec(), 0..10), prop::collection::vec(any_spec(), 0..10)],
        steps in prop::collection::vec(any_step(), 1..20),
        schedule in prop::collection::vec(0_usize..4, 0..40),
        epoch in prop_oneof![1_u64..4, Just((1 << 63) - 1)],
    ) {
        let logs = [peer_log(0, &specs[0]), peer_log(1, &specs[1])];
        let (hub_scratch, replica_scratch) = (Scratch::new("ownership-hub"), Scratch::new("ownership-replica"));
        let plan = Plan::default();
        let mut hub = open(&hub_scratch, OWN, &plan);
        // The hub claims its epoch, succeeding a former hub's claims.
        let former = former_claims(epoch);
        receive(&mut hub, &former, 500);
        let claimed = hub.claim(NonZeroU8::MIN, at(600)).unwrap().unwrap();
        prop_assert_eq!(hub.term().unwrap().map(|term| term.epoch.get()), Some(epoch));
        let mut held: Vec<SignedEvent> = [former.clone(), vec![claimed]].concat();
        let mut taken = [0_usize; 2];
        for (i, step) in steps.iter().enumerate() {
            let now = 10_000 + i64::try_from(i).unwrap() * 100;
            match *step {
                Step::Receive { peer, count } => {
                    let upto = (taken[peer] + count).min(logs[peer].len());
                    let events = &logs[peer][taken[peer]..upto];
                    receive(&mut hub, events, now);
                    held.extend_from_slice(events);
                    taken[peer] = upto;
                }
                Step::Ask { order: o, lease } => {
                    let request = OrderEvent::OwnershipRequested { lease: Lease::new(u64::from(lease)).unwrap() };
                    let draft = domain_draft(order(o).cast(), &request, 0);
                    let event = hub.write(|w| w.append(draft, at(now))).unwrap();
                    held.push(event);
                }
                Step::Answer { interrupted: true } => {
                    *plan.0.lock().unwrap() = true;
                    let answered = hub.answer_requests(at(now));
                    prop_assert!(matches!(answered, Err(StoreError::Interrupted(Point::Committing))), "{:?}", answered);
                }
                Step::Answer { interrupted: false } => {
                    let written = hub.answer_requests(at(now)).unwrap();
                    check_answers(&written, &held, epoch)?;
                    held.extend(written);
                    prop_assert!(expected_answers(&held, epoch).is_empty());
                }
            }
            for n in 0..2 {
                let order = folded(&held, order(n));
                let summary = hub.order(order.id()).unwrap();
                prop_assert_eq!(summary.as_ref().and_then(|summary| summary.ownership), order.ownership());
                let waiting = u64::try_from(order.requests().len()).unwrap();
                prop_assert_eq!(summary.map_or(0, |summary| summary.requests), waiting);
            }
        }
        // The hub takes in the rest, and answers: every request it holds has one answer.
        for peer in 0..2 {
            let rest = &logs[peer][taken[peer]..];
            receive(&mut hub, rest, 20_000);
            held.extend_from_slice(rest);
        }
        let written = hub.answer_requests(at(20_500)).unwrap();
        check_answers(&written, &held, epoch)?;
        held.extend(written);
        let mut answers: BTreeMap<Id<Event>, usize> = BTreeMap::new();
        for event in held.iter().filter(|event| event.body().origin_device == device(OWN)) {
            if let Some(request) = decoded_if_order(event).as_ref().and_then(answered) {
                *answers.entry(request).or_default() += 1;
            }
        }
        for event in &held {
            if matches!(decoded_if_order(event), Some(OrderEvent::OwnershipRequested { .. })) {
                let order = folded(&held, event.body().stream.id.cast());
                let count = answers.get(&event.body().event_id).copied().unwrap_or(0);
                // A request before its order's creation never waits, and gets no answer.
                let expected = usize::from(order.info().is_some_and(|info| {
                    let creation = held.iter().find(|held| held.body().event_id == info.created_by).unwrap();
                    (creation.body().hlc, creation.body().origin_device) < (event.body().hlc, event.body().origin_device)
                }));
                prop_assert_eq!(count, expected, "{:?}", event.body().event_id);
            }
        }
        prop_assert!(hub.check().unwrap().is_empty());

        // Another replica takes in every log in any order, and agrees.
        let hub_log = hub.log(device(OWN), 0, 1_000).unwrap();
        let mut replica = open(&replica_scratch, REPLICA, &plan);
        let all = [logs[0].clone(), logs[1].clone(), hub_log, former];
        let mut next = [0_usize; 4];
        let rest = (0..4).flat_map(|log| core::iter::repeat_n(log, all[log].len()));
        for (turn, log) in schedule.iter().copied().chain(rest).enumerate() {
            if next[log] == all[log].len() {
                continue;
            }
            receive(&mut replica, core::slice::from_ref(&all[log][next[log]]), 30_000 + i64::try_from(turn).unwrap());
            next[log] += 1;
        }
        for n in 0..2 {
            prop_assert_eq!(replica.order(order(n)).unwrap(), hub.order(order(n)).unwrap());
        }
        prop_assert!(replica.check().unwrap().is_empty());
        // The replica isn't the hub: it answers nothing.
        prop_assert!(matches!(replica.answer_requests(at(40_000)), Err(StoreError::NotHub)));
    }
}

/// The order event `event` holds, if it is one.
fn decoded_if_order(event: &SignedEvent) -> Option<OrderEvent> {
    let body = event.body();
    (body.stream.kind.as_str() == "order")
        .then(|| OrderEvent::decode(&body.schema, &body.payload).ok())
        .flatten()
}
