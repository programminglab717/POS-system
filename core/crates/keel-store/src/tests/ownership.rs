//! Known answers for ownership (ADR-0021): the hub answering requests for orders, and the
//! orders projection keeping each order's owner, lease and waiting requests.

use keel_domain::order::{
    Epoch, Lease, Order, OrderCommand, OrderEvent, Ownership, OwnershipGranted, Refusal,
};
use keel_domain::payment::{Payment, PaymentAuthorized, PaymentEvent, PaymentInitiated, Tender};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::Component;

use super::projections::{created, line_added, order_draft, payment_draft, usd};
use super::*;

/// The hub's epoch in these tests.
const EPOCH: u64 = 1;

fn order_id() -> Id<Order> {
    id(0x7000)
}

/// The store of device `n`, at `path`.
fn open_as(path: &Path, n: u8) -> TestStore {
    let (device, signer) = device(n);
    let config = StoreConfig { device, ..config() };
    Store::open(path, key(), config, signer, SeededEntropy::new(u64::from(n))).unwrap()
}

/// Receives `events`, in order, in one write.
fn take(store: &mut TestStore, events: &[SignedEvent]) {
    store
        .write(|w| {
            for event in events {
                let received = w.receive(&event.to_bytes(), &registry(), at(5_000))?;
                assert!(
                    matches!(received, Received::Stored(_) | Received::Duplicate),
                    "{received:?}"
                );
            }
            Ok::<_, StoreError>(())
        })
        .unwrap();
}

/// What device `n`'s store decides for `command` on the order, appended at `now`.
fn command(store: &mut TestStore, n: u8, command: OrderCommand, now: i64) -> SignedEvent {
    let order = store.load(Order::new(order_id())).unwrap();
    let event = order.decide(here(), device(n).0, command).unwrap();
    append(store, vec![order_draft(order_id(), &event)], at(now)).remove(0)
}

/// The order as `store` keeps it: its owner and lease, and how many requests wait.
fn owned(store: &TestStore) -> (Option<(Id<Device>, u64)>, u64) {
    let summary = store.order(order_id()).unwrap().unwrap();
    let ownership = summary.ownership.map(|ownership| (ownership.device, ownership.lease.get()));
    (ownership, summary.requests)
}

fn decoded(event: &SignedEvent) -> OrderEvent {
    let body = event.body();
    OrderEvent::decode(&body.schema, &body.payload).unwrap()
}

/// Device 2's store with the order created, and line 1 on it.
fn opened(dir: &TempDir) -> TestStore {
    let mut two = open_as(&dir.file("two.db"), 2);
    let events =
        vec![order_draft(order_id(), &created()), order_draft(order_id(), &line_added(1, 450))];
    append(&mut two, events, at(1_000));
    two
}

#[test]
fn the_hub_grants_a_request_and_every_store_sees_the_new_owner() {
    let dir = TempDir::new();
    let mut two = opened(&dir);
    let mut three = open_as(&dir.file("three.db"), 3);
    take(&mut three, &two.log(device(2).0, 0, 10).unwrap());
    assert_eq!(owned(&three), (Some((device(2).0, 0)), 0));
    let request = command(&mut three, 3, OrderCommand::RequestOwnership, 2_000);
    assert_eq!(owned(&three), (Some((device(2).0, 0)), 1));

    let mut hub = open_hub(&dir.db());
    take(&mut hub, &two.log(device(2).0, 0, 10).unwrap());
    take(&mut hub, core::slice::from_ref(&request));
    assert_eq!(owned(&hub), (Some((device(2).0, 0)), 1));
    let answers = hub.answer_requests(at(3_000)).unwrap();
    assert_eq!(answers.len(), 1);
    let grant = &answers[0];
    assert_eq!(
        decoded(grant),
        OrderEvent::OwnershipGranted(OwnershipGranted {
            request: request.body().event_id,
            device: device(3).0,
            lease: Lease::new(1).unwrap(),
            epoch: Epoch::new(EPOCH).unwrap(),
        })
    );
    // The hub records it on the order's stream, caused by the request, under the order's date.
    let body = grant.body();
    assert_eq!(body.origin_device, own().0);
    assert_eq!(body.stream, request.body().stream);
    assert_eq!(body.causation, Some(request.body().event_id.cast()));
    assert_eq!(body.actor, Actor::System(Component::new("hub").unwrap()));
    assert_eq!(body.business_date, request.body().business_date);
    assert_eq!(owned(&hub), (Some((device(3).0, 1)), 0));
    // Nothing waits now: answering again answers nothing.
    assert_eq!(hub.answer_requests(at(3_500)).unwrap(), []);
    // Every store that takes the grant, after the hub's claim, sees the new owner.
    let hub_log = hub.log(own().0, 0, 10).unwrap();
    take(&mut three, &hub_log);
    take(&mut two, &[request]);
    take(&mut two, &hub_log);
    for store in [&three, &two] {
        assert_eq!(owned(store), (Some((device(3).0, 1)), 0));
        let ownership = store.load(Order::new(order_id())).unwrap().ownership();
        let expected = Ownership { device: device(3).0, lease: Lease::new(1).unwrap() };
        assert_eq!(ownership, Some(expected));
    }
}

