//! The replication protocol's rules for batches (ADR-0019), checked frame by frame as a run goes,
//! against a model of what each replicator knows of each peer:
//!
//! - a replica sends a peer a batch only when none it sent the peer waits for acknowledgement,
//!   unless that one's acknowledgement timeout has passed;
//! - a replica whose last batch a peer took none of sends the peer nothing more until its next
//!   round, and a tick a round or more after the stall began is that round.
//!
//! The model works in each node's own clock, as its replicator does, so clock jumps are no
//! excuse, and forgets what a node knew when the node goes down, as its replicator does.

use std::collections::BTreeMap;

use keel_events::event::SignedEvent;
use keel_sync::{Frame, SyncConfig, VersionVector};
use keel_types::Timestamp;

/// A batch sent and not yet acknowledged: its number, when it was sent, and the first position
/// of each device's events in it.
#[derive(Clone, Debug)]
struct Waiting {
    batch: u64,
    sent: Timestamp,
    first: VersionVector,
}

/// What a replica knows of one peer, as far as batches go.
#[derive(Clone, Debug, Default)]
struct Link {
    waiting: Option<Waiting>,
    /// When, on the replica's clock, the peer took none of its last batch, until its next round.
    stalled: Option<Timestamp>,
}

/// The model of every replicator's links, keyed by (replica, peer).
#[derive(Clone, Debug)]
pub(crate) struct Monitor {
    config: SyncConfig,
    links: BTreeMap<(u8, u8), Link>,
}

/// Whether `span` has passed since `since` at `now`, a clock set back counting as having passed
/// it, as the replicator counts.
fn elapsed(since: Timestamp, now: Timestamp, span: core::time::Duration) -> bool {
    now.duration_since(since).is_none_or(|passed| passed >= span)
}

impl Monitor {
    pub(crate) const fn new(config: SyncConfig) -> Monitor {
        Monitor { config, links: BTreeMap::new() }
    }

    /// Node `n` went down or started: its replicator knows nothing of its peers.
    pub(crate) fn forget(&mut self, n: u8) {
        self.links.retain(|(replica, _), _| *replica != n);
    }

    /// Node `n` is about to tick at `clock`: batches waiting past their timeout are lost.
    pub(crate) fn ticking(&mut self, n: u8, clock: Timestamp) {
        let timeout = self.config.ack_timeout;
        for ((replica, _), link) in &mut self.links {
            if *replica == n
                && link.waiting.as_ref().is_some_and(|w| elapsed(w.sent, clock, timeout))
            {
                link.waiting = None;
            }
        }
    }

    /// Node `n` ticked at `clock` and sent `to`, among its peers, the `have` frames of its
    /// rounds: each ends a stall, and a stall a round old must end.
    pub(crate) fn ticked(&mut self, n: u8, clock: Timestamp, rounds: &[u8]) -> Result<(), String> {
        let round = self.config.round;
        for ((replica, peer), link) in &mut self.links {
            if *replica != n {
                continue;
            }
            if rounds.contains(peer) {
                link.stalled = None;
            } else if let Some(since) = link.stalled
                && clock.duration_since(since).is_some_and(|passed| passed >= round)
            {
                return Err(format!(
                    "node {n} stalled towards node {peer} at {since:?}, and ticked at {clock:?} \
                     without beginning a round with it"
                ));
            }
        }
        Ok(())
    }

    /// Node `from` sends `to` a batch at `clock`: checks it may.
    pub(crate) fn sending(
        &mut self,
        from: u8,
        to: u8,
        frame: &[u8],
        clock: Timestamp,
    ) -> Result<(), String> {
        let Ok(Frame::Events(events)) = Frame::decode(frame) else { return Ok(()) };
        let link = self.links.entry((from, to)).or_default();
        if let Some(since) = link.stalled {
            return Err(format!(
                "node {from} sent node {to} batch {} while stalled towards it since {since:?}",
                events.batch
            ));
        }
        if let Some(waiting) = &link.waiting {
            return Err(format!(
                "node {from} sent node {to} batch {} at {clock:?}, while batch {}, sent at {:?}, \
                 waited for acknowledgement",
                events.batch, waiting.batch, waiting.sent
            ));
        }
        let mut first = VersionVector::new();
        for bytes in &events.events {
            let event = SignedEvent::from_stored(bytes).map_err(|error| format!("{error:?}"))?;
            let body = event.body();
            let position = first.entry(body.origin_device).or_insert(body.origin_seq.get());
            *position = (*position).min(body.origin_seq.get());
        }
        link.waiting = Some(Waiting { batch: events.batch, sent: clock, first });
        Ok(())
    }

    /// Node `to` is about to handle `frame` from `from`, at `clock`: a `have` acknowledging the
    /// batch waiting ends the wait, and stalls `to` towards `from` if `from` took none of it.
    pub(crate) fn receiving(&mut self, from: u8, to: u8, frame: &[u8], clock: Timestamp) {
        let Ok(Frame::Have(have)) = Frame::decode(frame) else { return };
        let Some(link) = self.links.get_mut(&(to, from)) else { return };
        let Some(waiting) = link.waiting.take_if(|waiting| waiting.batch == have.acked) else {
            return;
        };
        let took_some = waiting
            .first
            .iter()
            .any(|(device, first)| have.vv.get(device).is_some_and(|held| held >= first));
        if !took_some {
            link.stalled = Some(clock);
        }
    }
}
