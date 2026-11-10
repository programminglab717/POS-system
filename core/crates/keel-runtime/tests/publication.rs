//! Catalog publishing through the runtime (ADR-0028, decision 3): the cloud, a hub and a
//! register, each a runtime over its own store, passing frames in memory under clocks the test
//! moves. The cloud publishes a profile; the hub and the register take it between two intents,
//! and the register's next line is priced by it while a line rung before keeps its price; a
//! device restarted opens with the profile it took; and a publication that isn't the roster's
//! cloud's is never taken.

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
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use keel_domain::codec::Name;
use keel_domain::order::Mode;
use keel_domain::profile::{Profile, demo};
use keel_domain::roster::{CloudAddress, HubAddress, Roster, RosterData, RosterDevice};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_runtime::{Identity, Runtime, RuntimeError};
use keel_store::StoreKey;
use keel_sync::Outgoing;
use keel_types::{Currency, Id, Locale, ManualClock, Money, SeededEntropy};

type TestRuntime = Runtime<SoftwareSigner, SeededEntropy, ManualClock, SeededEntropy>;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn signer(seed: u8) -> SoftwareSigner {
    SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[seed; 32]).unwrap()
}

const HUB: u64 = 0xd0;
const REGISTER: u64 = 0xd1;
const CLOUD: u64 = 0xcc;

/// Each device's key's seed.
fn seed(n: u64) -> u8 {
    match n {
        HUB => 1,
        REGISTER => 2,
        _ => 4,
    }
}

/// The espresso, $3.25 in the demo café's catalog.
const ESPRESSO: u64 = 0x401;