#[test]
fn a_payment_in_flight_holds_the_order_and_an_authorized_one_doesnt() {
    let dir = TempDir::new();
    let mut two = opened(&dir);
    let payment: Id<Payment> = id(0x8000);
    let initiated = PaymentEvent::Initiated(PaymentInitiated {
        order: order_id(),
        check: order_id().cast(),
        tender: Tender::Card,
        amount: usd(450),
    });
    append(&mut two, vec![payment_draft(payment, &initiated)], at(1_500));
    let mut three = open_as(&dir.file("three.db"), 3);
    take(&mut three, &two.log(device(2).0, 0, 10).unwrap());
    let request = command(&mut three, 3, OrderCommand::RequestOwnership, 2_000);

    let mut hub = open_hub(&dir.db());
    take(&mut hub, &two.log(device(2).0, 0, 10).unwrap());
    take(&mut hub, core::slice::from_ref(&request));
    let answers = hub.answer_requests(at(3_000)).unwrap();
    assert_eq!(
        decoded(&answers[0]),
        OrderEvent::OwnershipRefused {
            request: request.body().event_id,
            refusal: Refusal::PaymentInProgress
        }
    );
    assert_eq!(owned(&hub), (Some((device(2).0, 0)), 0));

    // Once the card is authorized, as for a tab, the order can move.
    let authorized =
        PaymentEvent::Authorized(PaymentAuthorized { amount: usd(450), reference: None });
    let event = append(&mut two, vec![payment_draft(payment, &authorized)], at(3_500)).remove(0);
    take(&mut hub, &[event]);
    take(&mut three, &hub.log(own().0, 0, 10).unwrap());
    let again = command(&mut three, 3, OrderCommand::RequestOwnership, 4_000);
    take(&mut hub, &[again]);
    let answers = hub.answer_requests(at(4_500)).unwrap();
    assert!(matches!(decoded(&answers[0]), OrderEvent::OwnershipGranted(_)));
    assert_eq!(owned(&hub), (Some((device(3).0, 1)), 0));
}

#[test]
fn of_two_requests_from_one_lease_the_first_is_granted() {
    let dir = TempDir::new();
    let two = opened(&dir);
    let log = two.log(device(2).0, 0, 10).unwrap();
    let mut requests = Vec::new();
    for n in [3, 4] {
        let mut store = open_as(&dir.file(&format!("{n}.db")), n);
        take(&mut store, &log);
        requests.push(command(&mut store, n, OrderCommand::RequestOwnership, 2_000 + i64::from(n)));
    }
    let mut hub = open_hub(&dir.db());
    take(&mut hub, &log);
    // The second request arrives first: canonical order, not arrival, decides.
    take(&mut hub, &[requests[1].clone(), requests[0].clone()]);
    let answers: Vec<OrderEvent> =
        hub.answer_requests(at(3_000)).unwrap().iter().map(decoded).collect();
    assert_eq!(
        answers,
        [
            OrderEvent::OwnershipGranted(OwnershipGranted {
                request: requests[0].body().event_id,
                device: device(3).0,
                lease: Lease::new(1).unwrap(),
                epoch: Epoch::new(EPOCH).unwrap(),
            }),
            OrderEvent::OwnershipRefused {
                request: requests[1].body().event_id,
                refusal: Refusal::LeaseMoved
            },
        ]
    );
    assert_eq!(owned(&hub), (Some((device(3).0, 1)), 0));
}

#[test]
fn a_request_waits_for_the_orders_creation() {
    let dir = TempDir::new();
    let two = opened(&dir);
    let mut three = open_as(&dir.file("three.db"), 3);
    take(&mut three, &two.log(device(2).0, 0, 10).unwrap());
    let request = command(&mut three, 3, OrderCommand::RequestOwnership, 2_000);
    // The hub has the request before the order: nothing to answer yet.
    let mut hub = open_hub(&dir.db());
    take(&mut hub, core::slice::from_ref(&request));
    assert_eq!(hub.answer_requests(at(3_000)).unwrap(), []);
    take(&mut hub, &two.log(device(2).0, 0, 10).unwrap());
    let answers = hub.answer_requests(at(3_500)).unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(owned(&hub), (Some((device(3).0, 1)), 0));
}

#[test]
fn answering_is_a_write_of_its_own_and_an_interrupted_one_leaves_nothing() {
    let dir = TempDir::new();
    let two = opened(&dir);
    let mut three = open_as(&dir.file("three.db"), 3);
    let log = two.log(device(2).0, 0, 10).unwrap();
    take(&mut three, &log);
    let request = command(&mut three, 3, OrderCommand::RequestOwnership, 2_000);
    // The hub stores its claim, the two events of the order and the request, then refuses the
    // answer.
    let faults = Box::new(RefuseAt { point: Point::Stored, nth: 5, seen: 0 });
    let mut hub =
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(7), faults)
            .unwrap();
    let claimed = claim(&mut hub, at(100));
    take(&mut hub, &log);
    take(&mut hub, core::slice::from_ref(&request));
    let interrupted = hub.answer_requests(at(3_000));
    assert!(matches!(interrupted, Err(StoreError::Interrupted(Point::Stored))), "{interrupted:?}");
    assert_eq!(hub.head(own().0).unwrap(), LogHead::of(&claimed));
    assert_eq!(owned(&hub), (Some((device(2).0, 0)), 1));
    assert_eq!(hub.answer_requests(at(3_500)).unwrap().len(), 1);
    assert_eq!(owned(&hub), (Some((device(3).0, 1)), 0));
    // A store that isn't the hub answers nothing.
    let mut four = open_as(&dir.file("four.db"), 4);
    take(&mut four, &log);
    take(&mut four, &[request]);
    assert!(matches!(four.answer_requests(at(4_000)), Err(StoreError::NotHub)));
    take(&mut four, &[claimed]);
    assert!(matches!(four.answer_requests(at(4_000)), Err(StoreError::NotHub)));
    assert_eq!(owned(&four), (Some((device(2).0, 0)), 1));
}
