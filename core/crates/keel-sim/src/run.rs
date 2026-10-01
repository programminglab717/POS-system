//! A run: the scheduler, the network, the faults, and the history the invariants are judged
//! against.
//!
//! Everything happens in one thread, in virtual time, in the order a queue of actions keyed by
//! time gives: frames arriving, replicators ticking, devices working, faults striking. Every
//! random choice comes from the run's generator, so a seed replays exactly.
//!
//! A node's replicator ticks when it says it next needs to, as a device's would: after each
//! thing it handles, the node sets a timer for [`keel_sync::Replicator::next_tick`], and the
//! timer runs in virtual time, whatever the node's clock does meanwhile.

use core::fmt::Write as _;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use keel_events::envelope::{Device, Event};
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::verify::DeviceRegistry;
use keel_store::{Point, StoreError};
use keel_sync::{Outgoing, StoreReplica, SyncConfig, VersionVector};
use keel_types::{Hlc, Id};

use crate::check;
use crate::config::Config;
use crate::monitor::Monitor;
use crate::node::{CLOUD, HUB, Node, SimError, device, registry, text};
use crate::rng::Rng;
use crate::workload::{self, Made, Move, Station, WorkError};

/// How long a device waits for its own log to settle before it works anyway, as an island.
pub(crate) const SETTLE_WAIT: i64 = 3_000;
/// The furthest a clock may be from virtual time, in milliseconds: well within the drift the
/// kernel tolerates, so no device ignores another's clock.
const MOST_OFFSET: i64 = 25_000;
/// The most actions a run may take before it is judged stuck.
const MOST_ACTIONS: u64 = 5_000_000;

/// Something that happens at a time.
#[derive(Clone, Debug)]
enum Action {
    Deliver { from: u8, to: u8, frame: Vec<u8> },
    Tick { node: u8, incarnation: u64 },
    Work { node: u8, incarnation: u64 },
    Crash { node: u8, mid_write: bool, down: i64 },
    Restart { node: u8 },
    Snapshot { node: u8 },
    Rollback { node: u8, down: i64 },
    Jump { node: u8, by: i64 },
    Heal,
}

impl Action {
    /// The action's kind and nodes, for the trace.
    fn describe(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        match self {
            Action::Deliver { from, to, frame } => {
                bytes.extend([0, *from, *to]);
                bytes.extend(EventHash::of(frame).as_bytes());
            }
            Action::Tick { node, .. } => bytes.extend([1, *node]),
            Action::Work { node, .. } => bytes.extend([2, *node]),
            Action::Crash { node, mid_write, .. } => bytes.extend([3, *node, u8::from(*mid_write)]),
            Action::Restart { node } => bytes.extend([4, *node]),
            Action::Snapshot { node } => bytes.extend([5, *node]),
            Action::Rollback { node, .. } => bytes.extend([6, *node]),
            Action::Jump { node, by } => {
                bytes.extend([7, *node]);
                bytes.extend(by.to_be_bytes());
            }
            Action::Heal => bytes.push(8),
        }
        bytes
    }
}

/// A link cut for a while.
#[derive(Clone, Copy, Debug)]
struct Cut {
    a: u8,
    b: u8,
    from: i64,
    to: i64,
}

/// An event a device appended, and what the device held when it did.
#[derive(Clone, Debug)]
pub(crate) struct Appended {
    pub(crate) node: u8,
    pub(crate) event: SignedEvent,
    /// The latest HLC of every event the device held just before.
    pub(crate) after: Hlc,
    pub(crate) at: i64,
}

/// A device's store restored from an older copy: the device, and the last position of its own
/// log the copy held.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rollback {
    pub(crate) node: u8,
    pub(crate) kept: u64,
    pub(crate) at: i64,
}

