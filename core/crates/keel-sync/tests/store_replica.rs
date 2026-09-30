//! The replicator over `keel-store`'s store (ADR-0019): two encrypted stores replicate, and a
//! store whose write is interrupted stores nothing and is sent the events again.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::sync::atomic::{AtomicU64, Ordering};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use keel_events::event::SignedEvent;
use keel_events::keys::SoftwareSigner;
use keel_events::verify::DeviceRegistry;
use keel_store::{Faults, Point, Store, StoreConfig, StoreError, StoreKey};
use keel_sync::{Frame, Have, Outgoing, Replicator, StoreReplica, SyncConfig};
use keel_types::SeededEntropy;
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

#[test]
fn two_stores_replicate_through_their_replicators() {
    let registry = registry([1, 3]);
    let (a_dir, hub_dir) = (Scratch::new("a"), Scratch::new("hub"));
    let plan = Plan::default();
    let (mut a, mut hub) = (open(&a_dir, 1, &plan), open(&hub_dir, 3, &plan));
    let config = SyncConfig::DEFAULT;
    let (mut a_sync, a_hello) =
        Replicator::start(&mut StoreReplica::new(&mut a, &registry), [device(3)], config, at(0))
            .unwrap();
    let (mut hub_sync, hub_hello) =
        Replicator::start(&mut StoreReplica::new(&mut hub, &registry), [device(1)], config, at(0))
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
    let (mut a_sync, _) =
        Replicator::start(&mut StoreReplica::new(&mut a, &registry), [device(3)], config, at(0))
            .unwrap();
    let (mut hub_sync, hub_hello) =
        Replicator::start(&mut StoreReplica::new(&mut hub, &registry), [device(1)], config, at(0))
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
