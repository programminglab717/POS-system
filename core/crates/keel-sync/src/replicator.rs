//! The replicator: one replica's side of replication with its peers (ADR-0019).
//!
//! Replication is anti-entropy by version vector. A replica tells each peer what it holds, in a
//! `have` frame, when it starts, after each batch it receives, and every round; a peer that holds
//! more sends it one batch of what it lacks, and waits for the next `have` before sending more.
//! Until a replica has heard from a peer, its `have` asks for the peer's in return, which the
//! peer sends at once, so a replica that starts soon learns what its peers hold. New events are
//! pushed at once to every peer that lacks them. None of this assumes anything of delivery: a
//! lost, duplicated, reordered or delayed frame costs time, never an event, since every `have`
//! says exactly what its sender holds, and receiving an event twice changes nothing.
//!
//! Replicas may have roles (ADR-0020, ADR-0022). A replica that can be the Store Hub has a
//! priority, and the one holding the winning claim is the hub. While it serves, its log settled
//! and not forked, it answers the requests for orders that wait (ADR-0021) and numbers the events
//! in records it appends, as it begins to serve and after every write that stores events, pushing
//! both like any new events. A replica may
//! name a durable peer, the cloud, whose `have` is the durable-ack watermark; replicas relay the
//! watermark to each other in `durable` frames, and keep the most each peer has told them.
//!
//! Every replica sends each peer a heartbeat every heartbeat period: its priority as hub; its
//! term, the epoch and the hub of the winning claim it holds; whether it is acting as that hub;
//! and the hub's beat, its own or the latest it had from the hub directly ([`crate::frame`]). A
//! replica that can be hub claims the next epoch when all of these hold (ADR-0022):
//! - its log is settled, and no peer has refused its own log, which would mean it forked;
//! - it hears no hub ([`Replicator::hub_reachable`]), and has listened for the periods of
//!   silence since it started, and since it learned of the winning claim it holds;
//! - in those periods it has heard no peer that would be preferred as hub;
//! - it holds as much of the log of each device holding a term on its chain as each peer but
//!   that device has said it holds, or has waited the periods of silence more;
//! - its store holds the whole chain of claims, and every record that counts on it.
//!
//! A replica that can't be the hub now, its log unsettled or forked, gives its priority as 0, so
//! that no candidate defers to it; and a hub whose log forked stops serving.
//!
//! A device's own log is settled once every peer that has said what it holds since the start
//! holds no more of that log than the replica, and either all have said so, or a heartbeat period
//! has passed since the first did.

use core::num::NonZeroU8;
use core::time::Duration;
use std::collections::BTreeMap;

use keel_domain::hub::{self, Term};
use keel_events::envelope::{Device, Location};
use keel_events::event::SignedEvent;
use keel_store::Received;
use keel_types::{Id, Timestamp};

use crate::election::{self, Election};
use crate::frame::{Durable, Events, Frame, Have, Heartbeat, VersionVector};
use crate::replica::{Claiming, Replica};

/// How the replicator paces itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncConfig {
    /// How often a replica tells each peer what it holds, whatever else happens.
    pub round: Duration,
    /// How long a batch waits for its acknowledgement before it is taken as lost.
    pub ack_timeout: Duration,
    /// The most events in one batch.
    pub batch_events: u32,
    /// The most bytes of events in one batch, unless its first event alone is more.
    pub batch_bytes: usize,
    /// How often a replica sends each peer a heartbeat: one heartbeat period.
    pub heartbeat: Duration,
    /// How many heartbeat periods without a word from the hub, or from a candidate preferred to
    /// the replica, before it counts as lost, from the start of the next.
    pub silence: u32,
}

impl SyncConfig {
    /// A round every 5 s, batches of up to 256 events and 256 KiB, taken as lost after 2 s, and a
    /// heartbeat every second, the hub lost after 3 silent.
    pub const DEFAULT: SyncConfig = SyncConfig {
        round: Duration::from_secs(5),
        ack_timeout: Duration::from_secs(2),
        batch_events: 256,
        batch_bytes: 256 * 1024,
        heartbeat: Duration::from_secs(1),
        silence: 3,
    };
}

/// What a replica does besides replicating (ADR-0020, ADR-0022).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Roles {
    /// Its priority as the Store Hub, higher preferred; `None` if it can never be the hub.
    pub hub: Option<NonZeroU8>,
    /// Its durable peer, whose `have` is the durable-ack watermark: the cloud, for the hub or a
    /// merchant's only device. `None` for a replica that learns the watermark from its peers.
    pub durable: Option<Id<Device>>,
}

