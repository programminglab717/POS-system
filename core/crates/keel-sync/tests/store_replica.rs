//! The replicator over `keel-store`'s store (ADR-0019): two encrypted stores replicate, a store
//! whose write is interrupted stores nothing and is sent the events again, and a hub answers a
//! request for an order, then numbers its answer (ADR-0021).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU8;
use core::sync::atomic::{AtomicU64, Ordering};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use keel_domain::order::{
    Channel, Mode, Order, OrderCommand, OrderCreated, OrderEvent, OwnershipGranted,
};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::SequenceEvent;
use keel_events::envelope::{StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::SoftwareSigner;
use keel_events::log::EventDraft;
use keel_events::verify::DeviceRegistry;
use keel_store::{Faults, Point, Received, Store, StoreConfig, StoreError, StoreKey};
use keel_sync::{
    Events, Frame, Have, Outgoing, Replicator, Roles, StoreReplica, SyncConfig, VersionVector,
};
use keel_types::{Currency, Id, SeededEntropy};
use support::{DRIFT, at, device, draft, here, registry, signer};

/// A directory of its own, in memory where there is one, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let shm = std::path::Path::new("/dev/shm");
        let base = if shm.is_dir() { shm.to_path_buf() } else { std::env::temp_dir() };
        let dir = base.join(format!("keel-sync-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn db(&self) -> PathBuf {
        self.0.join("store.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Refuses at the point it is given, once, after letting it pass the number of times given.
#[derive(Clone, Default)]
struct Plan(Arc<Mutex<Option<(Point, u32)>>>);

impl Faults for Plan {
    fn proceed(&mut self, point: Point) -> bool {
        let mut planned = self.0.lock().unwrap();
        match *planned {
            Some((at, 0)) if at == point => {
                *planned = None;
                false
            }
            Some((at, passes)) if at == point => {
                *planned = Some((at, passes - 1));
                true
            }
            _ => true,
        }
    }
}

type TestStore = Store<SoftwareSigner, SeededEntropy>;

fn open(scratch: &Scratch, n: u8, plan: &Plan) -> TestStore {
    let config = StoreConfig { device: device(n), location: here(), max_forward_drift: DRIFT };
    let key = StoreKey::new(&mut [n; 32]);
    let entropy = SeededEntropy::new(u64::from(n));
    Store::open_with_faults(scratch.db(), key, config, signer(n), entropy, Box::new(plan.clone()))
        .unwrap()
}

fn append(store: &mut TestStore, count: u64, ms: i64) -> Vec<SignedEvent> {
    store
        .write(|w| (0..count).map(|k| w.append(draft(k), at(ms))).collect::<Result<Vec<_>, _>>())
        .unwrap()
}

/// Delivers `out`, from `from`, to the replica `to`, and returns what it sends back.
fn deliver(
    replicator: &mut Replicator,
    store: &mut TestStore,
    registry: &DeviceRegistry,
    from: u8,
    out: &[Outgoing],
    ms: i64,
) -> Result<Vec<Outgoing>, StoreError> {
    let mut replica = StoreReplica::new(store, registry);
    let mut back = Vec::new();
    for Outgoing { frame, .. } in out {
        back.extend(replicator.on_frame(&mut replica, device(from), frame, at(ms))?);
    }
    Ok(back)
}

/// Hands `replicator` one `frame`, from device `from`, at `ms` milliseconds.
fn hand(
    replicator: &mut Replicator,
    store: &mut TestStore,
    registry: &DeviceRegistry,
    from: u8,
    frame: &Frame,
    ms: i64,
) -> Vec<Outgoing> {
    let mut replica = StoreReplica::new(store, registry);
    replicator.on_frame(&mut replica, device(from), &frame.encode(), at(ms)).unwrap()
}

#[test]
fn two_stores_replicate_through_their_replicators() {
    let registry = registry([1, 3]);
    let (a_dir, hub_dir) = (Scratch::new("a"), Scratch::new("hub"));
    let plan = Plan::default();
    let (mut a, mut hub) = (open(&a_dir, 1, &plan), open(&hub_dir, 3, &plan));
    let config = SyncConfig::DEFAULT;
    let (mut a_sync, a_hello) = Replicator::start(
        &mut StoreReplica::new(&mut a, &registry),
        [device(3)],
        config,
        Roles::default(),
        at(0),
    )
    .unwrap();
    let (mut hub_sync, hub_hello) = Replicator::start(
        &mut StoreReplica::new(&mut hub, &registry),
        [device(1)],
        config,
        Roles::default(),
        at(0),
    )
    .unwrap();
    // Each asks what the other holds, and is told at once.
    let hub_answer = deliver(&mut hub_sync, &mut hub, &registry, 1, &a_hello, 1).unwrap();
    let a_answer = deliver(&mut a_sync, &mut a, &registry, 3, &hub_hello, 1).unwrap();
    for answer in [&hub_answer, &a_answer] {
        let [Outgoing { frame, .. }] = answer.as_slice() else { panic!("{answer:?}") };
        assert!(matches!(Frame::decode(frame), Ok(Frame::Have(Have { asks: false, .. }))));
    }
    assert!(deliver(&mut a_sync, &mut a, &registry, 3, &hub_answer, 2).unwrap().is_empty());
    assert!(deliver(&mut hub_sync, &mut hub, &registry, 1, &a_answer, 2).unwrap().is_empty());
    assert!(a_sync.settled() && hub_sync.settled());
    // The device appends; its batch reaches the hub, which acknowledges it.
    let events = append(&mut a, 3, 10);
    let batches =
        a_sync.appended(&mut StoreReplica::new(&mut a, &registry), &events, at(10)).unwrap();
    assert!(matches!(Frame::decode(&batches[0].frame), Ok(Frame::Events(_))));
    let ack = deliver(&mut hub_sync, &mut hub, &registry, 1, &batches, 11).unwrap();
    assert_eq!(hub.log(device(1), 0, 10).unwrap(), events);
    assert!(deliver(&mut a_sync, &mut a, &registry, 3, &ack, 12).unwrap().is_empty());
    assert_eq!(a_sync.known(device(3)), Some(&a.version_vector().unwrap()));
    assert_eq!(hub.version_vector().unwrap(), a.version_vector().unwrap());
    assert_eq!(hub.check().unwrap(), []);
}

#[test]
fn a_store_interrupted_mid_write_stores_nothing_and_is_sent_the_events_again() {
    let registry = registry([1, 3]);
    let (a_dir, hub_dir) = (Scratch::new("a"), Scratch::new("hub"));
    let (a_plan, hub_plan) = (Plan::default(), Plan::default());
    let (mut a, mut hub) = (open(&a_dir, 1, &a_plan), open(&hub_dir, 3, &hub_plan));
    let config = SyncConfig::DEFAULT;
    let (mut a_sync, _) = Replicator::start(
        &mut StoreReplica::new(&mut a, &registry),
        [device(3)],
        config,
        Roles::default(),
        at(0),
    )
    .unwrap();
    let (mut hub_sync, hub_hello) = Replicator::start(
        &mut StoreReplica::new(&mut hub, &registry),
        [device(1)],
        config,
        Roles::default(),
        at(0),
    )
    .unwrap();
    deliver(&mut a_sync, &mut a, &registry, 3, &hub_hello, 1).unwrap();
    let events = append(&mut a, 2, 10);
    let batches =
        a_sync.appended(&mut StoreReplica::new(&mut a, &registry), &events, at(10)).unwrap();
    // The hub's write stops after storing the second event, and rolls back the first too: one
    // write takes the whole batch.
    *hub_plan.0.lock().unwrap() = Some((Point::Stored, 1));
    let failed = deliver(&mut hub_sync, &mut hub, &registry, 1, &batches, 11);
    assert!(matches!(failed, Err(StoreError::Interrupted(Point::Stored))), "{failed:?}");
    assert!(hub.version_vector().unwrap().is_empty());
    // No acknowledgement comes, so the device sends the batch again when it times out.
    let again = a_sync.on_tick(&mut StoreReplica::new(&mut a, &registry), at(10 + 2_000)).unwrap();
    deliver(&mut hub_sync, &mut hub, &registry, 1, &again, 2_011).unwrap();
    assert_eq!(hub.log(device(1), 0, 10).unwrap(), events);
    assert_eq!(hub_sync.version_vector(), &hub.version_vector().unwrap());
}

/// An event of the order `order`, as a draft.
fn order_draft(order: Id<Order>, event: &OrderEvent) -> EventDraft {
    let (schema, payload) = event.encode().unwrap();
    let stream = StreamRef { kind: StreamKind::new(OrderEvent::STREAM).unwrap(), id: order.cast() };
    EventDraft { stream, schema, payload, ..draft(0) }
}

/// A dine-in order, created.
fn created() -> OrderEvent {
    OrderEvent::Created(OrderCreated {
        channel: Channel::Pos,
        mode: Mode::DineIn,
        currency: Currency::from_code("USD").unwrap(),
        revenue_center: None,
        table: None,
        guest_count: None,
        customer: None,
        owner: None,
    })
}

/// The record `event` of epoch 2: the number of its first event, and its runs.
fn spans(event: &SignedEvent) -> (u64, Vec<(Id<keel_events::envelope::Device>, u64, u64)>) {
    let body = event.body();
    let Ok(SequenceEvent::Assigned(record)) = SequenceEvent::decode(&body.schema, &body.payload)
    else {
        panic!("not a record")
    };
    assert_eq!(record.epoch, 2);
    let spans: Vec<_> = record.runs.iter().map(|run| (run.device, run.from, run.to)).collect();
    (record.first, spans)
}

#[test]
fn a_hub_answers_a_request_for_an_order_then_numbers_its_answer() {
    let registry = registry([1, 2, 3]);
    let (a_dir, b_dir, hub_dir) = (Scratch::new("a"), Scratch::new("b"), Scratch::new("hub"));
    let plan = Plan::default();
    let (mut a, mut b, mut hub) =
        (open(&a_dir, 1, &plan), open(&b_dir, 2, &plan), open(&hub_dir, 3, &plan));
    // Device 1 opens an order; device 2 holds it, and asks for it.
    let order: Id<Order> = Id::parse("0192f0c1-0000-7000-8000-000000007000").unwrap();
    // Device 1 was the hub, in epoch 1.
    let first_claim = a.claim(NonZeroU8::MIN, at(5)).unwrap().unwrap();
    let creation = a.write(|w| w.append(order_draft(order, &created()), at(10))).unwrap();
    for event in [&first_claim, &creation] {
        let received = b.write(|w| w.receive(&event.to_bytes(), &registry, at(20))).unwrap();
        assert!(matches!(received, Received::Stored(_)));
    }
    let view = b.load(Order::new(order)).unwrap();
    let asked = view.decide(here(), device(2), OrderCommand::RequestOwnership).unwrap();
    let request = b.write(|w| w.append(order_draft(order, &asked), at(30))).unwrap();

    // The hub took the role in epoch 2, holding device 1's claim, which device 2 holds too, with
    // the hub's own. Settled, it takes the order and the request in from device 2.
    let received = hub.write(|w| w.receive(&first_claim.to_bytes(), &registry, at(32))).unwrap();
    assert!(matches!(received, Received::Stored(_)));
    let second_claim = hub.claim(NonZeroU8::MIN, at(35)).unwrap().unwrap();
    let roles = Roles { hub: Some(NonZeroU8::MIN), durable: None };
    let (mut hub_sync, _) = Replicator::start(
        &mut StoreReplica::new(&mut hub, &registry),
        [device(1), device(2)],
        SyncConfig::DEFAULT,
        roles,
        at(0),
    )
    .unwrap();
    let settle =
        Frame::Have(Have { location: here(), vv: VersionVector::new(), acked: 0, asks: false });
    hand(&mut hub_sync, &mut hub, &registry, 1, &settle, 40);
    // Device 1 holds none of the hub's log, but device 2 hasn't said yet.
    assert!(!hub_sync.settled());
    // Device 2 says what it holds: the hub is settled, and serves; then device 2 sends it.
    let holds = Frame::Have(Have {
        location: here(),
        vv: [(device(1), 2), (device(2), 1), (device(3), 1)].into_iter().collect(),
        acked: 0,
        asks: false,
    });
    let settling = hand(&mut hub_sync, &mut hub, &registry, 2, &holds, 45);
    assert!(hub_sync.settled() && hub_sync.serving());
    // Settling, the hub numbered both claims, and sent device 2 its record.
    let first_record = sent_to(&settling, device(2));
    assert_eq!(first_record.len(), 1);
    let batch =
        Frame::Events(Events { batch: 1, events: vec![creation.to_bytes(), request.to_bytes()] });
    hand(&mut hub_sync, &mut hub, &registry, 2, &batch, 50);

    // Settling, it numbered both claims; then it granted the request, and numbered the rest, its
    // grant too.
    let own = hub.log(device(3), 0, 10).unwrap();
    assert_eq!(own.len(), 4, "its claim, a record, the grant, then a record");
    assert_eq!(own[0], second_claim);
    assert_eq!(spans(&own[1]), (1, vec![(device(1), 1, 1), (device(3), 1, 1)]));
    let body = own[2].body();
    let grant = OrderEvent::decode(&body.schema, &body.payload).unwrap();
    let OrderEvent::OwnershipGranted(OwnershipGranted {
        request: answered, device: to, epoch, ..
    }) = grant
    else {
        panic!("not a grant: {grant:?}")
    };
    assert_eq!((answered, to, epoch.get()), (request.body().event_id, device(2), 2));
    assert_eq!(spans(&own[3]), (3, vec![(device(1), 2, 2), (device(2), 1, 1), (device(3), 3, 3)]));
    let summary = hub.order(order).unwrap().unwrap();
    assert_eq!(summary.ownership.map(|ownership| ownership.device), Some(device(2)));
    assert_eq!(summary.requests, 0);
    // Both go out to the device that asked, which holds the rest.
    assert_eq!(first_record, own[1..2]);
    // Once device 2 has the record, the grant and the next go out to it, the device that asked.
    let acked = settling
        .iter()
        .filter(|outgoing| outgoing.to == device(2))
        .find_map(|outgoing| match Frame::decode(&outgoing.frame) {
            Ok(Frame::Events(events)) => Some(events.batch),
            _ => None,
        })
        .unwrap();
    let took = Frame::Have(Have {
        location: here(),
        vv: [(device(1), 2), (device(2), 1), (device(3), 2)].into_iter().collect(),
        acked,
        asks: false,
    });
    let out = hand(&mut hub_sync, &mut hub, &registry, 2, &took, 55);
    assert_eq!(sent_to(&out, device(2)), own[2..]);
}

/// The events batches in `out` send to `to`.
fn sent_to(out: &[Outgoing], to: Id<keel_events::envelope::Device>) -> Vec<SignedEvent> {
    out.iter()
        .filter(|outgoing| outgoing.to == to)
        .filter_map(|outgoing| match Frame::decode(&outgoing.frame) {
            Ok(Frame::Events(events)) => Some(events.events),
            _ => None,
        })
        .flatten()
        .map(|bytes| SignedEvent::from_stored(&bytes).unwrap())
        .collect()
}
