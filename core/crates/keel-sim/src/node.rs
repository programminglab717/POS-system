//! A node: a device, the hub or the cloud, with its store on a RAM disk, its replicator and its
//! clock. A node goes down and comes back up; while it is down, its store is closed and its
//! replicator forgotten, as in a process that crashed.

use core::time::Duration;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use keel_events::envelope::{Device, Location};
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_events::verify::DeviceRegistry;
use keel_store::{Faults, Point, Store, StoreConfig, StoreError, StoreKey};
use keel_sync::{Outgoing, Replicator, StoreReplica, SyncConfig, VersionVector};
use keel_types::{Id, IdGenerator, SeededEntropy, Timestamp};

/// The hub.
pub(crate) const HUB: u8 = 10;
/// The cloud replica.
pub(crate) const CLOUD: u8 = 20;
/// The fresh store the replicas' projections are compared with at the end.
pub(crate) const ORACLE: u8 = 30;

/// Milliseconds since the Unix epoch at the simulation's time 0: 2026-09-30T10:00:00Z.
const BASE: i64 = 1_790_762_400_000;

/// How far ahead of physical time a remote HLC may move a node's clock.
pub(crate) const DRIFT: Duration = Duration::from_secs(60);

/// Identifier `n`, for the simulation's fixed things: nodes, the location, a menu item.
#[allow(clippy::expect_used, reason = "the bytes are always a version 7 UUID with its variant")]
pub(crate) fn id<T>(n: u64) -> Id<T> {
    // A fixed time, version 7, the RFC 9562 variant, and the low six bytes of `n`.
    let mut bytes = [0x01, 0x92, 0xf0, 0xc1, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 0];
    for (byte, value) in bytes.iter_mut().rev().zip(n.to_be_bytes().into_iter().rev()).take(6) {
        *byte = value;
    }
    Id::from_bytes(bytes).expect("a version 7 UUID")
}

/// The location every node keeps events for.
pub(crate) fn here() -> Id<Location> {
    id(0x1000)
}

/// Node `n`'s device.
pub(crate) fn device(n: u8) -> Id<Device> {
    id(u64::from(n))
}

/// Node `n`'s signing key: its secret is `n` repeated.
pub(crate) fn signer(n: u8) -> Result<SoftwareSigner, SimError> {
    SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[n; 32])
        .map_err(|error| SimError::Setup(format!("the signer of node {n}: {error}")))
}

/// Every node, enrolled here, the oracle included.
pub(crate) fn registry(nodes: &[u8]) -> Result<DeviceRegistry, SimError> {
    let mut registry = DeviceRegistry::new();
    for &n in nodes.iter().chain([ORACLE].iter()) {
        registry
            .enroll(device(n), here(), signer(n)?.public_key().clone())
            .map_err(|error| SimError::Setup(format!("enrolling node {n}: {error}")))?;
    }
    Ok(registry)
}

/// The physical time `ms` virtual milliseconds into the run, on a clock `offset` ms off.
pub(crate) fn time(ms: i64, offset: i64) -> Timestamp {
    let millis = BASE.saturating_add(ms).saturating_add(offset);
    Timestamp::from_millis(millis).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Why a run stopped before its end: a rule broke as it went, or the simulator couldn't go on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimError {
    /// A rule broke as the run went: the protocol's, or the replicas' deadline for agreeing.
    Broken(String),
    /// Setting up a node failed.
    Setup(String),
    /// A store failed in a way no fault explains.
    Store(u8, String),
    /// Copying a store's files failed.
    Files(String),
}

impl core::fmt::Display for SimError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SimError::Broken(what) => write!(f, "{what}"),
            SimError::Setup(what) => write!(f, "setting up: {what}"),
            SimError::Store(n, what) => write!(f, "node {n}'s store: {what}"),
            SimError::Files(what) => write!(f, "copying a store: {what}"),
        }
    }
}

/// A node's store.
pub(crate) type SimStore = Store<SoftwareSigner, SeededEntropy>;

/// Refuses at the point it is armed for, once: a crash in the middle of a write.
#[derive(Clone, Debug, Default)]
pub(crate) struct Plan(Arc<Mutex<Option<Point>>>);

impl Plan {
    pub(crate) fn arm(&self, point: Point) {
        if let Ok(mut planned) = self.0.lock() {
            *planned = Some(point);
        }
    }

    pub(crate) fn disarm(&self) {
        if let Ok(mut planned) = self.0.lock() {
            *planned = None;
        }
    }
}

impl Faults for Plan {
    fn proceed(&mut self, point: Point) -> bool {
        let Ok(mut planned) = self.0.lock() else { return true };
        if *planned == Some(point) {
            *planned = None;
            return false;
        }
        true
    }
}