/// The demo café's roster: a hub, a register, and `cloud` its cloud.
fn roster(cloud: u64) -> Roster {
    let device = |n: u64, name: &str, hub: Option<u8>| RosterDevice {
        id: id(n),
        name: Name::new(name).unwrap(),
        key: signer(seed(n)).public_key().clone(),
        hub: hub.and_then(NonZeroU8::new),
    };
    Roster::new(RosterData {
        location: demo().unwrap().data().location,
        devices: vec![
            device(CLOUD, "Cloud", None),
            device(HUB, "Back office", Some(1)),
            device(REGISTER, "Front counter", None),
        ],
        hub: HubAddress {
            host: "127.0.0.1".into(),
            port: NonZeroU16::new(7443).unwrap(),
            name: "hub.keel-cafe.local".into(),
        },
        cloud: Some(CloudAddress {
            device: id(cloud),
            host: "203.0.113.10".into(),
            port: NonZeroU16::new(8443).unwrap(),
            name: "cell.keel-cafe.example".into(),
        }),
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
    let path = std::env::temp_dir().join(format!("keel-publish-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn open(dir: &Dir, n: u64) -> TestRuntime {
    Runtime::open(
        dir.0.join("store.db"),
        StoreKey::new(&mut [0x4b; 32]),
        Identity { device: id(n), signer: signer(seed(n)) },
        demo().unwrap(),
        ManualClock::new("2026-10-02T13:00:00Z".parse().unwrap()),
        SeededEntropy::new(u64::from(seed(n))),
        SeededEntropy::new(u64::from(seed(n)) + 100),
        Locale::EN_US,
    )
    .unwrap()
}

/// The demo café's profile with the espresso at `cents`.
fn espresso_at(cents: i64) -> Profile {
    let mut data = demo().unwrap().data().clone();
    for item in &mut data.catalog.items {
        for variant in &mut item.variants {
            if variant.id == id(ESPRESSO) {
                variant.price = Money::from_minor(cents, Currency::USD);
            }
        }
    }
    Profile::new(data).unwrap()
}

/// Runtimes and the frames between them.
struct Location {
    nodes: BTreeMap<u64, TestRuntime>,
    wire: VecDeque<(Id<Device>, Outgoing)>,
}

impl Location {
    /// The runtimes, each started on the roster naming [`CLOUD`] its cloud, and settled.
    fn start(nodes: BTreeMap<u64, TestRuntime>) -> Location {
        let mut location = Location { nodes, wire: VecDeque::new() };
        let started: Vec<u64> = location.nodes.keys().copied().collect();
        for n in started {
            let frames = location.node(n).start_sync(roster(CLOUD)).unwrap();
            location.send(n, frames);
        }
        location.deliver();
        location.run(40);
        location
    }

    fn node(&mut self, n: u64) -> &mut TestRuntime {
        self.nodes.get_mut(&n).unwrap()
    }

    fn send(&mut self, from: u64, frames: Vec<Outgoing>) {
        self.wire.extend(frames.into_iter().map(|frame| (id(from), frame)));
    }

    /// Delivers every frame on the wire, and those they cause. The cloud and the register
    /// reach the hub alone, as on a location's network.
    fn deliver(&mut self) {
        while let Some((from, frame)) = self.wire.pop_front() {
            let Some(n) = self.nodes.keys().copied().find(|n| frame.to == id(*n)) else {
                continue;
            };
            if from != id(HUB) && n != HUB {
                continue;
            }
            if let Ok(answer) = self.node(n).on_frame(from, &frame.frame) {
                self.send(n, answer);
            }
        }
    }

    /// Moves every clock on by 250 ms, `times` times, ticking each runtime when it asks.
    fn run(&mut self, times: u32) {
        for _ in 0..times {
            let all: Vec<u64> = self.nodes.keys().copied().collect();
            for n in all {
                let runtime = self.node(n);
                runtime.clock().advance(Duration::from_millis(250)).unwrap();
                if runtime.tick_due()
                    && let Ok(frames) = runtime.on_tick()
                {
                    self.send(n, frames);
                }
            }
            self.deliver();
        }
    }
}

fn three(cloud: &Dir, hub: &Dir, register: &Dir) -> Location {
    let nodes = BTreeMap::from([
        (CLOUD, open(cloud, CLOUD)),
        (HUB, open(hub, HUB)),
        (REGISTER, open(register, REGISTER)),
    ]);
    let mut location = Location::start(nodes);
    location.node(REGISTER).sign_in(id(0x501)).unwrap();
    location
}

#[test]
fn a_profile_published_reaches_the_register_and_prices_its_next_line() {
    let (cloud_dir, hub_dir, register_dir) = (dir("cloud"), dir("hub"), dir("register"));
    let mut location = three(&cloud_dir, &hub_dir, &register_dir);
    let provisioned = location.node(REGISTER).profile_version();
    // A line rung under the provisioned profile.
    let order = location.node(REGISTER).start_order(Mode::Takeout).unwrap().order;
    let ticket =
        location.node(REGISTER).add_item(order, id(ESPRESSO), &[], NonZeroU32::MIN).unwrap();
    assert_eq!(ticket.lines[0].amount.money, Money::from_minor(325, Currency::USD));
    let frames = location.node(REGISTER).take_frames().unwrap();
    location.send(REGISTER, frames);
    location.deliver();

    let published = espresso_at(410);
    let version = location.node(CLOUD).publish_profile(&published).unwrap();
    assert_ne!(version, provisioned);
    assert_eq!(location.node(CLOUD).profile_version(), version, "the cloud takes its own");
    let frames = location.node(CLOUD).take_frames().unwrap();
    location.send(CLOUD, frames);
    location.deliver();
    location.run(40);
    for n in [HUB, REGISTER] {
        assert_eq!(location.node(n).profile_version(), version, "{n:x} took it");
        assert_eq!(location.node(n).profile().encode(), published.encode());
    }
    // The register's next line is priced by it; the line before keeps its price.
    let ticket =
        location.node(REGISTER).add_item(order, id(ESPRESSO), &[], NonZeroU32::MIN).unwrap();
    let amounts: Vec<i64> = ticket.lines.iter().map(|line| line.amount.money.minor()).collect();
    assert_eq!(amounts, [325, 410]);
    // Still signed in: the team is the same.
    assert_eq!(location.node(REGISTER).signed_in(), Some(id(0x501)));

    // Restarted, the register opens with the profile it took, once replication starts.
    let mut nodes = location.nodes;
    drop(nodes.remove(&REGISTER));
    let mut register = open(&register_dir, REGISTER);
    assert_eq!(register.profile_version(), provisioned, "the provisioned profile, until then");
    register.start_sync(roster(CLOUD)).unwrap();
    assert_eq!(register.profile_version(), version);
    // Or before, as soon as the shell gives it the roster, while it can't join its network.
    // Found in review: a register that couldn't join rang from the provisioned profile.
    drop(register);
    let mut register = open(&register_dir, REGISTER);
    assert!(register.take_published(&roster(CLOUD)).unwrap());
    assert_eq!(register.profile_version(), version);
    assert!(!register.take_published(&roster(CLOUD)).unwrap(), "nothing more to take");
    // Under a roster that names another cloud, the publications are looked for in its log.
    drop(register);
    let mut register = open(&register_dir, REGISTER);
    assert!(!register.take_published(&roster(REGISTER)).unwrap());
    assert_eq!(register.profile_version(), provisioned);
    // And back under the location's: its cloud's log is looked at from the start, wherever the
    // look at the other's ended.
    assert!(register.take_published(&roster(CLOUD)).unwrap());
    assert_eq!(register.profile_version(), version);
    let mut data = roster(CLOUD).data().clone();
    data.location = id(0xbad);
    assert!(matches!(
        register.take_published(&Roster::new(data).unwrap()),
        Err(RuntimeError::RosterMismatch)
    ));
}

#[test]
fn a_later_publication_replaces_an_earlier_one() {
    let (cloud_dir, hub_dir, register_dir) =
        (dir("later-cloud"), dir("later-hub"), dir("later-reg"));
    let mut location = three(&cloud_dir, &hub_dir, &register_dir);
    location.node(CLOUD).publish_profile(&espresso_at(410)).unwrap();
    let latest = location.node(CLOUD).publish_profile(&espresso_at(395)).unwrap();
    let frames = location.node(CLOUD).take_frames().unwrap();
    location.send(CLOUD, frames);
    location.deliver();
    location.run(40);
    assert_eq!(location.node(REGISTER).profile_version(), latest);
    // Publishing the provisioned profile again takes it back.
    let back = location.node(CLOUD).publish_profile(&demo().unwrap()).unwrap();
    let frames = location.node(CLOUD).take_frames().unwrap();
    location.send(CLOUD, frames);
    location.deliver();
    location.run(40);
    assert_eq!(location.node(REGISTER).profile_version(), back);
    assert_eq!(back, keel_domain::publication::version(&demo().unwrap()));
}

#[test]
fn only_the_roster_s_cloud_publishes() {
    let (cloud_dir, hub_dir, register_dir) = (dir("only-cloud"), dir("only-hub"), dir("only-reg"));
    let mut location = three(&cloud_dir, &hub_dir, &register_dir);
    let provisioned = location.node(REGISTER).profile_version();
    for n in [HUB, REGISTER] {
        assert!(matches!(
            location.node(n).publish_profile(&espresso_at(410)),
            Err(RuntimeError::NotCloud)
        ));
    }
    // Another location's profile isn't the cloud's to publish.
    let mut data = demo().unwrap().data().clone();
    data.location = id(0xbad);
    let elsewhere = Profile::new(data).unwrap();
    assert!(matches!(
        location.node(CLOUD).publish_profile(&elsewhere),
        Err(RuntimeError::RosterMismatch)
    ));
    // A cloud that hasn't settled its log doesn't write to it: it waits to hear the hub, or
    // to be an island. Found in review: it published before it heard the hub.
    let lonely = dir("only-lonely");
    let mut cloud = open(&lonely, CLOUD);
    cloud.start_sync(roster(CLOUD)).unwrap();
    assert!(matches!(cloud.publish_profile(&espresso_at(410)), Err(RuntimeError::NotSettled)));
    cloud.clock().advance(keel_runtime::ISLAND_AFTER).unwrap();
    assert!(cloud.publish_profile(&espresso_at(410)).is_ok());
    // Before replication starts, the runtime doesn't know who the cloud is.
    let stray = dir("only-stray");
    assert!(matches!(
        open(&stray, CLOUD).publish_profile(&espresso_at(410)),
        Err(RuntimeError::NotSyncing)
    ));
    drop(location);
    // A device whose own log holds a publication, written under a roster that named it the
    // cloud: under the location's roster, which names another, it never takes it.
    let impostor = dir("only-impostor");
    let mut register = open(&impostor, REGISTER);
    register.start_sync(roster(REGISTER)).unwrap();
    register.clock().advance(keel_runtime::ISLAND_AFTER).unwrap();
    let forged = register.publish_profile(&espresso_at(1)).unwrap();
    assert_eq!(register.profile_version(), forged);
    drop(register);
    let mut register = open(&impostor, REGISTER);
    register.start_sync(roster(CLOUD)).unwrap();
    assert_eq!(register.profile_version(), provisioned);
}