/// A frame to send to a peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The peer.
    pub to: Id<Device>,
    /// The encoded frame.
    pub frame: Vec<u8>,
}

/// What a replicator has done, for tests and monitoring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Frames dropped: undecodable, from a replica that isn't a peer, or for another location.
    pub dropped: u64,
    /// Batches sent.
    pub batches: u64,
    /// Events sent.
    pub sent: u64,
    /// Events received and stored.
    pub stored: u64,
    /// Events received that the replica already held.
    pub duplicates: u64,
    /// Events received after a gap in their device's log, and not stored.
    pub gaps: u64,
    /// Events received and quarantined.
    pub quarantined: u64,
    /// Batches taken as lost, unacknowledged in time.
    pub timeouts: u64,
    /// Batches a peer took none of, after which the replica waited for the next round.
    pub stalls: u64,
    /// Claims of the hub's role the replica made.
    pub claims: u64,
}

/// What a replica knows of one peer.
#[derive(Clone, Debug)]
struct Peer {
    /// What the peer holds, as far as the replica knows: its last `have`, and the events it
    /// sent since. `None` until its first `have`.
    known: Option<VersionVector>,
    /// The batch sent to the peer and not yet acknowledged.
    waiting: Option<Waiting>,
    /// The number of the last batch sent to the peer.
    batches: u64,
    /// The number of the last batch received from the peer, which the next `have` acknowledges.
    received: u64,
    /// When the replica last began a round with the peer, telling it what it holds.
    /// Acknowledgements don't count, so a peer that keeps sending batches can't hold off the
    /// rounds, nor the end of a stall that only a round brings.
    round_at: Timestamp,
    /// The peer took none of the last batch: send it nothing more before the next round.
    stalled: bool,
    /// The peer took none of its own log in the last batch that carried some: it holds another
    /// version of that log, its device having forked it, so its own log no longer goes first,
    /// lest it hold up every other device's.
    own_refused: bool,
    /// The first position of the replica's own log the peer refused: it took none of a batch of
    /// that log that began just after what it held of it. It holds another version of the log,
    /// the replica's device having forked it, unless it later says it holds the log that far.
    refused_ours: Option<u64>,
}

impl Peer {
    /// How long a round with the peer lasts: a round, or, until the replica has heard from it,
    /// an acknowledgement timeout, so that a `have` lost on the way is soon told again.
    const fn interval(&self, config: &SyncConfig) -> Duration {
        if self.known.is_none() { config.ack_timeout } else { config.round }
    }
}

/// A batch to send: its events, their size in bytes, and the first position of each device's
/// events in it.
#[derive(Default)]
struct Batch {
    events: Vec<Vec<u8>>,
    bytes: usize,
    first: VersionVector,
}

/// A batch waiting for its acknowledgement.
#[derive(Clone, Debug)]
struct Waiting {
    batch: u64,
    sent: Timestamp,
    /// The first position of each device's events in the batch.
    first: VersionVector,
}

/// One replica's side of replication with its peers. It does no I/O: hand it each frame
/// received and a tick now and then, and send the frames it returns.
#[derive(Clone, Debug)]
pub struct Replicator {
    config: SyncConfig,
    roles: Roles,
    location: Id<Location>,
    device: Id<Device>,
    /// What the replica holds, kept in step with the store as events are stored.
    ours: VersionVector,
    peers: BTreeMap<Id<Device>, Peer>,
    /// Whether the device's own log is settled: the peers that said what they hold since the
    /// start hold no more of it than the replica.
    settled: bool,
    /// The heartbeat period in which the first peer said what it holds.
    first_have: Option<u64>,
    /// What the replica has heard of the hub and the candidates.
    election: Election,
    /// When the replica last began a heartbeat period.
    heartbeat_at: Timestamp,
    /// The chain of terms the replica's store holds, as last read: the winning term first.
    terms: Vec<Term>,
    /// The beat the replica last gave acting as the hub: 0 if none.
    beat: u64,
    /// The durable-ack watermark: the most of each device's log the durable replica is known to
    /// hold.
    durable: VersionVector,
    stats: Stats,
}