/// A node.
#[derive(Debug)]
pub(crate) struct Node {
    pub(crate) n: u8,
    pub(crate) peers: Vec<u8>,
    dir: PathBuf,
    pub(crate) store: Option<SimStore>,
    pub(crate) replicator: Option<Replicator>,
    /// The identifiers its workload gives new orders, lines, checks and payments.
    pub(crate) ids: Option<IdGenerator<SeededEntropy>>,
    /// The sale it is working on, if it is a device.
    pub(crate) focus: Option<Id<keel_domain::order::Order>>,
    pub(crate) plan: Plan,
    /// How far its clock is from virtual time, in milliseconds.
    pub(crate) offset: i64,
    /// How many times it has started.
    pub(crate) incarnation: u64,
    /// When it last started, in virtual milliseconds.
    pub(crate) started: i64,
    /// Whether its store was restored from an older copy when it last started.
    pub(crate) rolled_back: bool,
    /// How long to stay down after an armed crash in the middle of a write.
    pub(crate) down_for: i64,
    /// What it held when it last went down.
    pub(crate) held: VersionVector,
    /// When its replicator's next tick is set for, in virtual milliseconds.
    pub(crate) tick_at: Option<i64>,
    snapshot: Option<PathBuf>,
    seed: u64,
}

impl Node {
    pub(crate) fn new(n: u8, peers: Vec<u8>, base: &Path, seed: u64) -> Result<Node, SimError> {
        let dir = base.join(format!("node-{n}"));
        std::fs::create_dir_all(&dir).map_err(|error| SimError::Files(error.to_string()))?;
        Ok(Node {
            n,
            peers,
            dir,
            store: None,
            replicator: None,
            ids: None,
            focus: None,
            plan: Plan::default(),
            offset: 0,
            incarnation: 0,
            started: 0,
            rolled_back: false,
            down_for: 0,
            held: VersionVector::new(),
            tick_at: None,
            snapshot: None,
            seed,
        })
    }

    pub(crate) fn db(&self) -> PathBuf {
        self.dir.join("store.db")
    }

    pub(crate) const fn is_up(&self) -> bool {
        self.store.is_some()
    }

    /// Its clock's time, `ms` virtual milliseconds into the run.
    pub(crate) fn time(&self, ms: i64) -> Timestamp {
        time(ms, self.offset)
    }

    /// Opens its store and starts replicating, at virtual time `ms`: returns the frames that
    /// tell its peers what it holds.
    pub(crate) fn start(
        &mut self,
        ms: i64,
        registry: &DeviceRegistry,
        config: SyncConfig,
    ) -> Result<Vec<Outgoing>, SimError> {
        self.incarnation = self.incarnation.saturating_add(1);
        let entropy = |salt: u64| {
            let n = u64::from(self.n);
            SeededEntropy::new(self.seed ^ (n << 48) ^ (self.incarnation << 32) ^ salt)
        };
        let store_config =
            StoreConfig { device: device(self.n), location: here(), max_forward_drift: DRIFT };
        let key = StoreKey::new(&mut [self.n; 32]);
        let mut store = Store::open_with_faults(
            self.db(),
            key,
            store_config,
            signer(self.n)?,
            entropy(1),
            Box::new(self.plan.clone()),
        )
        .map_err(|error| SimError::Store(self.n, format!("opening: {error}")))?;
        let peers = self.peers.iter().map(|&peer| device(peer));
        let now = self.time(ms);
        let (replicator, out) =
            Replicator::start(&mut StoreReplica::new(&mut store, registry), peers, config, now)
                .map_err(|error| SimError::Store(self.n, format!("starting: {error}")))?;
        self.store = Some(store);
        self.replicator = Some(replicator);
        self.ids = Some(IdGenerator::new(entropy(2)));
        self.started = ms;
        self.tick_at = None;
        Ok(out)
    }

    /// Goes down, as a crashed process: its store closes and its replicator is forgotten.
    pub(crate) fn go_down(&mut self) {
        if let Some(replicator) = &self.replicator {
            self.held = replicator.version_vector().clone();
        }
        self.store = None;
        self.replicator = None;
        self.ids = None;
        self.focus = None;
    }

    /// Copies its store's files, as they are between writes: what a crash would leave.
    pub(crate) fn take_snapshot(&mut self) -> Result<(), SimError> {
        let copy = self.dir.join("snapshot");
        let _ = std::fs::remove_dir_all(&copy);
        std::fs::create_dir_all(&copy).map_err(|error| SimError::Files(error.to_string()))?;
        for suffix in ["", "-wal"] {
            let from = with_suffix(&self.db(), suffix);
            if from.exists() {
                std::fs::copy(&from, with_suffix(&copy.join("store.db"), suffix))
                    .map_err(|error| SimError::Files(error.to_string()))?;
            }
        }
        self.snapshot = Some(copy);
        Ok(())
    }

    /// Restores its store from its snapshot, while it is down; false if it has none.
    pub(crate) fn restore_snapshot(&mut self) -> Result<bool, SimError> {
        let Some(copy) = self.snapshot.take() else { return Ok(false) };
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(with_suffix(&self.db(), suffix));
        }
        for suffix in ["", "-wal"] {
            let from = with_suffix(&copy.join("store.db"), suffix);
            if from.exists() {
                std::fs::copy(&from, with_suffix(&self.db(), suffix))
                    .map_err(|error| SimError::Files(error.to_string()))?;
            }
        }
        let _ = std::fs::remove_dir_all(&copy);
        Ok(true)
    }

    pub(crate) const fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Its error as text, for a report.
pub(crate) fn text(error: &StoreError) -> String {
    format!("{error:?}")
}