/// What a run did, for tests and coverage probes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The run's seed.
    pub seed: u64,
    /// A digest of every action taken, in order: the same seed must give the same trace.
    pub trace: [u8; 32],
    /// How long after healing the replicas agreed, in milliseconds.
    pub agreed_after: i64,
    /// Events the devices appended.
    pub events: u64,
    /// Each move: how many times it appended, was refused, and found nothing to act on.
    pub moves: BTreeMap<Move, [u64; 3]>,
    /// Frames sent, lost, duplicated, dropped by a cut link, and sent to a node that was down.
    pub frames: [u64; 5],
    /// Crashes between writes, crashes in the middle of one, rollbacks, clock jumps, and cuts.
    pub faults: [u64; 5],
    /// Events lost to rollbacks, held by no replica but their device's.
    pub lost: u64,
    /// Devices that wrote before their log settled after a rollback, as islands.
    pub forked: u64,
    /// Writes a device held back until its log settled.
    pub held_back: u64,
    /// The replicators' gaps, duplicates, timeouts and stalls, summed.
    pub replication: [u64; 4],
    /// The longest an event took to reach every replica, in milliseconds, among the events whose
    /// last arrival came while no node was down and no link was cut. In a run without faults,
    /// that is every event.
    pub max_lag: i64,
    /// The hub's sequencing records, and the events they number.
    pub sequenced: [u64; 2],
    /// `durable` frames sent.
    pub durable_frames: u64,
    /// The requests for orders the hub held (ADR-0021), its grants, and its refusals because the
    /// lease had moved on, the device already owned the order, and a payment was in progress.
    pub answers: [u64; 5],
    /// In the orders' folds on the hub: overrides that applied, stale overrides, stale grants,
    /// and events recorded by a device that didn't own the order.
    pub ownership: [u64; 4],
}

/// A failed run: its seed, and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The seed that fails.
    pub seed: u64,
    /// What went wrong.
    pub what: String,
}

impl core::fmt::Display for Failure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "seed {}: {}", self.seed, self.what)
    }
}

/// Runs `config`, and checks every invariant.
///
/// # Errors
/// [`Failure`] if an invariant fails, or the simulation can't go on.
pub fn simulate(config: &Config) -> Result<Report, Failure> {
    let fail = |what: String| Failure { seed: config.seed, what };
    let base = Scratch::new(config.seed).map_err(|error| fail(error.to_string()))?;
    let mut run = Run::new(*config, base.0.clone()).map_err(|error| fail(error.to_string()))?;
    run.go().map_err(|error| fail(error.to_string()))?;
    check::after(&mut run).map_err(fail)?;
    run.report.trace = *run.trace.as_bytes();
    Ok(run.report.clone())
}