impl Replicator {
    /// Starts replicating `replica` with `peers`, in `roles`, at time `now`: returns the
    /// replicator and the `have` and heartbeat frames to send them.
    ///
    /// # Errors
    /// If the replica's store can't be read.
    pub fn start<R: Replica>(
        replica: &mut R,
        peers: impl IntoIterator<Item = Id<Device>>,
        config: SyncConfig,
        roles: Roles,
        now: Timestamp,
    ) -> Result<(Replicator, Vec<Outgoing>), R::Error> {
        let ours = replica.version_vector()?;
        let terms = replica.terms()?;
        let device = replica.device();
        // Batches are numbered from the time the replicator starts, so a peer's acknowledgement
        // of a batch sent before a restart acknowledges none sent after it.
        let first = u64::try_from(now.as_micros()).unwrap_or(0);
        let peers: BTreeMap<Id<Device>, Peer> = peers
            .into_iter()
            .filter(|peer| *peer != device)
            .map(|peer| {
                let state = Peer {
                    known: None,
                    waiting: None,
                    batches: first,
                    received: 0,
                    round_at: now,
                    stalled: false,
                    own_refused: false,
                    refused_ours: None,
                };
                (peer, state)
            })
            .collect();
        let replicator = Replicator {
            config,
            roles,
            location: replica.location(),
            device,
            ours,
            peers,
            settled: false,
            first_have: None,
            election: Election::default(),
            heartbeat_at: now,
            terms,
            beat: 0,
            durable: VersionVector::new(),
            stats: Stats::default(),
        };
        let outgoing = replicator
            .peers
            .keys()
            .flat_map(|&to| [replicator.have(to), replicator.heartbeat(to)])
            .collect();
        Ok((replicator, outgoing))
    }

    /// Handles a frame received from `from` at time `now`, and returns the frames to send.
    /// A frame that doesn't decode, from a replica that isn't a peer, or for another location,
    /// is dropped and counted.
    ///
    /// # Errors
    /// If the replica's store can't be read or written. Nothing received in the frame was
    /// stored, and the peer will send it again.
    pub fn on_frame<R: Replica>(
        &mut self,
        replica: &mut R,
        from: Id<Device>,
        frame: &[u8],
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let decoded = Frame::decode(frame);
        match decoded {
            Ok(_) if !self.peers.contains_key(&from) => Ok(self.drop_frame()),
            Ok(Frame::Have(have)) if have.location == self.location => {
                self.on_have(replica, from, have, now)
            }
            Ok(Frame::Events(events)) => self.on_events(replica, from, &events, now),
            Ok(Frame::Durable(durable)) if durable.location == self.location => {
                Ok(self.learn_durable(&durable.vv, from))
            }
            Ok(Frame::Heartbeat(heartbeat)) if heartbeat.location == self.location => {
                self.election.hear(from, &heartbeat);
                Ok(Vec::new())
            }
            Ok(Frame::Have(_) | Frame::Durable(_) | Frame::Heartbeat(_)) | Err(_) => {
                Ok(self.drop_frame())
            }
        }
    }

    /// Handles the passing of time: begins a heartbeat period if one is due, sending each peer a
    /// heartbeat and claiming the hub's role if the replica should; takes a batch unacknowledged
    /// for too long as lost; begins a round with each peer one is due for, telling it what the
    /// replica holds and ending a stall; and sends what peers lack. A round is due a round after
    /// the last, or an acknowledgement timeout after it until the replica has heard from the
    /// peer. Call it every so often; [`Replicator::next_tick`] says when it is next needed.
    ///
    /// # Errors
    /// If the replica's store can't be read, or written as it claims or serves as the hub.
    pub fn on_tick<R: Replica>(
        &mut self,
        replica: &mut R,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let mut outgoing = Vec::new();
        if elapsed(self.heartbeat_at, now, self.config.heartbeat) {
            self.heartbeat_at = now;
            self.election.tick(self.epoch());
            let was_serving = self.serving();
            self.settle();
            if self.serving() && !was_serving {
                outgoing.extend(self.sequence(replica, now)?);
            }
            outgoing.extend(self.elect(replica, now)?);
            if self.serving() {
                // The clock's reading, in microseconds, or one past the last beat if that is
                // later: a beat always rises, whatever the clock does.
                let reading = u64::try_from(now.as_micros()).unwrap_or(0);
                self.beat = reading.max(self.beat.saturating_add(1));
            }
            outgoing.extend(self.peers.keys().map(|&to| self.heartbeat(to)));
        }
        let ids: Vec<Id<Device>> = self.peers.keys().copied().collect();
        for to in ids {
            let Some(peer) = self.peers.get_mut(&to) else { continue };
            if peer.waiting.as_ref().is_some_and(|w| elapsed(w.sent, now, self.config.ack_timeout))
            {
                peer.waiting = None;
                self.stats.timeouts = self.stats.timeouts.saturating_add(1);
            }
            if elapsed(peer.round_at, now, peer.interval(&self.config)) {
                peer.round_at = now;
                peer.stalled = false;
                outgoing.push(self.have(to));
                outgoing.extend(self.durable_for(to));
            }
            outgoing.extend(self.push(replica, to, now)?);
        }
        Ok(outgoing)
    }

