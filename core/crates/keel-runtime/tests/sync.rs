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
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use keel_domain::codec::Name;
use keel_domain::order::Mode;
use keel_domain::profile::demo;
use keel_domain::roster::{HubAddress, Roster, RosterData, RosterDevice};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_runtime::{Identity, Runtime, RuntimeError};
use keel_store::{Faults, NoFaults, Point, StoreKey};
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
        cloud: None,
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
    open_with(dir, n, seed, NoFaults)
}

fn open_with(dir: &Dir, n: u64, seed: u8, faults: impl Faults + Send + 'static) -> TestRuntime {
    Runtime::open_with_faults(
        dir.0.join("store.db"),
        StoreKey::new(&mut [0x4b; 32]),
        Identity { device: id(n), signer: signer(seed) },
        demo().unwrap(),
        ManualClock::new("2026-10-02T13:00:00Z".parse().unwrap()),
        SeededEntropy::new(u64::from(seed)),
        SeededEntropy::new(u64::from(seed) + 100),
        Locale::EN_US,
        faults,
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
            // A store error loses the frame, as a shell's loop goes on after reporting it.
            if let Ok(answer) = target.on_frame(from, &frame.frame) {
                self.send(n, answer);
            }
        }
    }

    /// Moves both clocks on by `step`, `times` times, ticking each runtime when it asks.
    fn run(&mut self, step: Duration, times: u32) {
        for _ in 0..times {
            self.hub.clock().advance(step).unwrap();
            self.register.clock().advance(step).unwrap();
            for n in [HUB, REGISTER] {
                let runtime = if n == HUB { &mut self.hub } else { &mut self.register };
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

#[test]
fn a_sale_on_the_register_reaches_the_hub() {
    let (hub_dir, register_dir) = (dir("hub"), dir("register"));
    let mut hub = open(&hub_dir, HUB, 1);
    let mut register = open(&register_dir, REGISTER, 2);
    register.sign_in(id(0x501), "1234").unwrap();
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
    let status = location.register.sync_status().unwrap().unwrap();
    assert!(status.settled && status.hub_reachable && !status.is_hub, "{status:?}");
    let hub_status = location.hub.sync_status().unwrap().unwrap();
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

/// A hub and a register started and settled, the hub serving.
fn settled(hub: TestRuntime, register: TestRuntime) -> Location {
    let mut location = Location { hub, register, wire: VecDeque::new() };
    let frames = location.hub.start_sync(roster()).unwrap();
    location.send(HUB, frames);
    let frames = location.register.start_sync(roster()).unwrap();
    location.send(REGISTER, frames);
    location.deliver();
    location.run(Duration::from_millis(250), 40);
    assert!(location.hub.sync_status().unwrap().unwrap().serving);
    location.register.sign_in(id(0x501), "1234").unwrap();
    location
}

/// Rings and pays a sale on the register, and pushes it.
fn sale(location: &mut Location) {
    let order = location.register.start_order(Mode::Takeout).unwrap().order;
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    location.register.pay_cash(order, Money::from_minor(1_000, Currency::USD)).unwrap();
    let frames = location.register.take_frames().unwrap();
    location.send(REGISTER, frames);
    location.deliver();
}

/// Whether the hub has numbered all the register wrote.
fn confirmed(location: &Location) -> bool {
    let register = &location.register;
    let head = register.store().head(id(REGISTER)).unwrap().seq();
    register.store().confirmed().unwrap().get(&id(REGISTER)).copied() == Some(head)
}

/// Found in review: the loop ticked only when the next tick's time had come, which a clock set
/// back put off for as long as it went back, so the hub fell silent.
#[test]
fn a_clock_set_back_doesnt_silence_the_hub() {
    let (hub_dir, register_dir) = (dir("back-hub"), dir("back-register"));
    let mut location = settled(open(&hub_dir, HUB, 1), open(&register_dir, REGISTER, 2));
    let back: keel_types::Timestamp = "2026-10-02T12:00:00Z".parse().unwrap();
    location.hub.clock().set(back);
    location.run(Duration::from_millis(250), 40);
    assert!(
        location.register.sync_status().unwrap().unwrap().hub_reachable,
        "the register hears its hub"
    );
    sale(&mut location);
    location.run(Duration::from_millis(250), 8);
    assert!(confirmed(&location));
}

/// Found in review: a register that heard no peer never settled, so it could never sell, where
/// ADR-0019 says a device that can reach no peer is an island and must.
#[test]
fn a_register_that_hears_no_one_sells_as_an_island() {
    let register_dir = dir("island");
    let mut register = open(&register_dir, REGISTER, 2);
    register.sign_in(id(0x501), "1234").unwrap();
    let _ = register.start_sync(roster()).unwrap();
    register
        .clock()
        .advance(keel_runtime::ISLAND_AFTER.checked_sub(Duration::from_millis(1)).unwrap())
        .unwrap();
    assert!(matches!(register.start_order(Mode::Takeout), Err(RuntimeError::NotSettled)));
    register.clock().advance(Duration::from_millis(1)).unwrap();
    let status = register.sync_status().unwrap().unwrap();
    assert!(!status.settled && !status.hub_reachable, "{status:?}");
    assert!(register.start_order(Mode::Takeout).is_ok(), "an island sells");
}

/// Fails the `n`th write to begin once armed.
#[derive(Clone)]
struct FailLater(Arc<AtomicU32>);

impl Faults for FailLater {
    fn proceed(&mut self, point: Point) -> bool {
        if point != Point::Began {
            return true;
        }
        // 0: not armed; 1: fail this write; more: count down.
        match self.0.load(Ordering::SeqCst) {
            0 => true,
            1 => {
                self.0.store(0, Ordering::SeqCst);
                false
            }
            n => {
                self.0.store(n - 1, Ordering::SeqCst);
                true
            }
        }
    }
}

/// Found in review: when the hub stored a sale and then failed to number it, nothing numbered it
/// until another new event came, since the register never sends what the hub holds again.
#[test]
fn a_numbering_that_fails_is_tried_again() {
    let (hub_dir, register_dir) = (dir("owed-hub"), dir("owed-register"));
    let armed = Arc::new(AtomicU32::new(0));
    let hub = open_with(&hub_dir, HUB, 1, FailLater(Arc::clone(&armed)));
    let mut location = settled(hub, open(&register_dir, REGISTER, 2));
    // The hub's next write stores the sale; the one after it, which numbers it, fails.
    armed.store(2, Ordering::SeqCst);
    sale(&mut location);
    assert_eq!(armed.load(Ordering::SeqCst), 0, "the numbering failed");
    location.run(Duration::from_millis(250), 12);
    assert!(confirmed(&location), "and was tried again");
}

/// Found in review: a hub the roster no longer lets host the hub went on serving, so the device
/// the roster now names never claimed.
#[test]
fn a_hub_the_roster_demotes_stops_serving() {
    let (hub_dir, register_dir) = (dir("demoted-hub"), dir("demoted-register"));
    let Location { hub, register, .. } =
        settled(open(&hub_dir, HUB, 1), open(&register_dir, REGISTER, 2));
    let now = hub.clock().now();
    drop(hub);
    // The hub, opened again with a roster that no longer lets it host the hub, holds the winning
    // claim, settles beside its register, and doesn't serve.
    let hub = open(&hub_dir, HUB, 1);
    hub.clock().set(now);
    let mut location = Location { hub, register, wire: VecDeque::new() };
    // The role moves to the register, as a roster must give it to some device.
    let mut data = roster().data().clone();
    data.devices[0].hub = None;
    data.devices[1].hub = NonZeroU8::new(1);
    let frames = location.hub.start_sync(Roster::new(data).unwrap()).unwrap();
    location.send(HUB, frames);
    location.deliver();
    location.run(Duration::from_millis(250), 40);
    let status = location.hub.sync_status().unwrap().unwrap();
    assert!(status.settled && status.is_hub && !status.serving, "{status:?}");
}

#[test]
fn a_hub_that_restarts_numbers_what_comes_after() {
    let (hub_dir, register_dir) = (dir("restart-hub"), dir("restart-register"));
    let mut location = settled(open(&hub_dir, HUB, 1), open(&register_dir, REGISTER, 2));
    sale(&mut location);
    location.run(Duration::from_millis(250), 4);
    assert!(confirmed(&location));
    let Location { hub, register, .. } = location;
    let now = hub.clock().now();
    drop(hub);
    let hub = open(&hub_dir, HUB, 1);
    hub.clock().set(now);
    let mut location = Location { hub, register, wire: VecDeque::new() };
    let frames = location.hub.start_sync(roster()).unwrap();
    location.send(HUB, frames);
    location.deliver();
    location.run(Duration::from_millis(250), 40);
    sale(&mut location);
    location.run(Duration::from_millis(250), 8);
    assert!(confirmed(&location), "the restarted hub numbers the next sale");
}

/// Pushes what each runtime wrote, and delivers it, and what it causes, until the wire is quiet.
fn exchange(location: &mut Location) {
    for _ in 0..4 {
        let frames = location.hub.take_frames().unwrap();
        location.send(HUB, frames);
        let frames = location.register.take_frames().unwrap();
        location.send(REGISTER, frames);
        location.deliver();
        location.run(Duration::from_millis(250), 2);
    }
}

/// An order parked on one device is recalled on another (ADR-0030, decision 6): it asks the hub,
/// and owns the order once the hub grants it; until then, only the owner changes it.
#[test]
fn an_order_parked_on_one_device_is_recalled_on_another_through_the_hub() {
    let (hub_dir, register_dir) = (dir("park-hub"), dir("park-register"));
    let mut location = settled(open(&hub_dir, HUB, 1), open(&register_dir, REGISTER, 2));
    location.hub.sign_in(id(0x501), "1234").unwrap();
    // Rung at the back office's device, and parked there.
    let order = location.hub.start_order(Mode::Takeout).unwrap().order;
    location.hub.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    exchange(&mut location);

    // The register lists it, owned by the other device.
    let open = location.register.open_orders().unwrap();
    let parked = open.iter().find(|ticket| ticket.order == order).unwrap();
    assert_eq!(parked.owner, Some(id(HUB)));
    assert!(parked.asking.is_empty());
    // Until it owns it, the register can't change what it holds.
    let line = parked.lines[0].line;
    assert!(matches!(
        location.register.remove_line(order, line),
        Err(RuntimeError::Order(keel_domain::order::CommandError::NotOwner(_)))
    ));

    // It recalls it: the request waits for the hub's answer, and asking again asks nothing more.
    let asked = location.register.recall(order).unwrap();
    assert_eq!(asked.asking, [id(REGISTER)]);
    assert_eq!(location.register.recall(order).unwrap().asking, [id(REGISTER)]);
    exchange(&mut location);

    // Granted: the register owns it, and rings on it.
    let recalled = location.register.ticket(order).unwrap();
    assert_eq!(recalled.owner, Some(id(REGISTER)), "{recalled:?}");
    assert!(recalled.asking.is_empty());
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    let paid = location.register.pay_cash(order, Money::from_minor(1_000, Currency::USD)).unwrap();
    assert_eq!(paid.ticket.lines.len(), 2);
    // Recalling an order it owns asks for nothing.
    assert!(location.register.recall(order).is_ok());
}

/// A device that can't wait for the hub takes the order on a manager's word (ADR-0021): gated as
/// a void is, and only from a device that doesn't own it already.
#[test]
fn an_order_is_taken_over_on_a_managers_word() {
    let (hub_dir, register_dir) = (dir("take-hub"), dir("take-register"));
    let mut location = settled(open(&hub_dir, HUB, 1), open(&register_dir, REGISTER, 2));
    location.hub.sign_in(id(0x501), "1234").unwrap();
    let order = location.hub.start_order(Mode::Takeout).unwrap().order;
    location.hub.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    exchange(&mut location);
    let reason = || keel_domain::order::Reason {
        code: keel_domain::codec::ReasonCode::new("hub_unreachable").unwrap(),
        note: None,
    };
    // Sam, a cashier, asks; Alex approves.
    location.register.sign_in(id(0x502), "2468").unwrap();
    assert!(matches!(
        location.register.take_over(order, reason()),
        Err(RuntimeError::ApprovalRequired(keel_domain::staff::Action::OrderVoid))
    ));
    location.register.approve(id(0x501), "1234").unwrap();
    let taken = location.register.take_over(order, reason()).unwrap();
    assert_eq!(taken.owner, Some(id(REGISTER)));
    // Its own order is no order to take over, and asks for no approval.
    assert!(matches!(location.register.take_over(order, reason()), Err(RuntimeError::Order(_))));
    assert!(matches!(
        location.register.approve(id(0x501), "1234"),
        Err(RuntimeError::NothingToApprove)
    ));
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    // The hub learns of it: the register owns the order there too.
    exchange(&mut location);
    assert_eq!(location.hub.ticket(order).unwrap().owner, Some(id(REGISTER)));
}

/// An 86 reaches every device of the location within 10 s (ADR-0033, decision 7; G-06): made on
/// one, it is sold out on the other once the frames that tick carries arrive, with no push from
/// the shell; a register polls its availability every 2 s, so it shows once heard, within 2 s.
#[test]
fn an_86_reaches_the_other_device_within_10_seconds() {
    use keel_runtime::{ItemRef, Until};
    let (hub_dir, register_dir) = (dir("86-hub"), dir("86-register"));
    let mut location = settled(open(&hub_dir, HUB, 1), open(&register_dir, REGISTER, 2));
    location.hub.sign_in(id(0x501), "1234").unwrap();
    let espresso = ItemRef::Variant(id(0x401));
    let step = Duration::from_millis(250);
    // Polled every step, as the shell does every 2 s: how long until `heard` holds.
    let until_heard = |location: &mut Location, heard: &dyn Fn(&Location) -> bool| {
        let mut waited = Duration::ZERO;
        while !heard(location) {
            // In-store, within a second (offline and sync §6.5).
            assert!(waited <= Duration::from_secs(1), "not heard after {waited:?}");
            location.run(step, 1);
            waited += step;
        }
        waited
    };
    let sold_out = |runtime: &TestRuntime| !runtime.sold_out().unwrap().is_empty();

    // Made on the hub, heard by the register.
    location.hub.eighty_six(espresso, Until::EndOfDay, None).unwrap();
    let waited = until_heard(&mut location, &|location| sold_out(&location.register));
    assert!(waited + Duration::from_secs(2) <= Duration::from_secs(10), "{waited:?}");
    assert_eq!(location.register.availability_version().unwrap(), 1);
    let order = location.register.start_order(Mode::Takeout).unwrap().order;
    assert!(matches!(
        location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN),
        Err(RuntimeError::SoldOut(_))
    ));

    // Put back on the register, heard by the hub.
    location.register.put_back(espresso).unwrap();
    let waited = until_heard(&mut location, &|location| !sold_out(&location.hub));
    assert!(waited + Duration::from_secs(2) <= Duration::from_secs(10), "{waited:?}");
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();

    // Changed on both at once: each device ends with the same, the later by HLC: the register's,
    // until the day ends.
    location.hub.eighty_six(espresso, Until::PutBack, None).unwrap();
    location.register.clock().advance(Duration::from_millis(1)).unwrap();
    location.register.eighty_six(espresso, Until::EndOfDay, None).unwrap();
    location.run(step, 8);
    let hub = location.hub.sold_out().unwrap();
    assert_eq!(hub, location.register.sold_out().unwrap());
    assert_eq!(hub.len(), 1);
    assert!(hub[0].until.is_some(), "the later change stands: {hub:?}");
    assert_eq!(
        location.hub.availability_version().unwrap(),
        location.register.availability_version().unwrap()
    );
    // Put back on one and 86'd on the other at once: the 86, the later, stands on both.
    location.hub.clock().advance(Duration::from_millis(1)).unwrap();
    location.hub.put_back(espresso).unwrap();
    location.register.clock().advance(Duration::from_millis(2)).unwrap();
    location.register.eighty_six(espresso, Until::PutBack, None).unwrap();
    location.run(step, 8);
    let hub = location.hub.sold_out().unwrap();
    assert_eq!(hub, location.register.sold_out().unwrap());
    assert_eq!(hub.len(), 1);
    assert_eq!(hub[0].until, None, "{hub:?}");
    // And the other way: an 86 and a later put-back leave it on sale on both.
    location.register.clock().advance(Duration::from_millis(1)).unwrap();
    location.register.eighty_six(espresso, Until::EndOfDay, None).unwrap();
    location.hub.clock().advance(Duration::from_millis(5)).unwrap();
    location.hub.put_back(espresso).unwrap();
    location.run(step, 8);
    assert!(location.hub.sold_out().unwrap().is_empty());
    assert!(location.register.sold_out().unwrap().is_empty());
}

/// A ticket sent on the register reaches a screen on another device as soon as the register
/// pushes it, and the screen's showing reaches the register, so it doesn't reroute what the
/// screen showed (ADR-0034, decisions 4 and 5; offline and sync §9: fired to visible on a KDS
/// through the hub). A register that can't hear the hub reroutes, 10 s on.
#[test]
fn a_ticket_reaches_a_screen_through_the_hub_and_its_showing_stops_the_reroute() {
    let (hub_dir, register_dir) = (dir("kds-hub"), dir("kds-register"));
    let mut location = settled(open(&hub_dir, HUB, 1), open(&register_dir, REGISTER, 2));
    let (bar, expo) = (id(0x701), id(0x703));
    let order = location.register.start_order(Mode::Takeout).unwrap().order;
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    location.register.send(order).unwrap();
    // Pushed at once, as a sale is: the screen has it with no time passing.
    let frames = location.register.take_frames().unwrap();
    location.send(REGISTER, frames);
    location.deliver();
    // The screen shows the bar and expo, which the espresso goes to.
    let screen = location.hub.kitchen_screen(&[bar, expo]).unwrap();
    assert_eq!(screen.tickets.len(), 2, "the screen has the tickets");
    let frames = location.hub.take_frames().unwrap();
    location.send(HUB, frames);
    location.deliver();
    // Only the order's owner sends it. Found in review: two devices sending one order sent its
    // stations two tickets.
    location.hub.sign_in(id(0x501), "1234").unwrap();
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    let frames = location.register.take_frames().unwrap();
    location.send(REGISTER, frames);
    location.deliver();
    assert!(matches!(
        location.hub.send(order),
        Err(RuntimeError::Order(keel_domain::order::CommandError::NotOwner(_)))
    ));
    location.register.send(order).unwrap();
    let frames = location.register.take_frames().unwrap();
    location.send(REGISTER, frames);
    location.deliver();
    location.hub.kitchen_screen(&[bar, expo]).unwrap();
    let frames = location.hub.take_frames().unwrap();
    location.send(HUB, frames);
    location.deliver();
    // 10 s on, the register reroutes nothing: it heard the screen show it.
    location.run(Duration::from_millis(250), 44);
    location.register.next_print_job().unwrap();
    assert!(location.register.kitchen_alerts().unwrap().is_empty());
    // An order the register sends while it can't hear the hub: rerouted, 10 s on; the screen
    // hears of it, and of the reroute, once the register hears the hub again.
    let order = location.register.start_order(Mode::Takeout).unwrap().order;
    location.register.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    location.register.send(order).unwrap();
    location.register.clock().advance(Duration::from_secs(10)).unwrap();
    location.register.next_print_job().unwrap();
    let alerts = location.register.kitchen_alerts().unwrap();
    assert_eq!(alerts.len(), 2, "the bar's and expo's: {alerts:#?}");
    let frames = location.register.take_frames().unwrap();
    location.send(REGISTER, frames);
    location.deliver();
    let heard = location.hub.kitchen_alerts().unwrap();
    assert_eq!(heard.len(), 2, "{heard:#?}");
    // The bar's three: the first order's two sendings, shown, and the third, rerouted.
    let screen = location.hub.kitchen_screen(&[bar]).unwrap();
    assert_eq!(screen.tickets.len(), 3);
    assert!(screen.tickets[..2].iter().all(|ticket| ticket.rerouted.is_none()));
    assert!(screen.tickets[2].rerouted.is_some());
}