/// A directory of the run's own, in memory where there is one, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(seed: u64) -> Result<Scratch, SimError> {
        use core::sync::atomic::{AtomicU64, Ordering};
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let shm = std::path::Path::new("/dev/shm");
        let root = if shm.is_dir() { shm.to_path_buf() } else { std::env::temp_dir() };
        let dir = root.join(format!("keel-sim-{}-{seed}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|error| SimError::Files(error.to_string()))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A run in progress.
pub(crate) struct Run {
    pub(crate) config: Config,
    rng: Rng,
    pub(crate) now: i64,
    queue: BTreeMap<(i64, u64), Action>,
    sequence: u64,
    pub(crate) nodes: BTreeMap<u8, Node>,
    pub(crate) registry: DeviceRegistry,
    sync: SyncConfig,
    /// The protocol's rules for batches, checked as frames are sent and received.
    monitor: Monitor,
    /// The first rule the run broke, if it broke one.
    broken: Option<String>,
    cuts: Vec<Cut>,
    pub(crate) healed: Option<i64>,
    trace: EventHash,
    pub(crate) report: Report,
    pub(crate) appended: Vec<Appended>,
    pub(crate) rollbacks: Vec<Rollback>,
    /// Devices that wrote before their log settled after a rollback.
    pub(crate) forked: BTreeSet<u8>,
    /// Where each device's own log stood when its snapshot was taken.
    snapshot_heads: BTreeMap<u8, u64>,
    /// For the lag: when each event was appended, and which nodes hold it so far.
    pending: BTreeMap<(Id<Device>, u64), (i64, BTreeSet<u8>)>,
    /// What each node held when the simulator last looked.
    seen: BTreeMap<u8, VersionVector>,
    /// Each node's durable-ack watermark when the simulator last looked, since it last started.
    watermarks: BTreeMap<u8, VersionVector>,
    /// How far into its own log each device's replicator, since it last started, has said its
    /// events are store-durable, and which events that made store-durable: the ones its device
    /// last appended at those positions.
    durable_marked: BTreeMap<u8, u64>,
    pub(crate) store_durable: BTreeSet<Id<Event>>,
    /// The event each device last appended at each position of its log.
    latest: BTreeMap<(u8, u64), Id<Event>>,
    pub(crate) base: PathBuf,
    /// Whether to print every action, when `KEEL_SIM_LOG` is set.
    log: bool,
}

impl Run {
    fn new(config: Config, base: PathBuf) -> Result<Run, SimError> {
        let devices: Vec<u8> = (1..=config.devices).collect();
        let all: Vec<u8> = devices.iter().copied().chain([HUB, CLOUD]).collect();
        let mut rng = Rng::new(config.seed);
        let mut nodes = BTreeMap::new();
        for &n in &all {
            // A star: devices to the hub, the hub to the cloud.
            let peers =
                if n == HUB { devices.iter().copied().chain([CLOUD]).collect() } else { vec![HUB] };
            let mut node = Node::new(n, peers, &base, rng.next())?;
            node.offset = rng.between(-20_000, 20_001);
            nodes.insert(n, node);
        }
        let batch_events = u32::try_from(rng.between(4, 65)).unwrap_or(16);
        let sync = SyncConfig { batch_events, ..SyncConfig::DEFAULT };
        Ok(Run {
            config,
            rng,
            now: 0,
            queue: BTreeMap::new(),
            sequence: 0,
            nodes,
            registry: registry(&all)?,
            sync,
            monitor: Monitor::new(sync),
            broken: None,
            cuts: Vec::new(),
            healed: None,
            trace: EventHash::ZERO,
            report: Report { seed: config.seed, ..Report::default() },
            appended: Vec::new(),
            rollbacks: Vec::new(),
            forked: BTreeSet::new(),
            snapshot_heads: BTreeMap::new(),
            pending: BTreeMap::new(),
            seen: BTreeMap::new(),
            watermarks: BTreeMap::new(),
            durable_marked: BTreeMap::new(),
            store_durable: BTreeSet::new(),
            latest: BTreeMap::new(),
            base,
            log: std::env::var_os("KEEL_SIM_LOG").is_some(),
        })
    }

    fn at(&mut self, time: i64, action: Action) {
        self.sequence = self.sequence.saturating_add(1);
        self.queue.insert((time, self.sequence), action);
    }

    /// Starts every node, plans the faults, and runs until the replicas agree after healing.
    fn go(&mut self) -> Result<(), SimError> {
        let ids: Vec<u8> = self.nodes.keys().copied().collect();
        for &n in &ids {
            self.start(n)?;
        }
        self.plan_faults();
        let working = self.config.working;
        self.at(working, Action::Heal);
        let mut actions: u64 = 0;
        while let Some(((time, _), action)) = self.queue.pop_first() {
            actions = actions.saturating_add(1);
            if actions > MOST_ACTIONS {
                return Err(SimError::Setup(format!("stuck after {actions} actions")));
            }
            if let Some(healed) = self.healed
                && time > healed.saturating_add(self.config.agree_within)
            {
                return Err(SimError::Broken(format!(
                    "the replicas didn't agree within {} ms of healing: {}",
                    self.config.agree_within,
                    self.state()
                )));
            }
            self.now = time;
            if self.log {
                #[allow(clippy::print_stderr, reason = "KEEL_SIM_LOG asks for every action")]
                {
                    eprintln!("{time:>7} {}", Self::shown(&action));
                }
            }
            let mut entry = time.to_be_bytes().to_vec();
            entry.extend(action.describe());
            let mut chained = self.trace.as_bytes().to_vec();
            chained.extend(entry);
            self.trace = EventHash::of(&chained);
            self.execute(action)?;
            if let Some(broken) = self.broken.take() {
                return Err(SimError::Broken(format!("at {time} ms: {broken}")));
            }
            if let Some(healed) = self.healed
                && self.agreed()
            {
                self.report.agreed_after = time.saturating_sub(healed);
                return Ok(());
            }
        }
        Err(SimError::Broken("the run ran out of actions before the replicas agreed".to_owned()))
    }

    /// Whether every node is up and settled, and holds what every other node holds. A device
    /// whose log forked is left out: replicas hold different versions of its log for good, and
    /// it can't settle.
    fn agreed(&self) -> bool {
        let forked: BTreeSet<Id<Device>> = self.forked.iter().map(|&n| device(n)).collect();
        let mut vectors = Vec::new();
        for (n, node) in &self.nodes {
            let Some(replicator) = &node.replicator else { return false };
            if !replicator.settled() && !self.forked.contains(n) {
                return false;
            }
            let vector: VersionVector = replicator
                .version_vector()
                .iter()
                .filter(|(origin, _)| !forked.contains(origin))
                .map(|(origin, position)| (*origin, *position))
                .collect();
            vectors.push(vector);
        }
        // Every watermark but the cloud's own reaches everything the cloud holds.
        let cloud = self.cloud_holds();
        let watermarks = self
            .nodes
            .iter()
            .filter(|(n, _)| **n != CLOUD)
            .all(|(_, node)| node.replicator.as_ref().is_some_and(|r| r.durable() == &cloud));
        watermarks && vectors.iter().zip(vectors.iter().skip(1)).all(|(a, b)| a == b)
    }

    /// What the cloud holds: as its replicator knows it, or, while it is down, as it did when it
    /// went down. Its store is never restored from an older copy.
    fn cloud_holds(&self) -> VersionVector {
        self.nodes.get(&CLOUD).map_or_else(VersionVector::new, |cloud| {
            cloud
                .replicator
                .as_ref()
                .map_or_else(|| cloud.held.clone(), |r| r.version_vector().clone())
        })
    }

    /// `action`, for the log a failing seed can be replayed with: frames decoded.
    fn shown(action: &Action) -> String {
        let short = |origin: &Id<Device>| origin.to_bytes()[15];
        match action {
            Action::Deliver { from, to, frame } => match keel_sync::Frame::decode(frame) {
                Ok(keel_sync::Frame::Have(have)) => {
                    let vv: Vec<(u8, u64)> = have.vv.iter().map(|(d, p)| (short(d), *p)).collect();
                    let asks = if have.asks { ", asks" } else { "" };
                    format!("{from}->{to} have {vv:?} acked {}{asks}", have.acked)
                }
                Ok(keel_sync::Frame::Events(events)) => {
                    let positions: Vec<(u8, u64)> = events
                        .events
                        .iter()
                        .filter_map(|bytes| SignedEvent::from_stored(bytes).ok())
                        .map(|event| {
                            (short(&event.body().origin_device), event.body().origin_seq.get())
                        })
                        .collect();
                    format!("{from}->{to} batch {} {positions:?}", events.batch)
                }
                Ok(keel_sync::Frame::Durable(durable)) => {
                    let vv: Vec<(u8, u64)> =
                        durable.vv.iter().map(|(d, p)| (short(d), *p)).collect();
                    format!("{from}->{to} durable {vv:?}")
                }
                Err(error) => format!("{from}->{to} undecodable: {error}"),
            },
            other => format!("{other:?}"),
        }
    }

    /// Each node's state, and the devices that forked, for a failure's report.
    fn state(&self) -> String {
        let mut state = String::new();
        for (n, node) in &self.nodes {
            let vector: Vec<(u64, u64)> = node
                .replicator
                .as_ref()
                .map(|r| {
                    r.version_vector()
                        .iter()
                        .map(|(d, p)| (u64::from(d.to_bytes()[15]), *p))
                        .collect()
                })
                .unwrap_or_default();
            let settled = node.replicator.as_ref().is_some_and(keel_sync::Replicator::settled);
            let stats = node.replicator.as_ref().map(keel_sync::Replicator::stats);
            let refused = node
                .store
                .as_ref()
                .and_then(|store| store.quarantine().ok())
                .map(|q| q.iter().map(|refused| refused.reason.code()).collect::<Vec<_>>());
            let _ = write!(state, "{{node {n}: {stats:?}, refused {refused:?}}} ");
            let _ = write!(
                state,
                "[node {n}: up {}, settled {settled}, holds {vector:?}] ",
                node.is_up()
            );
        }
        let _ = write!(state, "forked {:?}, rollbacks {:?}", self.forked, self.rollbacks);
        state
    }

    fn plan_faults(&mut self) {
        let working = self.config.working;
        let devices: Vec<u8> = (1..=self.config.devices).collect();
        let nodes: Vec<u8> = self.nodes.keys().copied().collect();
        let links: Vec<(u8, u8)> =
            devices.iter().map(|&d| (d, HUB)).chain([(HUB, CLOUD)]).collect();
        for _ in 0..self.config.partitions {
            let Some(&(a, b)) = self.rng.pick(&links) else { continue };
            let from = self.rng.between(0, working);
            let to = from.saturating_add(self.rng.between(1_000, 15_000));
            if self.log {
                #[allow(clippy::print_stderr, reason = "KEEL_SIM_LOG asks for every fault")]
                {
                    eprintln!("{from:>7} link {a}-{b} cut until {to}");
                }
            }
            self.cuts.push(Cut { a, b, from, to });
            self.report.faults[4] = self.report.faults[4].saturating_add(1);
        }
        for _ in 0..self.config.crashes {
            let Some(&node) = self.rng.pick(&nodes) else { continue };
            let time = self.rng.between(0, working);
            let mid_write = self.rng.chance(500);
            let down = self.rng.between(200, 8_000);
            self.at(time, Action::Crash { node, mid_write, down });
        }
        for _ in 0..self.config.rollbacks {
            let Some(&node) = self.rng.pick(&devices) else { continue };
            let snapshot = self.rng.between(0, working);
            let rollback = snapshot.saturating_add(self.rng.between(500, 10_000));
            // Longer than any frame takes: none it sent before it went down arrives after it
            // comes back.
            let down = self.rng.between(500, 5_000);
            self.at(snapshot, Action::Snapshot { node });
            self.at(rollback, Action::Rollback { node, down });
        }
        for _ in 0..self.config.clock_jumps {
            let Some(&node) = self.rng.pick(&nodes) else { continue };
            let time = self.rng.between(0, working);
            let by = self.rng.between(-10_000, 10_001);
            self.at(time, Action::Jump { node, by });
        }
    }

    /// Starts node `n`: opens its store, tells its peers what it holds, and sets it ticking and,
    /// if it is a device, working.
    fn start(&mut self, n: u8) -> Result<(), SimError> {
        let now = self.now;
        self.monitor.forget(n);
        self.watermarks.remove(&n);
        self.durable_marked.remove(&n);
        let (out, incarnation) = {
            let Some(node) = self.nodes.get_mut(&n) else { return Ok(()) };
            let out = node.start(now, &self.registry, self.sync)?;
            (out, node.incarnation)
        };
        self.send(n, out);
        self.set_timer(n);
        if n != HUB && n != CLOUD {
            let pace = self.rng.between(self.config.pace.0, self.config.pace.1);
            self.at(now.saturating_add(pace), Action::Work { node: n, incarnation });
        }
        self.observe(n);
        Ok(())
    }

    fn cut(&self, a: u8, b: u8) -> bool {
        self.healed.is_none()
            && self.cuts.iter().any(|cut| {
                ((cut.a, cut.b) == (a, b) || (cut.a, cut.b) == (b, a))
                    && (cut.from..cut.to).contains(&self.now)
            })
    }

    /// Sends `out` from node `from`: each frame after a delay, perhaps lost or duplicated while
    /// the network is faulty.
    fn send(&mut self, from: u8, out: Vec<Outgoing>) {
        let clock = self.nodes.get(&from).map(|node| node.time(self.now));
        for Outgoing { to, frame } in out {
            let Some(&to) = self.nodes.keys().find(|&&n| device(n) == to) else { continue };
            if let Some(clock) = clock
                && let Err(broken) = self.monitor.sending(from, to, &frame, clock)
            {
                self.broken.get_or_insert(broken);
            }
            if let Ok(keel_sync::Frame::Durable(durable)) = keel_sync::Frame::decode(&frame) {
                self.report.durable_frames = self.report.durable_frames.saturating_add(1);
                let cloud = self.cloud_holds();
                if let Some((origin, position)) = durable.vv.iter().find(|(origin, position)| {
                    cloud.get(*origin).is_none_or(|held| held < *position)
                }) {
                    let held = cloud.get(origin).copied().unwrap_or(0);
                    self.broken.get_or_insert(format!(
                        "{from} told {to} the cloud holds {origin:?} up to {position}; it holds {held}"
                    ));
                }
            }
            self.report.frames[0] = self.report.frames[0].saturating_add(1);
            let faulty = self.healed.is_none();
            let copies = if faulty && self.rng.chance(self.config.duplication) {
                self.report.frames[2] = self.report.frames[2].saturating_add(1);
                2
            } else {
                1
            };
            for _ in 0..copies {
                if faulty && self.rng.chance(self.config.loss) {
                    self.report.frames[1] = self.report.frames[1].saturating_add(1);
                    continue;
                }
                let (low, high) = self.config.delay;
                let delay = self.rng.between(low, high.saturating_add(1));
                let time = self.now.saturating_add(delay);
                self.at(time, Action::Deliver { from, to, frame: frame.clone() });
            }
        }
    }

    fn execute(&mut self, action: Action) -> Result<(), SimError> {
        match action {
            Action::Deliver { from, to, frame } => self.deliver(from, to, &frame),
            Action::Tick { node, incarnation } => self.tick(node, incarnation),
            Action::Work { node, incarnation } => self.work(node, incarnation),
            Action::Crash { node, mid_write, down } => {
                self.crash(node, mid_write, down);
                Ok(())
            }
            Action::Restart { node } => {
                if self.nodes.get(&node).is_some_and(|n| !n.is_up()) {
                    self.start(node)?;
                }
                Ok(())
            }
            Action::Snapshot { node } => self.snapshot(node),
            Action::Rollback { node, down } => self.rollback(node, down),
            Action::Jump { node, by } => {
                if let Some(n) = self.nodes.get_mut(&node) {
                    n.offset = n.offset.saturating_add(by).clamp(-MOST_OFFSET, MOST_OFFSET);
                    self.report.faults[3] = self.report.faults[3].saturating_add(1);
                }
                Ok(())
            }
            Action::Heal => self.heal(),
        }
    }

    fn deliver(&mut self, from: u8, to: u8, frame: &[u8]) -> Result<(), SimError> {
        if self.cut(from, to) {
            self.report.frames[3] = self.report.frames[3].saturating_add(1);
            return Ok(());
        }
        let now = self.now;
        let result = {
            let Some(node) = self.nodes.get_mut(&to) else { return Ok(()) };
            let time = node.time(now);
            let (Some(store), Some(replicator)) = (node.store.as_mut(), node.replicator.as_mut())
            else {
                self.report.frames[4] = self.report.frames[4].saturating_add(1);
                return Ok(());
            };
            self.monitor.receiving(from, to, frame, time);
            replicator.on_frame(
                &mut StoreReplica::new(store, &self.registry),
                device(from),
                frame,
                time,
            )
        };
        match result {
            Ok(out) => self.send(to, out),
            Err(error) => self.failed(to, &error)?,
        }
        self.watch(to);
        self.set_timer(to);
        self.observe(to);
        Ok(())
    }

    /// Ticks node `n`'s replicator, unless the node has restarted since the timer was set, or
    /// the timer was reset for earlier.
    fn tick(&mut self, n: u8, incarnation: u64) -> Result<(), SimError> {
        let now = self.now;
        let result = {
            let Some(node) = self.nodes.get_mut(&n) else { return Ok(()) };
            if node.incarnation != incarnation || node.tick_at != Some(now) {
                return Ok(());
            }
            node.tick_at = None;
            let time = node.time(now);
            let (Some(store), Some(replicator)) = (node.store.as_mut(), node.replicator.as_mut())
            else {
                return Ok(());
            };
            self.monitor.ticking(n, time);
            replicator.on_tick(&mut StoreReplica::new(store, &self.registry), time).map(|out| {
                // A `have` a tick sends begins a round with its peer.
                let rounds: Vec<u8> = out
                    .iter()
                    .filter(|out| {
                        matches!(
                            keel_sync::Frame::decode(&out.frame),
                            Ok(keel_sync::Frame::Have(_))
                        )
                    })
                    .filter_map(|out| {
                        self.nodes.keys().find(|&&peer| device(peer) == out.to).copied()
                    })
                    .collect();
                (out, rounds, time)
            })
        };
        match result {
            Ok((out, rounds, time)) => {
                if let Err(broken) = self.monitor.ticked(n, time, &rounds) {
                    self.broken.get_or_insert(broken);
                }
                self.send(n, out);
            }
            Err(error) => self.failed(n, &error)?,
        }
        self.watch(n);
        self.set_timer(n);
        Ok(())
    }

    /// Sets node `n`'s timer for its replicator's next tick, unless it is set for sooner: the
    /// wait from the node's clock to the time its replicator asks for, at least a millisecond.
    fn set_timer(&mut self, n: u8) {
        let now = self.now;
        let Some(node) = self.nodes.get_mut(&n) else { return };
        let Some(next) = node.replicator.as_ref().and_then(keel_sync::Replicator::next_tick) else {
            return;
        };
        let wait = next
            .duration_since(node.time(now))
            .map_or(0, |wait| i64::try_from(wait.as_micros().div_ceil(1000)).unwrap_or(i64::MAX));
        let due = now.saturating_add(wait.max(1));
        if node.tick_at.is_none_or(|set| due < set) {
            node.tick_at = Some(due);
            let incarnation = node.incarnation;
            self.at(due, Action::Tick { node: n, incarnation });
        }
    }

    /// Device `n` makes a move, unless it is down, restarted since, healed, or waiting for its
    /// own log to settle.
    fn work(&mut self, n: u8, incarnation: u64) -> Result<(), SimError> {
        let now = self.now;
        if self.healed.is_some() {
            return Ok(());
        }
        let next = now.saturating_add(self.rng.between(self.config.pace.0, self.config.pace.1));
        let (settled, waited, rolled_back) = {
            let Some(node) = self.nodes.get(&n) else { return Ok(()) };
            if node.incarnation != incarnation || !node.is_up() {
                return Ok(());
            }
            let settled = node.replicator.as_ref().is_some_and(keel_sync::Replicator::settled);
            let waited = now >= node.started.saturating_add(SETTLE_WAIT);
            (settled, waited, node.rolled_back)
        };
        if settled && let Some(node) = self.nodes.get_mut(&n) {
            node.rolled_back = false;
        }
        if !settled && !waited {
            self.report.held_back = self.report.held_back.saturating_add(1);
            self.at(now.saturating_add(200), Action::Work { node: n, incarnation });
            return Ok(());
        }
        // Whether the device is cut off from the hub, which only the simulator knows.
        let island = self.cut(n, HUB) || !self.nodes.get(&HUB).is_some_and(Node::is_up);
        let station = Station { device: device(n), island };
        let result = {
            let Some(node) = self.nodes.get_mut(&n) else { return Ok(()) };
            let time = node.time(now);
            let (Some(store), Some(ids), focus) =
                (node.store.as_mut(), node.ids.as_mut(), &mut node.focus)
            else {
                return Ok(());
            };
            let held: Vec<Id<Device>> = store
                .version_vector()
                .map_err(|error| SimError::Store(n, text(&error)))?
                .into_keys()
                .collect();
            let mut after = Hlc::ZERO;
            for origin in held {
                let head = store.head(origin).map_err(|error| SimError::Store(n, text(&error)))?;
                after = after.max(head.hlc());
            }
            workload::work(store, ids, &mut self.rng, focus, station, time)
                .map(|made| (made, after))
        };
        match result {
            Ok(((r#move, made), after)) => {
                let counts = self.report.moves.entry(r#move).or_insert([0; 3]);
                match made {
                    Made::Appended(events) => {
                        counts[0] = counts[0].saturating_add(1);
                        if !settled && rolled_back {
                            self.forked.insert(n);
                        }
                        self.appended_by(n, &events, after)?;
                    }
                    Made::Refused => counts[1] = counts[1].saturating_add(1),
                    Made::Nothing => counts[2] = counts[2].saturating_add(1),
                }
            }
            Err(WorkError::Store(StoreError::Interrupted(_))) => {
                self.crash_now(n, true);
                return Ok(());
            }
            Err(WorkError::Store(error)) => return Err(SimError::Store(n, text(&error))),
            Err(WorkError::Other(what)) => return Err(SimError::Setup(what)),
        }
        self.at(next, Action::Work { node: n, incarnation });
        Ok(())
    }

    /// Records the events device `n` appended, and pushes them to its peers.
    fn appended_by(&mut self, n: u8, events: &[SignedEvent], after: Hlc) -> Result<(), SimError> {
        let now = self.now;
        for event in events {
            self.appended.push(Appended { node: n, event: event.clone(), after, at: now });
            let body = event.body();
            self.latest.insert((n, body.origin_seq.get()), body.event_id);
            self.pending
                .insert((body.origin_device, body.origin_seq.get()), (now, BTreeSet::new()));
            self.report.events = self.report.events.saturating_add(1);
        }
        let result = {
            let Some(node) = self.nodes.get_mut(&n) else { return Ok(()) };
            let time = node.time(now);
            let (Some(store), Some(replicator)) = (node.store.as_mut(), node.replicator.as_mut())
            else {
                return Ok(());
            };
            replicator.appended(&mut StoreReplica::new(store, &self.registry), events, time)
        };
        match result {
            Ok(out) => self.send(n, out),
            Err(error) => self.failed(n, &error)?,
        }
        self.watch(n);
        self.set_timer(n);
        self.observe(n);
        Ok(())
    }

    /// A store failed in the middle of a write: a planned crash, or a failure of the run.
    fn failed(&mut self, n: u8, error: &StoreError) -> Result<(), SimError> {
        if matches!(error, StoreError::Interrupted(_)) {
            self.crash_now(n, true);
            Ok(())
        } else {
            Err(SimError::Store(n, text(error)))
        }
    }

    fn crash(&mut self, n: u8, mid_write: bool, down: i64) {
        if self.healed.is_some() || !self.nodes.get(&n).is_some_and(Node::is_up) {
            return;
        }
        if mid_write {
            let points = [Point::Began, Point::Stored, Point::Committing];
            let point = self.rng.pick(&points).copied().unwrap_or(Point::Began);
            if let Some(node) = self.nodes.get_mut(&n) {
                node.plan.arm(point);
                node.down_for = down;
            }
        } else if let Some(node) = self.nodes.get_mut(&n) {
            node.down_for = down;
            self.crash_now(n, false);
        }
    }

    /// Takes node `n` down now, between writes or in the middle of one, and brings it back up
    /// after its downtime.
    fn crash_now(&mut self, n: u8, mid_write: bool) {
        let Some(node) = self.nodes.get_mut(&n) else { return };
        node.plan.disarm();
        node.go_down();
        self.monitor.forget(n);
        let back = self.now.saturating_add(node.down_for.max(1));
        if let Some(count) = self.report.faults.get_mut(usize::from(mid_write)) {
            *count = count.saturating_add(1);
        }
        self.at(back, Action::Restart { node: n });
    }

    fn snapshot(&mut self, n: u8) -> Result<(), SimError> {
        let Some(node) = self.nodes.get_mut(&n) else { return Ok(()) };
        if !node.is_up() || self.healed.is_some() {
            return Ok(());
        }
        node.take_snapshot()?;
        let own =
            node.replicator.as_ref().and_then(|r| r.version_vector().get(&device(n)).copied());
        self.snapshot_heads.insert(n, own.unwrap_or(0));
        Ok(())
    }

    fn rollback(&mut self, n: u8, down: i64) -> Result<(), SimError> {
        if self.healed.is_some() {
            return Ok(());
        }
        let Some(node) = self.nodes.get_mut(&n) else { return Ok(()) };
        if !node.is_up() || !node.has_snapshot() {
            return Ok(());
        }
        node.plan.disarm();
        node.go_down();
        self.monitor.forget(n);
        node.restore_snapshot()?;
        node.rolled_back = true;
        let kept = self.snapshot_heads.get(&n).copied().unwrap_or(0);
        self.rollbacks.push(Rollback { node: n, kept, at: self.now });
        self.report.faults[2] = self.report.faults[2].saturating_add(1);
        self.at(self.now.saturating_add(down), Action::Restart { node: n });
        Ok(())
    }

    fn heal(&mut self) -> Result<(), SimError> {
        self.healed = Some(self.now);
        let down: Vec<u8> = self
            .nodes
            .iter_mut()
            .map(|(n, node)| {
                node.plan.disarm();
                (*n, node.is_up())
            })
            .filter(|(_, up)| !up)
            .map(|(n, _)| n)
            .collect();
        for n in down {
            self.start(n)?;
        }
        Ok(())
    }

    /// Checks node `n`'s replicator after it handled something: its watermark only rose since it
    /// started. Notes how far into a device's own log its events are store-durable.
    fn watch(&mut self, n: u8) {
        let Some(replicator) = self.nodes.get(&n).and_then(|node| node.replicator.as_ref()) else {
            return;
        };
        let watermark = replicator.durable().clone();
        let store_durable = replicator.store_durable();
        if let Some(before) = self.watermarks.get(&n)
            && let Some((origin, position)) = before
                .iter()
                .find(|(origin, position)| watermark.get(*origin).is_none_or(|now| now < *position))
        {
            self.broken
                .get_or_insert(format!("node {n}'s watermark for {origin:?} fell from {position}"));
        }
        self.watermarks.insert(n, watermark);
        let marked = self.durable_marked.get(&n).copied().unwrap_or(0);
        for position in marked.saturating_add(1)..=store_durable {
            if let Some(&event) = self.latest.get(&(n, position)) {
                self.store_durable.insert(event);
            }
        }
        self.durable_marked.insert(n, marked.max(store_durable));
    }

    /// Notes what node `n` now holds, for the lag of events reaching every replica.
    fn observe(&mut self, n: u8) {
        let Some(vector) = self
            .nodes
            .get(&n)
            .and_then(|node| node.replicator.as_ref())
            .map(|r| r.version_vector().clone())
        else {
            return;
        };
        let previous = self.seen.insert(n, vector.clone()).unwrap_or_default();
        let everyone = self.nodes.len();
        let quiet = self.cuts.iter().all(|cut| !(cut.from..cut.to).contains(&self.now))
            && self.nodes.values().all(Node::is_up);
        for (origin, &held) in &vector {
            let before = previous.get(origin).copied().unwrap_or(0);
            let mut position = before;
            while position < held {
                position = position.saturating_add(1);
                let key = (*origin, position);
                let Some((appended, holders)) = self.pending.get_mut(&key) else { continue };
                holders.insert(n);
                if holders.len() == everyone {
                    let lag = self.now.saturating_sub(*appended);
                    if quiet {
                        self.report.max_lag = self.report.max_lag.max(lag);
                    }
                    self.pending.remove(&key);
                }
            }
        }
    }
}