    /// Takes in events the replica's own device appended, once the write storing them has
    /// committed, and returns the batches that send them to peers lacking them. The Store Hub
    /// sequences them, once its log is settled.
    ///
    /// # Errors
    /// If the replica's store can't be read, or, for the hub, written.
    pub fn appended<R: Replica>(
        &mut self,
        replica: &mut R,
        events: &[SignedEvent],
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let mut outgoing = self.take_in(replica, events, now)?;
        if !events.is_empty() {
            outgoing.extend(self.sequence(replica, now)?);
        }
        Ok(outgoing)
    }

    /// Takes in events the replica's own device appended, and returns the batches that send them
    /// to peers lacking them.
    fn take_in<R: Replica>(
        &mut self,
        replica: &mut R,
        events: &[SignedEvent],
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let mut in_step = true;
        for event in events {
            let body = event.body();
            in_step &= self.note(body.origin_device, body.origin_seq.get());
        }
        if !in_step {
            self.ours = replica.version_vector()?;
        }
        if events.iter().any(is_claim) {
            self.read_terms(replica)?;
        }
        self.push_all(replica, now)
    }

    /// Reads the chain of terms the store holds again, after it stored a claim, noting it if the
    /// winning claim is now another replica's that the replica hadn't held.
    fn read_terms<R: Replica>(&mut self, replica: &mut R) -> Result<(), R::Error> {
        let winning = self.terms.first().map(|term| term.claim);
        self.terms = replica.terms()?;
        if let Some(term) = self.terms.first()
            && Some(term.claim) != winning
            && term.device != self.device
        {
            self.election.learn_claim();
        }
        Ok(())
    }

    /// As the Store Hub, while it serves: answers the requests for orders that wait (ADR-0021),
    /// numbers what no record that counts covers, answers included, and returns the batches that
    /// send the answers and records to peers lacking them.
    fn sequence<R: Replica>(
        &mut self,
        replica: &mut R,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        if !self.serving() {
            return Ok(Vec::new());
        }
        let mut written = replica.answer_requests(now)?;
        written.extend(replica.sequence(now)?);
        if written.is_empty() {
            return Ok(Vec::new());
        }
        self.take_in(replica, &written, now)
    }

    /// Claims the hub's role, at the start of a heartbeat period, if the replica should: see
    /// the module's documentation. Returns the batches that send the claim, and the hub's first
    /// answers and records, to peers lacking them.
    fn elect<R: Replica>(
        &mut self,
        replica: &mut R,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let silence = u64::from(self.config.silence);
        let Some(priority) = self.roles.hub else { return Ok(Vec::new()) };
        let ready = !self.is_hub()
            && self.settled
            && !self.forked()
            && self.election.period() >= silence
            && !self.election.hears_hub(self.heard_term(), silence)
            && !self.election.hears_preferred(self.device, priority.get(), silence);
        if !ready {
            self.election.stop_waiting();
            return Ok(Vec::new());
        }
        if self.caught_up() {
            self.election.stop_waiting();
        } else if !self.election.waited_behind(silence) {
            return Ok(Vec::new());
        }
        match replica.claim(priority, now)? {
            Claiming::Claimed(claim) => {
                self.stats.claims = self.stats.claims.saturating_add(1);
                self.election.stop_waiting();
                let mut outgoing = self.take_in(replica, &[*claim], now)?;
                outgoing.extend(self.sequence(replica, now)?);
                Ok(outgoing)
            }
            Claiming::Hub => {
                self.read_terms(replica)?;
                Ok(Vec::new())
            }
            Claiming::Behind => Ok(Vec::new()),
        }
    }

