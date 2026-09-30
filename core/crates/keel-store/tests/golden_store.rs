//! The golden store: a store made once, encrypted with a fixed key, and kept in the repository
//! (ADR-0018). Every later kernel must open it and read it as it was made. A SQLCipher, OpenSSL or
//! kernel that couldn't would leave devices unable to open their stores.
//!
//! It holds the device's own events, among them an order with a line and a closed check and a
//! payment captured on it, another device's events, a quarantined message, and effects in every
//! state. It was made by [`make_the_golden_store`], which doesn't overwrite it: a store at schema
//! version 2 must go on opening, so a new schema version adds a golden store of its own, and
//! keeps this one. Don't change [`stock`] either: the test compares the golden store with a store
//! stocked afresh.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::print_stdout,
    reason = "test code: a broken assumption should fail loudly"
)]

mod support;

use core::fmt::Write as _;
use std::path::PathBuf;

use keel_domain::order::{Line, Order, OrderStatus};
use keel_domain::payment::{Payment, PaymentCaptured, PaymentEvent, PaymentInitiated, Tender};
use keel_events::event::SignedEvent;
use keel_events::keys::SoftwareSigner;
use keel_events::log::{Link, LogHead};
use keel_store::{
    Effect, EffectKind, EffectState, OrderState, PaymentState, Reason, Store, StoreError,
};
use keel_types::{Id, SeededEntropy};
use support::{
    OWN, PEERS, Scratch, at, check_closed, config, created, device, domain_draft, draft, here, id,
    line_added, next_event, projection_rows, registry, signer, store_key_of, usd,
};

/// The golden store's key: each byte of it.
const GOLDEN_KEY: u8 = 0x47;

/// The golden store of schema version 2.
fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/store-v2.db")
}

type TestStore = Store<SoftwareSigner, SeededEntropy>;

fn order() -> Id<Order> {
    id(0x7000)
}

fn line() -> Id<Line> {
    id(0x7100)
}

fn payment() -> Id<Payment> {
    id(0x8000)
}

fn effect(key: &str, cause: Option<&SignedEvent>) -> Effect {
    Effect {
        key: key.as_bytes().to_vec(),
        kind: EffectKind::new("print.kitchen").unwrap(),
        payload: key.as_bytes().to_vec(),
        cause: cause.map(|event| event.body().event_id),
    }
}

/// The golden store's content, written into `store` in three writes.
fn stock(store: &mut TestStore) {
    let registry = registry();
    let mut head = LogHead::EMPTY;
    let peer: Vec<SignedEvent> = (0..3)
        .map(|j| {
            let event = next_event(PEERS[0], here(), head, at(1_000 + j), 1, 0);
            head = LogHead::of(&event);
            event
        })
        .collect();
    store
        .write(|w| {
            w.append(draft(1, 1), at(10_000))?;
            let created =
                w.append(domain_draft(order().cast(), &created(0x200, 2), 0), at(10_000))?;
            let added =
                w.append(domain_draft(order().cast(), &line_added(line(), 450), 0), at(10_000))?;
            for event in &peer[..2] {
                w.receive(&event.to_bytes(), &registry, at(10_000))?;
            }
            w.receive(b"not an event", &registry, at(10_000))?;
            w.enqueue(&effect("print", Some(&added)), at(10_000))?;
            w.enqueue(&effect("reprint", Some(&added)), at(10_000))?;
            w.enqueue(&effect("charge", Some(&created)), at(10_000))?;
            w.enqueue(&effect("notify", None), at(10_000))?;
            Ok::<(), StoreError>(())
        })
        .unwrap();
    store
        .write(|w| {
            let initiated = PaymentEvent::Initiated(PaymentInitiated {
                order: order(),
                check: order().cast(),
                tender: Tender::Card,
                amount: usd(450),
            });
            w.append(domain_draft(payment().cast(), &initiated, 0), at(20_000))?;
            let captured = PaymentEvent::Captured(PaymentCaptured {
                amount: usd(450),
                tip: Some(usd(50)),
                reference: None,
                cash: None,
            });
            w.append(domain_draft(payment().cast(), &captured, 0), at(20_000))?;
            let closed = check_closed(order().cast(), line(), 450, payment());
            w.append(domain_draft(order().cast(), &closed, 0), at(20_000))?;
            w.receive(&peer[2].to_bytes(), &registry, at(20_000))?;
            for key in ["print", "reprint", "charge", "notify"] {
                w.start(key.as_bytes(), at(20_000))?;
            }
            w.finish(b"print")?;
            w.fail(b"reprint")?;
            w.retry(b"notify", at(90_000))?;
            Ok::<(), StoreError>(())
        })
        .unwrap();
    store.write(|w| w.append(draft(2, 2), at(30_000))).unwrap();
}

fn open(path: &std::path::Path, seed: u64) -> TestStore {
    let key = store_key_of(GOLDEN_KEY);
    Store::open(path, key, config(), signer(OWN), SeededEntropy::new(seed)).unwrap()
}

