//! Sync ingest end to end (ADR-0028, decision 6.1): a location provisioned with its cloud, by
//! `keel-provision`; the cell serving it; a hub and a register, each a runtime over its own store
//! on its own thread, on the system's clock, joined by `keel-net` over real TLS on the host. A
//! sale rung on the register reaches the hub and the cloud, and the durable-ack watermark comes
//! back to the register.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::{NonZeroU8, NonZeroU16, NonZeroU32};
use core::time::Duration;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use keel_cloud::{CellError, Location};
use keel_domain::order::Mode;
use keel_domain::profile::{Profile, demo};
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::Net;
use keel_provision::{CloudSpec, DeviceSpec, Kit, Spec, provision};
use keel_runtime::{Identity, Runtime};
use keel_store::{OrderState, StoreKey};
use keel_types::{Currency, Locale, Money, OsEntropy, SystemClock};

type DeviceRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("keel-cloud-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn open(kit: &Kit, dir: &Dir) -> DeviceRuntime {
    let mut store_key = kit.store_key;
    Runtime::open(
        dir.0.join("store.db"),
        StoreKey::new(&mut store_key),
        Identity {
            device: kit.device,
            signer: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &kit.signing_key)
                .unwrap(),
        },
        Profile::decode(&kit.profile).unwrap(),
        SystemClock,
        OsEntropy,
        OsEntropy,
        Locale::EN_US,
    )
    .unwrap()
}

fn spec(hub_port: u16, cloud_port: u16) -> Spec {
    Spec {
        profile: demo().unwrap().encode(),
        hub_host: "127.0.0.1".into(),
        hub_port: NonZeroU16::new(hub_port).unwrap(),
        hub_name: "hub.keel-cafe.local".into(),
        cloud: Some(CloudSpec {
            host: "127.0.0.1".into(),
            port: NonZeroU16::new(cloud_port).unwrap(),
            name: "cell.keel-cafe.example".into(),
        }),
        devices: vec![
            DeviceSpec { name: "Back office".into(), hub: NonZeroU8::new(1) },
            DeviceSpec { name: "Front counter".into(), hub: None },
        ],
    }
}

fn bound() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

/// Runs `runtime` on `net` until `stop`.
fn serve(
    mut runtime: DeviceRuntime,
    net: Net,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<DeviceRuntime> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            keel_hub::step(&mut runtime, &net, Duration::from_millis(50)).unwrap();
        }
        runtime
    })
}