    /// Whether the replica holds as much of the log of each device holding a term on its chain
    /// as each peer but that device has said it holds. A hub's word of its own log can't help:
    /// what only the hub holds, the replica could have only from the hub, which has fallen silent
    /// or been succeeded, and its successor's cut fences it.
    fn caught_up(&self) -> bool {
        self.terms.iter().all(|term| {
            let held = self.ours.get(&term.device).copied().unwrap_or(0);
            self.peers
                .iter()
                .filter(|(peer, _)| **peer != term.device)
                .filter_map(|(_, peer)| peer.known.as_ref())
                .all(|known| known.get(&term.device).copied().unwrap_or(0) <= held)
        })
    }

    /// Whether a peer refused the device's own log: it holds another version of it, the device
    /// having forked it, perhaps writing after its store was restored from an older copy and
    /// before it could reach a peer holding the rest. Its later events never replicate, so it
    /// mustn't be the hub (ADR-0019, ADR-0022).
    pub fn forked(&self) -> bool {
        self.peers.values().any(|peer| peer.refused_ours.is_some())
    }

    /// The epoch of the winning claim the replica holds: 0 if it holds none.
    fn epoch(&self) -> u64 {
        self.terms.first().map_or(0, |term| term.epoch.get())
    }

    /// The term of the winning claim the replica holds, as heartbeats name it: its epoch and its
    /// hub's device.
    fn heard_term(&self) -> Option<election::Term> {
        self.terms.first().map(|term| (term.epoch.get(), term.device))
    }

    /// The heartbeat for `to`: the replica's priority, 0 if it can't be the hub now, its log
    /// being unsettled or forked; its term; whether it acts as the term's hub; and the hub's
    /// beat, its own as the hub, else the latest it had from the hub directly.
    fn heartbeat(&self, to: Id<Device>) -> Outgoing {
        let silence = u64::from(self.config.silence);
        let eligible = self.settled && !self.forked();
        let priority = if eligible { self.roles.hub.map_or(0, NonZeroU8::get) } else { 0 };
        let term = self.heard_term();
        let acting = self.serving();
        let beat =
            if acting { Some(self.beat) } else { self.election.beat_heard_directly(term, silence) };
        let heartbeat = Heartbeat {
            location: self.location,
            priority,
            epoch: self.epoch(),
            hub: term.map(|(_, hub)| hub),
            acting,
            beat,
        };
        Outgoing { to, frame: Frame::Heartbeat(heartbeat).encode() }
    }

    /// Whether the device's own log is settled: the peers that said what they hold since the
    /// start hold no more of that log than the replica, after sending whatever more they held,
    /// and either all have said so, or a heartbeat period has passed since the first did. After a
    /// start, the device mustn't write before its log is settled, or, if it can reach no peer,
    /// before it must (ADR-0019, ADR-0022).
    pub const fn settled(&self) -> bool {
        self.settled
    }

    /// Whether the replica holds the winning claim, as it last read its store: it is the Store
    /// Hub, and serves as one while its log is settled and not forked (ADR-0022).
    pub fn is_hub(&self) -> bool {
        self.terms.first().is_some_and(|term| term.device == self.device)
    }

    /// Whether the replica serves as the Store Hub: it holds the winning claim, and its log is
    /// settled and not forked.
    pub fn serving(&self) -> bool {
        self.settled && !self.forked() && self.is_hub()
    }

    /// The winning claim's term, as the replica last read its store: the hub's epoch, device and
    /// claim. `None` while it holds no claim.
    pub fn term(&self) -> Option<&Term> {
        self.terms.first()
    }

    /// Whether the replica hears its hub: it serves as the hub; or the latest beat it knows of,
    /// of the hub of the winning claim it holds or of the hub of a later epoch, reached it in the
    /// current heartbeat period or the periods of silence before it, from the hub or from a peer
    /// that had it directly; or it learned of the winning claim as recently. A device that
    /// doesn't is an island, and works independently (ADR-0022).
    pub fn hub_reachable(&self) -> bool {
        let silence = u64::from(self.config.silence);
        self.serving() || self.election.hears_hub(self.heard_term(), silence)
    }

    /// What the replica holds, as the replicator knows it.
    pub const fn version_vector(&self) -> &VersionVector {
        &self.ours
    }

    /// How far into the device's own log a peer has said it holds: the device's events up to
    /// there are store-durable, held by two replicas; after it, "not yet backed up" (ADR-0020).
    pub fn store_durable(&self) -> u64 {
        self.peers
            .values()
            .filter_map(|peer| peer.known.as_ref()?.get(&self.device).copied())
            .max()
            .unwrap_or(0)
    }

    /// The durable-ack watermark: how far into each device's log the location's durable replica
    /// is known to hold. It only rises.
    pub const fn durable(&self) -> &VersionVector {
        &self.durable
    }