/// Set to make the golden store.
const MAKE: &str = "KEEL_STORE_MAKE_GOLDEN";

/// Makes the golden store, if there is none, when [`MAKE`] is set: only to make one for a new
/// schema version, under a new name, then pin what it holds in the test below.
#[test]
#[ignore = "makes the golden store, which is made once and kept"]
fn make_the_golden_store() {
    if std::env::var(MAKE).is_err() {
        return;
    }
    let path = golden();
    assert!(!path.exists(), "a golden store is kept as it was made: don't overwrite {path:?}");
    let scratch = Scratch::new("golden-make");
    let mut store = open(&scratch.db(), 1);
    stock(&mut store);
    for n in [OWN, PEERS[0]] {
        println!("device {n}: head {:?}", store.head(device(n)).unwrap());
    }
    drop(store);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(scratch.db(), &path).unwrap();
}

/// The golden store opens with its key, checks sound, and reads as it was made: its logs verify
/// and end where they did, and it holds what a store stocked afresh holds.
#[test]
fn the_golden_store_opens_and_reads_as_it_was_made() {
    let scratch = Scratch::new("golden");
    std::fs::copy(golden(), scratch.db()).unwrap();
    let mut store = open(&scratch.db(), 2);
    assert!(!store.recovered(), "the golden store was closed cleanly");
    assert_eq!(store.check().unwrap(), []);
    // The logs end where they did when the store was made, and verify.
    let vector: Vec<(u8, u64)> = [OWN, PEERS[0]]
        .into_iter()
        .map(|n| (n, store.version_vector().unwrap()[&device(n)]))
        .collect();
    assert_eq!(vector, [(OWN, 7), (PEERS[0], 3)]);
    let heads: Vec<String> = [OWN, PEERS[0]]
        .into_iter()
        .map(|n| {
            let hash = store.head(device(n)).unwrap().hash();
            hash.as_bytes().iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
        })
        .collect();
    assert_eq!(
        heads,
        [
            "833ce7717efc2c0ca38fc4b08f1b3cd0fe91037be848e764b8909f146c14b1bc",
            "8b772a9d172cf101cc339aa469eb126834db67584ef35f101cdbc6c8a8500e94",
        ]
    );
    let registry = registry();
    for n in [OWN, PEERS[0]] {
        let mut head = LogHead::EMPTY;
        for event in store.log(device(n), 0, 100).unwrap() {
            assert_eq!(registry.verify(&event.to_bytes()).as_ref(), Ok(&event));
            assert_eq!(head.link(&event), Ok(Link::Next));
            head = LogHead::of(&event);
        }
        assert_eq!(head, store.head(device(n)).unwrap());
    }
    // What was derived from the events.
    // The order's one check is closed; the order stays open until it is closed itself.
    let order = store.order(order()).unwrap().unwrap();
    assert_eq!(
        (order.state, order.live_lines, order.open_checks, order.closed_checks),
        (OrderState::Active, 1, 0, 1)
    );
    assert_eq!(*store.load(Order::new(self::order())).unwrap().status(), OrderStatus::Active);
    let payment = store.payment(payment()).unwrap().unwrap();
    assert_eq!(
        (payment.state, payment.captured, payment.tip),
        (PaymentState::Captured, Some(usd(450)), Some(usd(50)))
    );
    let effects: Vec<(Vec<u8>, EffectState, u32)> = store
        .effects()
        .unwrap()
        .into_iter()
        .map(|queued| (queued.effect.key, queued.state, queued.attempts))
        .collect();
    let expected = [
        ("print", EffectState::Done),
        ("reprint", EffectState::Failed),
        ("charge", EffectState::Running),
        ("notify", EffectState::Pending),
    ]
    .map(|(key, state)| (key.as_bytes().to_vec(), state, 1));
    assert_eq!(effects, expected);
    let quarantine = store.quarantine().unwrap();
    assert_eq!(quarantine.len(), 1);
    assert_eq!(quarantine[0].reason, Reason::Malformed);
    // A store stocked afresh holds the same: events byte for byte, projections, outbox and
    // quarantine.
    let fresh = Scratch::new("golden-fresh");
    let mut again = open(&fresh.db(), 1);
    stock(&mut again);
    for n in [OWN, PEERS[0]] {
        assert_eq!(store.log(device(n), 0, 100).unwrap(), again.log(device(n), 0, 100).unwrap());
    }
    assert_eq!(store.effects().unwrap(), again.effects().unwrap());
    assert_eq!(store.quarantine().unwrap(), again.quarantine().unwrap());
    drop(again);
    assert_eq!(projection_rows(&scratch, GOLDEN_KEY), projection_rows(&fresh, GOLDEN_KEY));
    // The store goes on, and opens again.
    let next = store.write(|w| w.append(draft(1, 99), at(40_000))).unwrap();
    drop(store);
    let mut store = open(&scratch.db(), 3);
    assert_eq!(store.head(device(OWN)).unwrap(), LogHead::of(&next));
    assert_eq!(store.check().unwrap(), []);
}
