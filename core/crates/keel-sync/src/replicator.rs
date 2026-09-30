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

use core::time::Duration;
use std::collections::BTreeMap;

use keel_events::envelope::{Device, Location};
use keel_events::event::SignedEvent;
use keel_store::Received;
use keel_types::{Id, Timestamp};

use crate::frame::{Events, Frame, Have, VersionVector};
use crate::replica::Replica;

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
}

impl SyncConfig {
    /// A round every 5 s, batches of up to 256 events and 256 KiB, taken as lost after 2 s.
    pub const DEFAULT: SyncConfig = SyncConfig {
        round: Duration::from_secs(5),
        ack_timeout: Duration::from_secs(2),
        batch_events: 256,
        batch_bytes: 256 * 1024,
    };
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
    location: Id<Location>,
    device: Id<Device>,
    /// What the replica holds, kept in step with the store as events are stored.
    ours: VersionVector,
    peers: BTreeMap<Id<Device>, Peer>,
    /// Whether a peer has shown it holds no more of the device's own log than the replica.
    settled: bool,
    stats: Stats,
}

impl Replicator {
    /// Starts replicating `replica` with `peers`, at time `now`: returns the replicator and the
    /// `have` frames to send them.
    ///
    /// # Errors
    /// If the replica's store can't be read.
    pub fn start<R: Replica>(
        replica: &mut R,
        peers: impl IntoIterator<Item = Id<Device>>,
        config: SyncConfig,
        now: Timestamp,
    ) -> Result<(Replicator, Vec<Outgoing>), R::Error> {
        let ours = replica.version_vector()?;
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
                };
                (peer, state)
            })
            .collect();
        let replicator = Replicator {
            config,
            location: replica.location(),
            device,
            ours,
            peers,
            settled: false,
            stats: Stats::default(),
        };
        let outgoing = replicator.peers.keys().map(|&to| replicator.have(to)).collect();
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
            Ok(Frame::Have(_)) | Err(_) => Ok(self.drop_frame()),
        }
    }

    /// Handles the passing of time: takes a batch unacknowledged for too long as lost, begins a
    /// round with each peer one is due for, telling it what the replica holds and ending a stall,
    /// and sends what peers lack. A round is due a round after the last, or an acknowledgement
    /// timeout after it until the replica has heard from the peer. Call it every so often;
    /// [`Replicator::next_tick`] says when it is next needed.
    ///
    /// # Errors
    /// If the replica's store can't be read.
    pub fn on_tick<R: Replica>(
        &mut self,
        replica: &mut R,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let mut outgoing = Vec::new();
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
            }
            outgoing.extend(self.push(replica, to, now)?);
        }
        Ok(outgoing)
    }

    /// Takes in events the replica's own device appended, once the write storing them has
    /// committed, and returns the batches that send them to peers lacking them.
    ///
    /// # Errors
    /// If the replica's store can't be read.
    pub fn appended<R: Replica>(
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
        self.push_all(replica, now)
    }

    /// Whether the device's own log is settled: a peer has shown it holds no more of that log
    /// than the replica, after sending whatever more it held. After a start, the device mustn't
    /// write before its log is settled, or, if it can reach no peer, before it must (ADR-0019).
    pub const fn settled(&self) -> bool {
        self.settled
    }

    /// What the replica holds, as the replicator knows it.
    pub const fn version_vector(&self) -> &VersionVector {
        &self.ours
    }

    /// What the peer `peer` holds, as far as the replica knows; `None` before its first `have`.
    pub fn known(&self, peer: Id<Device>) -> Option<&VersionVector> {
        self.peers.get(&peer).and_then(|peer| peer.known.as_ref())
    }

    /// What the replicator has done so far.
    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// When [`Replicator::on_tick`] is next needed: the earliest round or acknowledgement
    /// timeout due, or `None` without peers.
    pub fn next_tick(&self) -> Option<Timestamp> {
        self.peers
            .values()
            .flat_map(|peer| {
                let round = peer.round_at.checked_add(peer.interval(&self.config));
                let timeout = peer
                    .waiting
                    .as_ref()
                    .and_then(|waiting| waiting.sent.checked_add(self.config.ack_timeout));
                [round, timeout]
            })
            .flatten()
            .min()
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
            peer.waiting = None;
        }
        peer.known = Some(have.vv);
        self.settle();
        // A peer that has heard nothing from the replica since it started asks what the replica
        // holds, to settle its own log: tell it at once.
        let mut outgoing = if have.asks { vec![self.have(from)] } else { Vec::new() };
        outgoing.extend(self.push(replica, from, now)?);
        Ok(outgoing)
    }

    fn on_events<R: Replica>(
        &mut self,
        replica: &mut R,
        from: Id<Device>,
        batch: &Events,
        now: Timestamp,
    ) -> Result<Vec<Outgoing>, R::Error> {
        let outcomes = replica.receive(&batch.events, now)?;
        let mut stored_any = false;
        let mut in_step = true;
        for outcome in outcomes {
            match outcome {
                Received::Stored(event) => {
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
        if let Some(peer) = self.peers.get_mut(&from) {
            peer.received = batch.batch;
        }
        self.settle();
        let mut outgoing = vec![self.have(from)];
        if stored_any {
            outgoing.extend(self.push_all(replica, now)?);
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

    /// Settles the device's own log once a peer holds no more of it than the replica.
    fn settle(&mut self) {
        if self.settled {
            return;
        }
        let own = self.ours.get(&self.device).copied().unwrap_or(0);
        self.settled = self.peers.values().any(|peer| {
            peer.known
                .as_ref()
                .is_some_and(|known| known.get(&self.device).copied().unwrap_or(0) <= own)
        });
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

/// Whether `span` has passed since `since`, at `now`. A clock set back since counts as having
/// passed it, so that a clock corrected backwards never stops the replicator's rounds.
fn elapsed(since: Timestamp, now: Timestamp, span: Duration) -> bool {
    now.duration_since(since).is_none_or(|passed| passed >= span)
}