    /// What the peer `peer` holds, as far as the replica knows; `None` before its first `have`.
    pub fn known(&self, peer: Id<Device>) -> Option<&VersionVector> {
        self.peers.get(&peer).and_then(|peer| peer.known.as_ref())
    }

    /// What the replicator has done so far.
    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// When [`Replicator::on_tick`] is next needed: the earliest round, acknowledgement timeout
    /// or heartbeat period due, or `None` without peers.
    pub fn next_tick(&self) -> Option<Timestamp> {
        let heartbeat = self.heartbeat_at.checked_add(self.config.heartbeat);
        self.peers
            .values()
            .flat_map(|peer| {
                let round = peer.round_at.checked_add(peer.interval(&self.config));
                let timeout = peer
                    .waiting
                    .as_ref()
                    .and_then(|waiting| waiting.sent.checked_add(self.config.ack_timeout));
                [round, timeout, heartbeat]
            })
            .flatten()
            .min()
    }

    /// Takes in a watermark `from` told: relayed to the other peers if it raised the replica's.
    fn learn_durable(&mut self, vv: &VersionVector, from: Id<Device>) -> Vec<Outgoing> {
        if self.merge_durable(vv) { self.relay_durable(from) } else { Vec::new() }
    }

    /// Raises the watermark to `vv`, device by device: whether it rose.
    fn merge_durable(&mut self, vv: &VersionVector) -> bool {
        let mut rose = false;
        for (&device, &position) in vv {
            let held = self.durable.entry(device).or_insert(0);
            if position > *held {
                *held = position;
                rose = true;
            }
        }
        rose
    }

    /// The watermark, for every peer but `except` and the durable peer, which knows it best.
    fn relay_durable(&self, except: Id<Device>) -> Vec<Outgoing> {
        self.peers
            .keys()
            .filter(|&&to| to != except)
            .filter_map(|&to| self.durable_for(to))
            .collect()
    }

    /// The watermark, for `to`: none if the replica knows none, or `to` is its durable peer.
    fn durable_for(&self, to: Id<Device>) -> Option<Outgoing> {
        if self.durable.is_empty() || self.roles.durable == Some(to) {
            return None;
        }
        let durable = Durable { location: self.location, vv: self.durable.clone() };
        Some(Outgoing { to, frame: Frame::Durable(durable).encode() })
    }

    fn drop_frame(&mut self) -> Vec<Outgoing> {
        self.stats.dropped = self.stats.dropped.saturating_add(1);
        Vec::new()
    }

    fn on_have<R: Replica>(
        &mut self,
        replica: &mut R,
        from: Id<Device>,
        have: Have,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let was_serving = self.serving();
        let Some(peer) = self.peers.get_mut(&from) else { return Ok(Vec::new()) };
        if let Some(waiting) = &peer.waiting
            && waiting.batch == have.acked
        {
            let took_some = waiting
                .first
                .iter()
                .any(|(device, first)| have.vv.get(device).is_some_and(|held| held >= first));
            if !took_some {
                // Every event the peer refused, or the first of each device it did: wait
                // for the next round rather than send them all again at once.
                peer.stalled = true;
                self.stats.stalls = self.stats.stalls.saturating_add(1);
            }
            if let Some(first) = waiting.first.get(&from) {
                peer.own_refused = have.vv.get(&from).is_none_or(|held| held < first);
            }
            if let Some(&first) = waiting.first.get(&self.device)
                && have.vv.get(&self.device).copied().unwrap_or(0).checked_add(1) == Some(first)
            {
                peer.refused_ours = Some(first);
            }
            peer.waiting = None;
        }
        let held = have.vv.get(&self.device).copied().unwrap_or(0);
        if peer.refused_ours.is_some_and(|refused| held >= refused) {
            peer.refused_ours = None;
        }
        self.first_have.get_or_insert(self.election.period());
        let mut outgoing = Vec::new();
        // The durable peer's `have` is the watermark.
        if self.roles.durable == Some(from) && self.merge_durable(&have.vv) {
            outgoing.extend(self.relay_durable(from));
        }
        if let Some(peer) = self.peers.get_mut(&from) {
            peer.known = Some(have.vv);
        }
        self.settle();
        // A peer that has heard nothing from the replica since it started asks what the replica
        // holds, to settle its own log: tell it at once, and the watermark with it.
        if have.asks {
            outgoing.push(self.have(from));
            outgoing.extend(self.durable_for(from));
        }
        outgoing.extend(self.push(replica, from, now)?);
        if self.serving() && !was_serving {
            outgoing.extend(self.sequence(replica, now)?);
        }
        Ok(outgoing)
    }

