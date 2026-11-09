//! Replication through the runtime (ADR-0026, decision 2): a hub and a register, each a runtime
//! over its own store, passing frames in memory under clocks the test moves. The register
//! refuses to write until its log is settled; a sale it rings reaches the hub, which numbers it;
//! and a roster that isn't the device's is refused.

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
use std::collections::VecDeque;
use std::path::PathBuf;

use keel_domain::codec::Name;
use keel_domain::order::Mode;
use keel_domain::profile::demo;
use keel_domain::roster::{HubAddress, Roster, RosterData, RosterDevice};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_runtime::{Identity, Runtime, RuntimeError};
use keel_store::StoreKey;
use keel_sync::Outgoing;
use keel_types::{Clock, Currency, Id, Locale, ManualClock, Money, SeededEntropy};

type TestRuntime = Runtime<SoftwareSigner, SeededEntropy, ManualClock, SeededEntropy>;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn signer(seed: u8) -> SoftwareSigner {
    SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[seed; 32]).unwrap()
}

const HUB: u64 = 0xd0;
const REGISTER: u64 = 0xd1;

/// The demo café's roster: a hub, and a register that never hosts it.
fn roster() -> Roster {
    let device = |n: u64, name: &str, seed: u8, hub: Option<u8>| RosterDevice {
        id: id(n),
        name: Name::new(name).unwrap(),
        key: signer(seed).public_key().clone(),
        hub: hub.and_then(NonZeroU8::new),
    };
    Roster::new(RosterData {
        location: demo().unwrap().data().location,
        devices: vec![
            device(HUB, "Back office", 1, Some(1)),
            device(REGISTER, "Front counter", 2, None),
        ],
        hub: HubAddress {
            host: "127.0.0.1".into(),
            port: NonZeroU16::new(7443).unwrap(),
            name: "hub.keel-cafe.local".into(),
        },
    })
    .unwrap()
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("keel-sync-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn open(dir: &Dir, n: u64, seed: u8) -> TestRuntime {
    Runtime::open(
        dir.0.join("store.db"),
        StoreKey::new(&mut [0x4b; 32]),
        Identity { device: id(n), signer: signer(seed) },
        demo().unwrap(),
        ManualClock::new("2026-10-02T13:00:00Z".parse().unwrap()),
        SeededEntropy::new(u64::from(seed)),
        SeededEntropy::new(u64::from(seed) + 100),
        Locale::EN_US,
    )
    .unwrap()
}

/// Two runtimes and the frames between them.
struct Location {
    hub: TestRuntime,
    register: TestRuntime,
    wire: VecDeque<(Id<Device>, Outgoing)>,
}

impl Location {
    fn send(&mut self, from: u64, frames: Vec<Outgoing>) {
        self.wire.extend(frames.into_iter().map(|frame| (id(from), frame)));
    }

    /// Delivers every frame on the wire, and those they cause.
    fn deliver(&mut self) {
        while let Some((from, frame)) = self.wire.pop_front() {
            let (target, n) = if frame.to == id(HUB) {
                (&mut self.hub, HUB)
            } else {
                (&mut self.register, REGISTER)
            };
            let answer = target.on_frame(from, &frame.frame).unwrap();
            self.send(n, answer);
        }
    }

    /// Moves both clocks on by `step`, `times` times, ticking each runtime when it asks.
    fn run(&mut self, step: Duration, times: u32) {
        for _ in 0..times {
            self.hub.clock().advance(step).unwrap();
            self.register.clock().advance(step).unwrap();
            for n in [HUB, REGISTER] {
                let runtime = if n == HUB { &mut self.hub } else { &mut self.register };
                if runtime.next_tick().is_some_and(|at| at <= runtime.clock().now()) {
                    let frames = runtime.on_tick().unwrap();
                    self.send(n, frames);
                }
            }
            self.deliver();
        }
    }
}

#[test]
fn a_sale_on_the_register_reaches_the_hub() {
    let (hub_dir, register_dir) = (dir("hub"), dir("register"));
    let mut hub = open(&hub_dir, HUB, 1);
    let mut register = open(&register_dir, REGISTER, 2);
    register.sign_in(id(0x501)).unwrap();
    let hub_frames = hub.start_sync(roster()).unwrap();
    let register_frames = register.start_sync(roster()).unwrap();
    // Until it hears from its peers, the register doesn't write.
    assert!(matches!(register.start_order(Mode::Takeout), Err(RuntimeError::NotSettled)));
    let mut location = Location { hub, register, wire: VecDeque::new() };
    location.send(HUB, hub_frames);
    location.send(REGISTER, register_frames);
    location.deliver();
    // The hub claims its term after three periods of silence from any other.
    location.run(Duration::from_millis(250), 40);
    let status = location.register.sync_status().unwrap();
    assert!(status.settled && status.hub_reachable && !status.is_hub, "{status:?}");
    let hub_status = location.hub.sync_status().unwrap();
    assert!(hub_status.is_hub && hub_status.serving, "{hub_status:?}");

    let order = location.register.start_order(Mode::Takeout).unwrap().order;
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    location.register.pay_cash(order, Money::from_minor(1_000, Currency::USD)).unwrap();
    let frames = location.register.take_frames().unwrap();
    assert!(!frames.is_empty(), "the sale is pushed at once");
    location.send(REGISTER, frames);
    location.deliver();
    // The hub holds the closed order, and numbers what the register wrote.
    let summary = location.hub.store().order(order).unwrap().unwrap();
    assert_eq!(summary.state, keel_store::OrderState::Closed);
    location.run(Duration::from_millis(250), 8);
    let confirmed = location.register.store().confirmed().unwrap();
    let head = location.register.store().head(id(REGISTER)).unwrap().seq();
    assert_eq!(
        confirmed.get(&id(REGISTER)).copied(),
        Some(head),
        "the register's log is confirmed"
    );
}

#[test]
fn a_roster_that_isnt_the_devices_is_refused() {
    let register_dir = dir("refused");
    let mut register = open(&register_dir, REGISTER, 2);
    // A device the roster doesn't have.
    let mut stranger = open(&dir("stranger"), 0xd9, 9);
    assert!(matches!(stranger.start_sync(roster()), Err(RuntimeError::RosterMismatch)));
    // Another key for this device.
    let mut data = roster().data().clone();
    data.devices[1].key = signer(7).public_key().clone();
    assert!(matches!(
        register.start_sync(Roster::new(data).unwrap()),
        Err(RuntimeError::RosterMismatch)
    ));
    // Another location's.
    let mut data = roster().data().clone();
    data.location = id(0xbad);
    assert!(matches!(
        register.start_sync(Roster::new(data).unwrap()),
        Err(RuntimeError::RosterMismatch)
    ));
    // Before replication starts, the runtime has nothing to say.
    assert!(matches!(register.on_tick(), Err(RuntimeError::NotSyncing)));
    assert_eq!(register.take_frames().unwrap(), []);
    assert_eq!(register.next_tick(), None);
}