#[test]
fn a_sale_reaches_the_cloud_and_the_register_hears_it_is_backed_up() {
    let (hub_listener, hub_port) = bound();
    let (cloud_listener, cloud_port) = bound();
    let provisioned = provision(&spec(hub_port, cloud_port)).unwrap();
    let kit = |name: &str| provisioned.kits.iter().find(|kit| kit.name == name).unwrap().clone();
    let (hub_kit, register_kit) = (kit("Back office"), kit("Front counter"));
    let roster = provisioned.roster.clone();
    let (cloud_dir, hub_dir, register_dir) = (dir("cell"), dir("hub"), dir("register"));
    let cloud_kit = cloud_dir.0.join("kit");
    provisioned.cloud.as_ref().unwrap().write(&cloud_kit).unwrap();
    let cloud = Location::open(&cloud_kit, cloud_listener).unwrap();
    assert_eq!(cloud.id(), roster.data().location);

    let mut hub = open(&hub_kit, &hub_dir);
    let first = hub.start_sync(roster.clone()).unwrap();
    // The hub listens for the location's devices and dials its cloud.
    let hub_net = Net::hub(hub_listener, roster.clone(), hub_kit.credentials()).unwrap();
    for frame in first {
        hub_net.send(frame.to, frame.frame);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let hub = serve(hub, hub_net, Arc::clone(&stop));

    let mut register = open(&register_kit, &register_dir);
    register.sign_in(demo().unwrap().data().team[0].id, "1234").unwrap();
    let first = register.start_sync(roster.clone()).unwrap();
    let net = Net::device(roster, register_kit.credentials()).unwrap();
    for frame in first {
        net.send(frame.to, frame.frame);
    }
    let started = Instant::now();
    let (mut order, mut paid, mut head) = (None, None, 0);
    while started.elapsed() < Duration::from_secs(60) {
        keel_hub::step(&mut register, &net, Duration::from_millis(50)).unwrap();
        let status = register.sync_status().unwrap().unwrap();
        if status.settled && status.hub_reachable && order.is_none() {
            let id = register.start_order(Mode::Takeout).unwrap().order;
            let variant = demo().unwrap().data().menu.pages[0].buttons[0];
            register.add_item(id, variant, &[], NonZeroU32::MIN).unwrap();
            register.pay_cash(id, Money::from_minor(1_000, Currency::USD)).unwrap();
            order = Some(id);
            paid = Some(Instant::now());
            let now = register.sync_status().unwrap().unwrap();
            head = now.head;
            // What it has just written hasn't left it yet, so it can't be backed up.
            assert!(now.backed_up < head, "{now:?}");
        }
        // Backed up: the cloud holds every event the register wrote, and the hub told it so.
        if order.is_some() && status.backed_up >= head {
            break;
        }
    }
    let order = order.expect("the register settled and heard its hub");
    let status = register.sync_status().unwrap().unwrap();
    assert!(head > 0);
    assert_eq!(status.head, head, "the register wrote nothing after the sale");
    assert_eq!(status.backed_up, head, "the cloud's watermark reaches the register within 60 s");
    let latency = paid.unwrap().elapsed();
    assert!(latency < Duration::from_secs(10), "backed up {latency:?} after it was paid");
    // The cloud's store holds the sale, closed, as the kernel folds it.
    let device = register_kit.device;
    let (state, cloud_head) = cloud
        .with(move |runtime| {
            let state = runtime.store().order(order).unwrap().unwrap().state;
            (state, runtime.store().head(device).unwrap().seq())
        })
        .unwrap();
    assert_eq!(state, OrderState::Closed);
    assert_eq!(cloud_head, head);
    stop.store(true, Ordering::Relaxed);
    let hub = hub.join().unwrap();
    assert_eq!(hub.store().order(order).unwrap().unwrap().state, OrderState::Closed);
}

#[test]
fn a_kit_that_isnt_its_location_s_cloud_s_is_refused() {
    let provisioned = provision(&spec(7443, 8443)).unwrap();
    let kits = dir("not-cloud");
    // A device's kit, with a roster that names a cloud that isn't it.
    let path = kits.0.join("register");
    provisioned.kits[0].write(&path).unwrap();
    let (listener, _) = bound();
    assert!(matches!(Location::open(&path, listener), Err(CellError::NotCloud)));
    // A location without a cloud has none to serve.
    let without = provision(&Spec { cloud: None, ..spec(7443, 8443) }).unwrap();
    let path = kits.0.join("hub");
    without.kits[0].write(&path).unwrap();
    let (listener, _) = bound();
    assert!(matches!(Location::open(&path, listener), Err(CellError::NotCloud)));
}

#[test]
fn a_job_runs_on_the_location_s_thread_and_its_answer_comes_back() {
    let (listener, port) = bound();
    let provisioned = provision(&spec(7443, port)).unwrap();
    let dir = dir("job");
    let path = dir.0.join("kit");
    provisioned.cloud.as_ref().unwrap().write(&path).unwrap();
    let cloud = Location::open(&path, listener).unwrap();
    let device = provisioned.cloud.as_ref().unwrap().device;
    // The cloud's own log: what it opened with, nothing a device sent.
    let head = cloud.with(move |runtime| runtime.store().head(device).unwrap().seq()).unwrap();
    let again = cloud.with(move |runtime| runtime.store().head(device).unwrap().seq()).unwrap();
    assert_eq!(head, again);
    assert_eq!(cloud.name(), demo().unwrap().data().name.to_string());
    drop(cloud);
    // Opened again on the same store, it holds what it held.
    let (listener, _) = bound();
    let cloud = Location::open(&path, listener).unwrap();
    assert_eq!(
        cloud.with(move |runtime| runtime.store().head(device).unwrap().seq()).unwrap(),
        head
    );
}