    fn on_events<R: Replica>(
        &mut self,
        replica: &mut R,
        from: Id<Device>,
        batch: &Events,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let was_serving = self.serving();
        let outcomes = replica.receive(&batch.events, now)?;
        let mut stored_any = false;
        let mut claims = false;
        let mut in_step = true;
        for outcome in outcomes {
            match outcome {
                Received::Stored(event) => {
                    claims |= is_claim(&event);
                    let body = event.body();
                    let (device, position) = (body.origin_device, body.origin_seq.get());
                    in_step &= self.note(device, position);
                    // The peer holds it, and every event of its device before it: never send
                    // them back.
                    if let Some(known) = self.peers.get_mut(&from).and_then(|p| p.known.as_mut()) {
                        let held = known.entry(device).or_insert(0);
                        *held = (*held).max(position);
                    }
                    stored_any = true;
                    self.stats.stored = self.stats.stored.saturating_add(1);
                }
                Received::Duplicate => {
                    self.stats.duplicates = self.stats.duplicates.saturating_add(1);
                }
                Received::Gap { .. } => self.stats.gaps = self.stats.gaps.saturating_add(1),
                Received::Quarantined(_) => {
                    self.stats.quarantined = self.stats.quarantined.saturating_add(1);
                }
            }
        }
        if !in_step {
            self.ours = replica.version_vector()?;
        }
        if claims {
            self.read_terms(replica)?;
        }
        if let Some(peer) = self.peers.get_mut(&from) {
            peer.received = batch.batch;
        }
        self.settle();
        let mut outgoing = vec![self.have(from)];
        if stored_any {
            outgoing.extend(self.push_all(replica, now)?);
        }
        if stored_any || (self.serving() && !was_serving) {
            outgoing.extend(self.sequence(replica, now)?);
        }
        Ok(outgoing)
    }

    /// Notes that the replica now holds `device`'s log up to `position`. False if that doesn't
    /// follow what the replicator knew, which means it has fallen out of step with the store.
    fn note(&mut self, device: Id<Device>, position: u64) -> bool {
        let held = self.ours.get(&device).copied().unwrap_or(0);
        if held.checked_add(1) == Some(position) {
            self.ours.insert(device, position);
            true
        } else {
            position <= held
        }
    }

    /// Settles the device's own log once every peer that has said what it holds holds no more
    /// of it than the replica, and either every peer has said, or a heartbeat period has passed
    /// since the first did.
    fn settle(&mut self) {
        if self.settled {
            return;
        }
        let own = self.ours.get(&self.device).copied().unwrap_or(0);
        let mut heard = self.peers.values().filter_map(|peer| peer.known.as_ref()).peekable();
        if heard.peek().is_none() {
            return;
        }
        let mut all = true;
        for peer in self.peers.values() {
            match &peer.known {
                Some(known) if known.get(&self.device).copied().unwrap_or(0) > own => return,
                Some(_) => {}
                None => all = false,
            }
        }
        let waited = self.first_have.is_some_and(|first| self.election.period() > first);
        self.settled = all || waited;
    }

    /// The `have` frame for `to`, acknowledging the last batch received from it, and asking for
    /// its `have` in return until the replica has heard from it.
    fn have(&self, to: Id<Device>) -> Outgoing {
        let peer = self.peers.get(&to);
        let acked = peer.map_or(0, |peer| peer.received);
        let asks = peer.is_some_and(|peer| peer.known.is_none());
        let have = Have { location: self.location, vv: self.ours.clone(), acked, asks };
        Outgoing { to, frame: Frame::Have(have).encode() }
    }

    fn push_all<R: Replica>(
        &mut self,
        replica: &mut R,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let mut outgoing = Vec::new();
        let ids: Vec<Id<Device>> = self.peers.keys().copied().collect();
        for to in ids {
            outgoing.extend(self.push(replica, to, now)?);
        }
        Ok(outgoing)
    }

