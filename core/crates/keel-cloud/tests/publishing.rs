//! Catalog publishing end to end (ADR-0028, decision 6.2): a location provisioned with its cloud,
//! the cell serving it, a hub and a register over real TLS on the host. The cell publishes a
//! profile; it reaches the register within the 60 s budget, and the next sale is priced by it.
//! The cell, restarted, opens with what it published.

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

use keel_cloud::Location;
use keel_domain::order::Mode;
use keel_domain::profile::{Profile, demo};
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::Net;
use keel_provision::{CloudSpec, DeviceSpec, Kit, Spec, provision};
use keel_runtime::{Identity, Runtime};
use keel_store::StoreKey;
use keel_types::{Currency, Locale, Money, OsEntropy, SystemClock};

type DeviceRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("keel-publishing-{name}-{}", std::process::id()));
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

fn bound() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

/// The demo café's profile with its first button's variant at `cents`.
fn first_button_at(cents: i64) -> Profile {
    let mut data = demo().unwrap().data().clone();
    let first = data.menu.pages[0].buttons[0];
    for item in &mut data.catalog.items {
        for variant in &mut item.variants {
            if variant.id == first {
                variant.price = Money::from_minor(cents, Currency::USD);
            }
        }
    }
    Profile::new(data).unwrap()
}

#[test]
fn a_catalog_published_reaches_the_register_within_60_s_and_prices_the_next_sale() {
    let (hub_listener, hub_port) = bound();
    let (cloud_listener, cloud_port) = bound();
    let provisioned = provision(&Spec {
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
    })
    .unwrap();
    let kit = |name: &str| provisioned.kits.iter().find(|kit| kit.name == name).unwrap().clone();
    let (hub_kit, register_kit) = (kit("Back office"), kit("Front counter"));
    let roster = provisioned.roster.clone();
    let (cloud_dir, hub_dir, register_dir) = (dir("cell"), dir("hub"), dir("register"));
    let cloud_kit = cloud_dir.0.join("kit");
    provisioned.cloud.as_ref().unwrap().write(&cloud_kit).unwrap();
    let cloud = Location::open(&cloud_kit, cloud_listener).unwrap();

    let mut hub = open(&hub_kit, &hub_dir);
    let first = hub.start_sync(roster.clone()).unwrap();
    let hub_net = Net::hub(hub_listener, roster.clone(), hub_kit.credentials()).unwrap();
    for frame in first {
        hub_net.send(frame.to, frame.frame);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let hub = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                keel_hub::step(&mut hub, &hub_net, Duration::from_millis(50)).unwrap();
            }
            hub
        })
    };

    let mut register = open(&register_kit, &register_dir);
    register.sign_in(demo().unwrap().data().team[0].id).unwrap();
    let first = register.start_sync(roster.clone()).unwrap();
    let net = Net::device(roster, register_kit.credentials()).unwrap();
    for frame in first {
        net.send(frame.to, frame.frame);
    }
    let provisioned_version = register.profile_version();
    let published = first_button_at(410);
    let (mut version, mut sent) = (None, None);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(90) {
        keel_hub::step(&mut register, &net, Duration::from_millis(50)).unwrap();
        let status = register.sync_status().unwrap().unwrap();
        // Published once the register is settled with its hub, as a back office would.
        if version.is_none() && status.settled && status.hub_reachable {
            version = Some(cloud.publish(published.clone()).unwrap());
            sent = Some(Instant::now());
        }
        if version.is_some_and(|version| register.profile_version() == version) {
            break;
        }
    }
    let version = version.expect("the register settled with its hub");
    assert_ne!(version, provisioned_version);
    assert_eq!(register.profile_version(), version, "the register takes the publication");
    let took = sent.unwrap().elapsed();
    assert!(took < Duration::from_secs(60), "taken {took:?} after it was published");
    assert_eq!(cloud.profile_version().unwrap(), version);

    // The next sale is priced by it.
    let order = register.start_order(Mode::Takeout).unwrap().order;
    let variant = published.data().menu.pages[0].buttons[0];
    let ticket = register.add_item(order, variant, &[], NonZeroU32::MIN).unwrap();
    assert_eq!(ticket.lines[0].amount.money, Money::from_minor(410, Currency::USD));
    stop.store(true, Ordering::Relaxed);
    let hub = hub.join().unwrap();
    assert_eq!(hub.profile_version(), version);

    // The cell, restarted, opens with what it published.
    drop(cloud);
    let (listener, _) = bound();
    let cloud = Location::open(&cloud_kit, listener).unwrap();
    assert_eq!(cloud.profile_version().unwrap(), version);
}
