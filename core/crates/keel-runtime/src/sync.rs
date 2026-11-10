//! Replication in the runtime (ADR-0026, decision 2): the device's store replicated with the
//! location's other devices through `keel-sync`'s replicator, sans I/O. The shell, through
//! `keel-net`, hands the runtime each frame a peer sends and a tick when it asks, and sends the
//! frames the runtime gives back; the runtime's thread is the store's one writer throughout.

use keel_domain::roster::Roster;
use keel_events::envelope::Device;
use keel_events::keys::Signer;
use keel_events::verify::DeviceRegistry;
use keel_store::StoreError;
use keel_sync::{Outgoing, Replicator, Roles, StoreReplica, SyncConfig};
use keel_types::{Clock, Entropy, Id, Timestamp};

use crate::error::RuntimeError;
use crate::runtime::Runtime;

/// The most of the device's own events pushed to the replicator at once.
const PUSH: u32 = 1_000;

/// How long after replication starts a device waits for its log to settle before it writes as
/// an island, which a device that can reach no peer must: ADR-0019's "up to a few seconds", as
/// the simulator's devices wait.
pub const ISLAND_AFTER: core::time::Duration = core::time::Duration::from_secs(3);

/// The longest the replicator asks to wait for a tick: a round, 5 s. A tick asked for later than
/// this was asked for by a clock since set back, and is due now.
const LONGEST_WAIT: core::time::Duration = core::time::Duration::from_secs(10);

/// A runtime's replication: its replicator, the location's devices and their keys, and how far
/// into its own log it has told the replicator of.
pub(crate) struct Sync {
    replicator: Replicator,
    registry: DeviceRegistry,
    pub(crate) roster: Roster,
    pushed: u64,
    /// When replication started, by the runtime's clock.
    started: Timestamp,
    /// How far into the cloud's log the runtime has looked for publications.
    pub(crate) looked: u64,
}

impl Sync {
    /// Whether the device may write at `now`: its log is settled, or it has waited
    /// [`ISLAND_AFTER`] for that and writes as an island. A clock set back since replication
    /// started counts as having waited.
    pub(crate) fn may_write(&self, now: Timestamp) -> bool {
        let waited = now.as_millis().saturating_sub(self.started.as_millis());
        let island = i64::try_from(ISLAND_AFTER.as_millis()).unwrap_or(i64::MAX);
        self.replicator.settled() || waited >= island || waited < 0
    }
}

/// What a register shows of its replication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "four independent facts a screen shows")]
pub struct SyncStatus {
    /// The device's own log is settled: until it is, intents are refused.
    pub settled: bool,
    /// A hub is reachable: when it isn't, the device is an island.
    pub hub_reachable: bool,
    /// This device holds the hub's role.
    pub is_hub: bool,
    /// This device serves as the hub now.
    pub serving: bool,
    /// How far into this device's own log the location's cloud is known to hold, its durable-ack
    /// watermark (ADR-0020): 0 if it holds none of it, or the location has no cloud.
    pub backed_up: u64,
    /// How far this device's own log goes: what is past [`SyncStatus::backed_up`] isn't backed up
    /// yet.
    pub head: u64,
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Starts replicating the store with the devices of `roster`, the location's: the frames to
    /// send them. Until the device's own log is settled, intents are refused.
    ///
    /// # Errors
    /// [`RuntimeError::RosterMismatch`] if the roster is another location's, or doesn't have this
    /// device, or names another key for it; [`RuntimeError::Store`] if the store can't be read.
    pub fn start_sync(&mut self, roster: Roster) -> Result<Vec<Outgoing>, RuntimeError> {
        self.start_sync_as(roster, true)
    }

    /// As [`Runtime::start_sync`], but a device that may not host the hub, whatever the roster
    /// says, when `may_host` is false: one the location's devices can't reach, as they dial the
    /// roster's one address and it isn't listening there. Found in review: a standby that didn't
    /// listen took the role when the hub fell silent, and every register it held the term for
    /// became an island.
    ///
    /// # Errors
    /// As [`Runtime::start_sync`].
    pub fn start_sync_as(
        &mut self,
        roster: Roster,
        may_host: bool,
    ) -> Result<Vec<Outgoing>, RuntimeError> {
        let device = self.device;
        let me = roster.device(device).ok_or(RuntimeError::RosterMismatch)?;
        if roster.data().location != self.profile.data().location || me.key != self.key {
            return Err(RuntimeError::RosterMismatch);
        }
        let hub = if may_host { me.hub } else { None };
        // A device that may host the hub names the location's cloud its durable peer, as the
        // simulator's hub and standby do (ADR-0020, ADR-0028); the others learn the watermark
        // from the hub. The cloud itself names none: it is the durable copy.
        let durable = roster
            .cloud()
            .map(|cloud| cloud.device)
            .filter(|cloud| hub.is_some() && *cloud != device);
        let roles = Roles { hub, durable };
        let peers: Vec<Id<Device>> =
            roster.data().devices.iter().map(|d| d.id).filter(|d| *d != device).collect();
        let registry = roster.registry();
        let now = self.clock.now();
        // 64 random bits for the replicator's batches: an identifier's last eight bytes, most of
        // them random.
        let nonce_id: Id<Sync> = self.ids.generate(now)?;
        let tail: [u8; 8] =
            nonce_id.to_bytes().get(8..).and_then(|tail| tail.try_into().ok()).unwrap_or_default();
        let nonce = u64::from_be_bytes(tail);
        let (replicator, outgoing) = Replicator::start(
            &mut StoreReplica::new(&mut self.store, &registry),
            peers,
            SyncConfig::DEFAULT,
            roles,
            nonce,
            now,
        )?;
        let pushed = self.store.head(device)?.seq();
        self.sync = Some(Sync { replicator, registry, roster, pushed, started: now, looked: 0 });
        // The latest publication the store holds, which a device restarted took before.
        self.take_publications()?;
        Ok(outgoing)
    }