    /// The next batch for `to`, if it lacks events the replica holds, no batch is waiting for
    /// its acknowledgement, and it hasn't stalled.
    fn push<R: Replica>(
        &mut self,
        replica: &mut R,
        to: Id<Device>,
        now: Timestamp,
    ) -> Result<Option<Outgoing>, R::Error> {
        let Some(peer) = self.peers.get(&to) else { return Ok(None) };
        if peer.waiting.is_some() || peer.stalled {
            return Ok(None);
        }
        let Some(known) = peer.known.clone() else { return Ok(None) };
        let own_first = !peer.own_refused;
        let Some(Batch { events, first, .. }) = self.batch_for(replica, to, own_first, &known)?
        else {
            return Ok(None);
        };
        let Some(peer) = self.peers.get_mut(&to) else { return Ok(None) };
        peer.batches = peer.batches.wrapping_add(1);
        let batch = peer.batches;
        peer.waiting = Some(Waiting { batch, sent: now, first });
        self.stats.batches = self.stats.batches.saturating_add(1);
        let count = u64::try_from(events.len()).unwrap_or(u64::MAX);
        self.stats.sent = self.stats.sent.saturating_add(count);
        Ok(Some(Outgoing { to, frame: Frame::Events(Events { batch, events }).encode() }))
    }

    /// A batch of what the peer `to`, holding `known`, lacks; `None` if it lacks nothing. With
    /// `own_first`, the peer's own log comes first, as much of it as fits, since its device
    /// mustn't write until it has it back (ADR-0019); then each other device lagging gets an
    /// equal share of the room left. Each device's events run in order from the position after
    /// the peer's.
    fn batch_for<R: Replica>(
        &self,
        replica: &mut R,
        to: Id<Device>,
        own_first: bool,
        known: &VersionVector,
    ) -> Result<Option<Batch>, R::Error> {
        let (own, others): (Vec<Lag>, Vec<Lag>) = self
            .ours
            .iter()
            .filter_map(|(&device, &held)| {
                let theirs = known.get(&device).copied().unwrap_or(0);
                (held > theirs).then_some(Lag { device, theirs, held })
            })
            .partition(|lag| own_first && lag.device == to);
        let mut batch = Batch::default();
        let mut open = true;
        for lag in own {
            open = self.fill(replica, &mut batch, lag, self.config.batch_events)?;
        }
        if open {
            let taken = u32::try_from(batch.events.len()).unwrap_or(u32::MAX);
            let room = self.config.batch_events.saturating_sub(taken);
            let count = u32::try_from(others.len()).unwrap_or(u32::MAX).max(1);
            let share = room.div_ceil(count).max(1);
            for lag in others {
                if !self.fill(replica, &mut batch, lag, share)? {
                    break;
                }
            }
        }
        Ok((!batch.events.is_empty()).then_some(batch))
    }

    /// Adds up to `limit` of the events the peer lacks of `lag`'s device to `batch`, as room and
    /// the byte budget allow; the budget always admits a batch's first event. False once the
    /// batch is full.
    fn fill<R: Replica>(
        &self,
        replica: &mut R,
        batch: &mut Batch,
        lag: Lag,
        limit: u32,
    ) -> Result<bool, R::Error> {
        let taken = u32::try_from(batch.events.len()).unwrap_or(u32::MAX);
        let room = self.config.batch_events.saturating_sub(taken);
        let lagging = u32::try_from(lag.held.saturating_sub(lag.theirs)).unwrap_or(u32::MAX);
        let limit = limit.min(room).min(lagging);
        if limit == 0 {
            return Ok(false);
        }
        for event in replica.events_after(lag.device, lag.theirs, limit)? {
            let size = event.len();
            if !batch.events.is_empty()
                && batch.bytes.saturating_add(size) > self.config.batch_bytes
            {
                return Ok(false);
            }
            batch.first.entry(lag.device).or_insert_with(|| lag.theirs.saturating_add(1));
            batch.bytes = batch.bytes.saturating_add(size);
            batch.events.push(event);
        }
        Ok(true)
    }
}

/// A device whose log a peer holds less of than the replica: up to `theirs`, where the replica
/// holds up to `held`.
#[derive(Clone, Copy)]
struct Lag {
    device: Id<Device>,
    theirs: u64,
    held: u64,
}

/// Whether `event` is a claim of the hub's role: its term may change the chain.
fn is_claim(event: &SignedEvent) -> bool {
    event.body().stream.kind.as_str() == hub::STREAM
}

/// Whether `span` has passed since `since`, at `now`. A clock set back since counts as having
/// passed it, so that a clock corrected backwards never stops the replicator's rounds.
fn elapsed(since: Timestamp, now: Timestamp, span: Duration) -> bool {
    now.duration_since(since).is_none_or(|passed| passed >= span)
}
