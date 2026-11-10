//! `cargo run -p keel-cloud --example demo -- <dir> <api address> <web dir>`: a location end to
//! end on the host, for the back office's tests and for trying it by hand. It provisions the
//! demo café with its cloud into `<dir>`, and runs in one process the cell, serving the back
//! office's API on `<api address>` and its files from `<web dir>`, the location's hub, and a
//! register, all over real TLS on loopback. The register rings and pays three sales, which reach
//! the cell through the hub, then rings one more every time the cell publishes a profile; it
//! writes the version of the profile it rings from to `<dir>/register-version`. The operator's
//! token is in `<dir>/token`.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a harness for tests, which reports to its terminal and stops on any failure"
)]

use core::num::{NonZeroU8, NonZeroU16, NonZeroU32};
use core::time::Duration;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use keel_cloud::Location;
use keel_cloud::api::{Cell, router};
use keel_domain::order::Mode;
use keel_domain::profile::{Profile, demo};
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::Net;
use keel_provision::{CloudSpec, DeviceSpec, Kit, Spec, provision};
use keel_runtime::{Identity, Runtime};
use keel_store::StoreKey;
use keel_types::{Currency, Locale, Money, OsEntropy, SystemClock};

type DeviceRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

/// The operator's token: long enough, and the tests' to know.
const TOKEN: &str = "demo-operator-token-0123456789abcdef";

fn open(kit: &Kit, dir: &Path) -> DeviceRuntime {
    std::fs::create_dir_all(dir).unwrap();
    let mut store_key = kit.store_key;
    Runtime::open(
        dir.join("store.db"),
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

/// Rings the menu's first button and pays for it in cash: a sale.
fn sell(register: &mut DeviceRuntime) {
    let variant = register.profile().data().menu.pages[0].buttons[0];
    let order = register.start_order(Mode::Takeout).unwrap().order;
    register.add_item(order, variant, &[], NonZeroU32::MIN).unwrap();
    register.pay_cash(order, Money::from_minor(10_000, Currency::USD)).unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(api), Some(web)) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: demo <dir> <api address> <web dir>");
        std::process::exit(2);
    };
    let dir = PathBuf::from(dir);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token"), TOKEN).unwrap();
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
    let cloud_kit = dir.join("cloud");
    provisioned.cloud.as_ref().unwrap().write(&cloud_kit).unwrap();
    let location = Location::open(&cloud_kit, cloud_listener).unwrap();

    // The hub, on a thread of its own.
    let mut hub = open(&hub_kit, &dir.join("hub"));
    let first = hub.start_sync(roster.clone()).unwrap();
    let hub_net = Net::hub(hub_listener, roster.clone(), hub_kit.credentials()).unwrap();
    for frame in first {
        hub_net.send(frame.to, frame.frame);
    }
    std::thread::spawn(move || {
        loop {
            if let Err(error) = keel_hub::step(&mut hub, &hub_net, Duration::from_millis(50)) {
                eprintln!("demo: the hub: {error}");
            }
        }
    });

    // The register, on a thread of its own.
    let version_file = dir.join("register-version");
    let mut register = open(&register_kit, &dir.join("register"));
    register.sign_in(demo().unwrap().data().team[0].id).unwrap();
    let first = register.start_sync(roster.clone()).unwrap();
    let net = Net::device(roster, register_kit.credentials()).unwrap();
    for frame in first {
        net.send(frame.to, frame.frame);
    }
    std::thread::spawn(move || {
        let (mut sold, mut version) = (0_u32, None);
        loop {
            if let Err(error) = keel_hub::step(&mut register, &net, Duration::from_millis(50)) {
                eprintln!("demo: the register: {error}");
            }
            let Ok(Some(status)) = register.sync_status() else { continue };
            if !(status.settled && status.hub_reachable) {
                continue;
            }
            if sold < 3 {
                sell(&mut register);
                sold = sold.saturating_add(1);
                continue;
            }
            let now = register.profile_version();
            if version != Some(now) {
                // A publication taken: a sale at its prices, and the version for the tests.
                if version.is_some() {
                    sell(&mut register);
                }
                version = Some(now);
                std::fs::write(&version_file, now.to_hex()).unwrap();
            }
        }
    });

    let cell = Arc::new(Cell::new(vec![location], TOKEN, Some(PathBuf::from(web))));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(&api).await.unwrap();
        println!("demo: the back office on http://{api}");
        axum::serve(listener, router(cell)).await.unwrap();
    });
}