    /// Takes in a frame `from` a peer: the frames to send in answer, with those that push what
    /// this device has written since it last did.
    ///
    /// # Errors
    /// [`RuntimeError::NotSyncing`] before [`Runtime::start_sync`]; [`RuntimeError::Store`] if the
    /// store can't be read or written, in which case nothing the frame held was stored, and the
    /// peer sends it again.
    pub fn on_frame(
        &mut self,
        from: Id<Device>,
        frame: &[u8],
    ) -> Result<Vec<Outgoing>, RuntimeError> {
        let mut outgoing = self.take_frames()?;
        let now = self.clock.now();
        let more =
            self.with_sync(|replicator, replica| replicator.on_frame(replica, from, frame, now))?;
        outgoing.extend(more);
        self.caught_up()?;
        // Between two intents: a publication that came is taken whole, or not at all. A store
        // that can't be read now leaves it for the next frame, which looks again from where this
        // look began; the frame itself is stored, and its answers go out.
        let _ = self.take_publications();
        Ok(outgoing)
    }

    /// The replicator's tick, when [`Runtime::next_tick`] asks for one: heartbeats, rounds and
    /// the election, and the frames they send.
    ///
    /// # Errors
    /// As [`Runtime::on_frame`].
    pub fn on_tick(&mut self) -> Result<Vec<Outgoing>, RuntimeError> {
        let mut outgoing = self.take_frames()?;
        let now = self.clock.now();
        outgoing.extend(self.with_sync(|replicator, replica| replicator.on_tick(replica, now))?);
        self.caught_up()?;
        Ok(outgoing)
    }

    /// When the replicator next needs a tick, if it is replicating.
    pub fn next_tick(&self) -> Option<Timestamp> {
        self.sync.as_ref().and_then(|sync| sync.replicator.next_tick())
    }

    /// Whether the replicator needs a tick now: its next tick has come, or lies further ahead
    /// than any wait it asks for, which means the clock was set back since it asked. Found in
    /// review: waiting for such a tick stopped heartbeats for as long as the clock went back.
    pub fn tick_due(&self) -> bool {
        let now = self.clock.now();
        let longest = i64::try_from(LONGEST_WAIT.as_millis()).unwrap_or(i64::MAX);
        self.next_tick()
            .is_some_and(|at| at <= now || at.as_millis().saturating_sub(now.as_millis()) > longest)
    }

    /// The frames that push to peers what this device has written since it last did: the shell
    /// takes them after each intent, so that a sale reaches the hub at once.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read, or, for the hub, written.
    pub fn take_frames(&mut self) -> Result<Vec<Outgoing>, RuntimeError> {
        let device = self.device;
        let mut outgoing = Vec::new();
        loop {
            let Some(pushed) = self.sync.as_ref().map(|sync| sync.pushed) else {
                return Ok(outgoing);
            };
            let events = self.store.log(device, pushed, PUSH)?;
            let Some(last) = events.last().map(|event| event.body().origin_seq.get()) else {
                return Ok(outgoing);
            };
            let now = self.clock.now();
            outgoing.extend(
                self.with_sync(|replicator, replica| replicator.appended(replica, &events, now))?,
            );
            if events.len() < usize::try_from(PUSH).unwrap_or(usize::MAX) {
                // The last of it: everything is pushed, and the hub's records the replicator
                // wrote after it, which it knows of. Found in review: they were handed to it
                // again.
                self.caught_up()?;
            } else if let Some(sync) = self.sync.as_mut() {
                sync.pushed = last;
            }
        }
    }

    /// Marks everything in the device's own log as known to the replicator: what it wrote there
    /// itself, as the hub's records.
    fn caught_up(&mut self) -> Result<(), RuntimeError> {
        let head = self.store.head(self.device)?.seq();
        if let Some(sync) = self.sync.as_mut() {
            sync.pushed = head;
        }
        Ok(())
    }

    /// What the register shows of its replication, if it is replicating.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn sync_status(&self) -> Result<Option<SyncStatus>, RuntimeError> {
        let Some(sync) = self.sync.as_ref() else { return Ok(None) };
        Ok(Some(SyncStatus {
            settled: sync.replicator.settled(),
            hub_reachable: sync.replicator.hub_reachable(),
            is_hub: sync.replicator.is_hub(),
            serving: sync.replicator.serving(),
            backed_up: sync.replicator.durable().get(&self.device).copied().unwrap_or(0),
            head: self.store.head(self.device)?.seq(),
        }))
    }

    /// The roster the runtime replicates with, if it does.
    pub fn roster(&self) -> Option<&Roster> {
        self.sync.as_ref().map(|sync| &sync.roster)
    }

    /// Runs `call` on the replicator, over the store.
    fn with_sync<T>(
        &mut self,
        call: impl FnOnce(&mut Replicator, &mut StoreReplica<'_, S, E>) -> Result<T, StoreError>,
    ) -> Result<T, RuntimeError> {
        let sync = self.sync.as_mut().ok_or(RuntimeError::NotSyncing)?;
        let mut replica = StoreReplica::new(&mut self.store, &sync.registry);
        Ok(call(&mut sync.replicator, &mut replica)?)
    }
}
