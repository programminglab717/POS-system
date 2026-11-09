//! A location end to end (ADR-0026, decision 6): provisioned by `keel-provision`, a hub and a
//! register, each a runtime over its own store on its own thread, on the system's clock, joined
//! by `keel-net` over real TLS on the host. The register settles, rings and pays a sale, and the
//! hub holds it closed and numbers it; and the provisioned kits survive being written and read.

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

use keel_domain::order::{Mode, Order};
use keel_domain::profile::{Profile, demo};
use keel_domain::roster::Roster;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::Net;
use keel_provision::{DeviceSpec, Kit, Spec, provision};
use keel_runtime::{Identity, Runtime};
use keel_store::StoreKey;
use keel_types::{Currency, Id, Locale, Money, OsEntropy, SystemClock};

type DeviceRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("keel-location-{name}-{}", std::process::id()));
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

fn spec(port: u16) -> Spec {
    Spec {
        profile: demo().unwrap().encode(),
        hub_host: "127.0.0.1".into(),
        hub_port: NonZeroU16::new(port).unwrap(),
        hub_name: "hub.keel-cafe.local".into(),
        devices: vec![
            DeviceSpec { name: "Back office".into(), hub: NonZeroU8::new(1) },
            DeviceSpec { name: "Front counter".into(), hub: None },
        ],
    }
}

/// Runs `runtime` on `net` until `stop`, then gives it back.
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
fn a_sale_on_a_register_reaches_the_hub_over_the_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let provisioned = provision(&spec(port)).unwrap();
    let kit = |name: &str| provisioned.kits.iter().find(|kit| kit.name == name).unwrap().clone();
    let (hub_kit, register_kit) = (kit("Back office"), kit("Front counter"));
    let roster = provisioned.roster.clone();
    let (hub_dir, register_dir) = (dir("hub"), dir("register"));

    let mut hub = open(&hub_kit, &hub_dir);
    let first = hub.start_sync(roster.clone()).unwrap();
    let hub_net = Net::hub(listener, roster.clone(), hub_kit.credentials()).unwrap();
    for frame in first {
        hub_net.send(frame.to, frame.frame);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let hub = serve(hub, hub_net, Arc::clone(&stop));

    let mut register = open(&register_kit, &register_dir);
    register.sign_in(demo().unwrap().data().team[0].id).unwrap();
    let first = register.start_sync(roster.clone()).unwrap();
    let net = Net::device(roster, register_kit.credentials()).unwrap();
    for frame in first {
        net.send(frame.to, frame.frame);
    }
    // The register settles, and hears the hub serve.
    let started = Instant::now();
    let mut order: Option<Id<Order>> = None;
    let mut paid = None;
    while started.elapsed() < Duration::from_secs(30) {
        keel_hub::step(&mut register, &net, Duration::from_millis(50)).unwrap();
        let status = register.sync_status().unwrap();
        if status.settled && status.hub_reachable && order.is_none() {
            let id = register.start_order(Mode::Takeout).unwrap().order;
            register.add_item(id, demo_variant(), &[], NonZeroU32::MIN).unwrap();
            register.pay_cash(id, Money::from_minor(1_000, Currency::USD)).unwrap();
            order = Some(id);
            paid = Some(Instant::now());
        }
        // Confirmed: the hub numbered every event the register wrote.
        if order.is_some() {
            let confirmed = register.store().confirmed().unwrap();
            let head = register.store().head(register_kit.device).unwrap().seq();
            if confirmed.get(&register_kit.device).copied() == Some(head) {
                break;
            }
        }
    }
    let order = order.expect("the register settled and heard its hub");
    let confirmed = register.store().confirmed().unwrap();
    assert_eq!(
        confirmed.get(&register_kit.device).copied(),
        Some(register.store().head(register_kit.device).unwrap().seq()),
        "the register's sale is confirmed by the hub within 30 s"
    );
    // Pushed at once, not at the next round, 5 s away: the hub numbers it and the register hears.
    let latency = paid.unwrap().elapsed();
    assert!(latency < Duration::from_secs(2), "confirmed {latency:?} after it was paid");
    stop.store(true, Ordering::Relaxed);
    let hub = hub.join().unwrap();
    assert_eq!(hub.store().order(order).unwrap().unwrap().state, keel_store::OrderState::Closed);
    assert!(hub.sync_status().unwrap().serving);
}

fn demo_variant() -> Id<keel_domain::refs::Variant> {
    demo().unwrap().data().menu.pages[0].buttons[0]
}

#[test]
fn a_kit_survives_being_written_and_read() {
    let provisioned = provision(&spec(7443)).unwrap();
    assert_eq!(provisioned.kits.len(), 2);
    let roster = Roster::decode(&provisioned.kits[0].roster).unwrap();
    assert_eq!(roster, provisioned.roster);
    // The roster lists the devices in order, each with the key its kit signs with.
    for kit in &provisioned.kits {
        let device = roster.device(kit.device).unwrap();
        let signer =
            SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &kit.signing_key).unwrap();
        assert_eq!(&device.key, keel_events::keys::Signer::public_key(&signer));
        assert_eq!(device.hub.is_some(), kit.name == "Back office");
    }
    // Every key is drawn on its own.
    let (first, second) = (&provisioned.kits[0], &provisioned.kits[1]);
    assert_ne!(first.store_key, first.signing_key);
    assert_ne!(first.store_key, second.store_key);
    assert_ne!(first.signing_key, second.signing_key);
    let dir = dir("kit");
    for kit in &provisioned.kits {
        let path = dir.0.join(kit.device.to_string());
        kit.write(&path).unwrap();
        assert_eq!(&Kit::read(&path).unwrap(), kit);
    }
    // A missing file, or a key of the wrong length, is refused.
    let path = dir.0.join(provisioned.kits[0].device.to_string());
    std::fs::write(path.join("signing.key"), [0_u8; 31]).unwrap();
    assert!(Kit::read(&path).is_err());
    std::fs::remove_file(path.join("roster")).unwrap();
    assert!(Kit::read(&path).is_err());
}
