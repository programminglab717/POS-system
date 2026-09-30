//! Property tests for the store's key (ADR-0018), against a model: which key is the store's, and
//! what it holds.
//!
//! A case is a sequence of writes, rekeys, rekeys a fault hook interrupts before they begin or
//! refuses once they have happened, and reopenings with one of four keys. Only the store's key
//! may open it, and it must hold exactly the events written, through every change of key.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use std::sync::{Arc, Mutex};

use keel_events::event::SignedEvent;
use keel_events::keys::SoftwareSigner;
use keel_store::{Faults, Point, Store, StoreError};
use keel_types::SeededEntropy;
use proptest::prelude::*;
use support::{OWN, Scratch, at, config, device, draft, signer, store_key_of};

/// The keys a case uses: each byte of each.
const KEYS: [u8; 4] = [0x4B, 0x4C, 0x4D, 0x4E];

#[derive(Clone, Copy, Debug)]
enum Op {
    /// Appends this many events.
    Write(u8),
    /// Changes the store's key to this one.
    Rekey(usize),
    /// A rekey to this key, interrupted before it begins.
    Interrupted(usize),
    /// A rekey to this key, refused once it has happened, which changes nothing.
    RefusedAfter(usize),
    /// Reopens the store, trying this key first.
    Reopen(usize),
}

fn any_op() -> impl Strategy<Value = Op> {
    let key = 0..KEYS.len();
    prop_oneof![
        3 => (1_u8..4).prop_map(Op::Write),
        2 => key.clone().prop_map(Op::Rekey),
        1 => key.clone().prop_map(Op::Interrupted),
        1 => key.clone().prop_map(Op::RefusedAfter),
        2 => key.prop_map(Op::Reopen),
    ]
}

/// Refuses at the point it is given, once.
#[derive(Clone, Default)]
struct Plan(Arc<Mutex<Option<Point>>>);

impl Faults for Plan {
    fn proceed(&mut self, point: Point) -> bool {
        let mut planned = self.0.lock().unwrap();
        if *planned == Some(point) {
            *planned = None;
            return false;
        }
        true
    }
}

type TestStore = Store<SoftwareSigner, SeededEntropy>;

fn open(scratch: &Scratch, key: usize, plan: &Plan, seed: u64) -> Result<TestStore, StoreError> {
    let key = store_key_of(KEYS[key]);
    let faults = Box::new(plan.clone());
    Store::open_with_faults(
        scratch.db(),
        key,
        config(),
        signer(OWN),
        SeededEntropy::new(seed),
        faults,
    )
}

proptest! {
    /// Only the store's key opens it, through every change of key, and it holds what was written.
    #[test]
    fn only_the_stores_key_opens_it(ops in prop::collection::vec(any_op(), 1..16)) {
        let scratch = Scratch::new("keys");
        let plan = Plan::default();
        let mut key = 0;
        let mut store = open(&scratch, key, &plan, 1).unwrap();
        let mut written: Vec<SignedEvent> = Vec::new();
        let mut opened = 1;
        for (i, op) in ops.iter().enumerate() {
            let now = at(i64::try_from(i).unwrap() * 100);
            match *op {
                Op::Write(count) => {
                    let drafts = (0..count).map(|k| draft(1, u64::from(k)));
                    let events = store
                        .write(|w| drafts.map(|draft| w.append(draft, now)).collect::<Result<Vec<_>, _>>())
                        .unwrap();
                    written.extend(events);
                }
                Op::Rekey(new) => {
                    store.rekey(store_key_of(KEYS[new])).unwrap();
                    key = new;
                }
                Op::Interrupted(new) => {
                    *plan.0.lock().unwrap() = Some(Point::Rekeying);
                    let rekeyed = store.rekey(store_key_of(KEYS[new]));
                    prop_assert!(matches!(rekeyed, Err(StoreError::Interrupted(Point::Rekeying))), "{rekeyed:?}");
                }
                Op::RefusedAfter(new) => {
                    *plan.0.lock().unwrap() = Some(Point::Rekeyed);
                    store.rekey(store_key_of(KEYS[new])).unwrap();
                    *plan.0.lock().unwrap() = None;
                    key = new;
                }
                Op::Reopen(first) => {
                    drop(store);
                    opened += 1;
                    if first != key {
                        let refused = open(&scratch, first, &plan, opened);
                        prop_assert!(matches!(refused, Err(StoreError::KeyRejected)), "{:?}", refused.map(|_| ()));
                    }
                    store = open(&scratch, key, &plan, opened).unwrap();
                    prop_assert!(!store.recovered());
                }
            }
            prop_assert_eq!(&store.log(device(OWN), 0, 1_000).unwrap(), &written, "after {:?}", op);
        }
        prop_assert_eq!(store.check().unwrap(), []);
        drop(store);
        // Every other key is refused; the store's opens it, holding what was written.
        for other in 0..KEYS.len() {
            let reopened = open(&scratch, other, &plan, 99);
            if other == key {
                prop_assert_eq!(reopened.unwrap().log(device(OWN), 0, 1_000).unwrap(), written.clone());
            } else {
                prop_assert!(matches!(reopened, Err(StoreError::KeyRejected)), "{:?}", reopened.map(|_| ()));
            }
        }
    }
}
