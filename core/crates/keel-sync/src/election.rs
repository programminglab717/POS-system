//! What a replica hears of the Store Hub and of the other candidates for its role (ADR-0022),
//! from their heartbeats, counted in heartbeat periods: the replicator's own heartbeat ticks,
//! never read off the clock, so that a clock jumping forwards can't make a hub look silent.
//!
//! - **Beats.** The hub raises its beat with every period it acts, and gives it in its
//!   heartbeats; a replica that had it directly passes on the latest it had, while that is
//!   recent. A heartbeat names the term its beat is of, the epoch and the hub's device, and beats
//!   are compared only within one term: after a split, two hubs can hold one epoch, and their
//!   beats, read off their own clocks, say nothing of each other.
//! - Something heard is **recent** in the period it came in and the periods of silence after it:
//!   a peer counts as silent from the start of the period after it missed that many heartbeats.
//! - A replica **hears the hub** while the latest beat it knows of its own term's hub, or of the
//!   hub of any later epoch, first reached it recently, from the hub or from a peer that had it
//!   directly. A peer passes on only beats it had from the hub itself, so a dead hub can't be
//!   kept alive by replicas vouching for each other; and a beat that reaches a replica first
//!   directly, then again through a peer, is no newer for it: hearing the hub through a peer
//!   never outlasts hearing it directly. A replica that hears the hub only through peers hears
//!   each beat up to a period late, and so counts the hub lost up to a period later.
//! - Learning of a winning claim another replica made counts as hearing its hub, in the period
//!   it was learned. A claim travels faster than the beats of its hub, which a peer passes on only
//!   as its next period begins; without this, a replica whose epoch a new claim had just raised
//!   would hear no hub of it, and claim the next epoch at once.
//! - A peer would be **preferred** as hub to the replica if, by its last heartbeat, recent, its
//!   priority is higher, or equal and its device identifier lower.

use std::collections::BTreeMap;

use keel_events::envelope::Device;
use keel_types::Id;

use crate::frame::Heartbeat;

/// A hub's term, as a heartbeat names it: its epoch and the hub's device.
pub(crate) type Term = (u64, Id<Device>);

/// A beat, and the period it first came in.
#[derive(Clone, Copy, Debug)]
struct Heard {
    beat: u64,
    period: u64,
}

/// The later of `heard` and `beat`, of the same term, which came in `period`.
fn later(heard: Option<Heard>, beat: u64, period: u64) -> Heard {
    match heard {
        Some(heard) if heard.beat >= beat => heard,
        _ => Heard { beat, period },
    }
}

/// What a replica has heard from one peer.
#[derive(Clone, Copy, Debug)]
struct Peer {
    /// The period of its last heartbeat.
    spoke: u64,
    /// Its priority as hub, by its last heartbeat.
    priority: u8,
    /// The latest beat it gave acting as the hub, and the epoch of its term.
    acting: Option<(u64, Heard)>,
}

/// What a replica has heard, period by period.
#[derive(Clone, Debug, Default)]
pub(crate) struct Election {
    /// The heartbeat periods begun since the replicator started.
    period: u64,
    peers: BTreeMap<Id<Device>, Peer>,
    /// The latest beat the replica knows of each hub's term, of its own epoch or a later one,
    /// from the hub or from a peer that had it directly.
    latest: BTreeMap<Term, Heard>,
    /// The period in which the replica learned of the winning claim it holds, if another replica
    /// made it.
    learned: Option<u64>,
    /// The period since which the replica, ready to claim, has held less of an earlier hub's log
    /// than a peer said it holds.
    behind_since: Option<u64>,
}

impl Election {
    /// Begins the next period, for a replica of `epoch`: the beats of earlier epochs' hubs are
    /// forgotten, since they can never again be heard as its hub's.
    pub(crate) fn tick(&mut self, epoch: u64) {
        self.period = self.period.saturating_add(1);
        self.latest.retain(|&(of, _), _| of >= epoch);
    }

    /// The heartbeat periods begun since the replicator started.
    pub(crate) const fn period(&self) -> u64 {
        self.period
    }

    /// Notes `heartbeat`, from `from`, in the current period. A heartbeat that says it acts as a
    /// hub other than its sender gives no beat.
    pub(crate) fn hear(&mut self, from: Id<Device>, heartbeat: &Heartbeat) {
        let period = self.period;
        let peer = self.peers.entry(from).or_insert(Peer {
            spoke: period,
            priority: heartbeat.priority,
            acting: None,
        });
        peer.spoke = period;
        peer.priority = heartbeat.priority;
        let (Some(hub), Some(beat)) = (heartbeat.hub, heartbeat.beat) else { return };
        if heartbeat.acting {
            if hub != from {
                return;
            }
            let epoch = heartbeat.epoch;
            peer.acting = match peer.acting {
                Some((of, heard)) if of > epoch => Some((of, heard)),
                Some((of, heard)) if of == epoch => Some((of, later(Some(heard), beat, period))),
                _ => Some((epoch, Heard { beat, period })),
            };
        }
        let term = (heartbeat.epoch, hub);
        let heard = later(self.latest.get(&term).copied(), beat, period);
        self.latest.insert(term, heard);
    }

    /// Notes that the replica has just learned of a winning claim another replica made.
    pub(crate) fn learn_claim(&mut self) {
        self.learned = Some(self.period);
    }

    /// Whether something heard in `period` is recent: in the current period or the `silence`
    /// periods before it.
    const fn is_recent(&self, period: u64, silence: u64) -> bool {
        self.period.saturating_sub(period) <= silence
    }

    /// The latest beat the replica had directly from the hub of `term`, the replica's own, if it
    /// is recent: the beat the replica passes on.
    pub(crate) fn beat_heard_directly(&self, term: Option<Term>, silence: u64) -> Option<u64> {
        let (epoch, hub) = term?;
        let (of, heard) = self.peers.get(&hub)?.acting?;
        (of == epoch && self.is_recent(heard.period, silence)).then_some(heard.beat)
    }

    /// Whether the replica, holding the winning claim of `term`, if any, learned of that claim
    /// recently, or the latest beat it knows of that term's hub, or of the hub of a later epoch,
    /// first reached it recently.
    pub(crate) fn hears_hub(&self, term: Option<Term>, silence: u64) -> bool {
        self.learned.is_some_and(|learned| self.is_recent(learned, silence))
            || self.latest.iter().any(|(&(epoch, hub), heard)| {
                self.is_recent(heard.period, silence)
                    && term.is_none_or(|(own, of)| epoch > own || (epoch == own && hub == of))
            })
    }

    /// Whether a peer that would be preferred as hub to device `own`, of `priority`, said so in
    /// its last heartbeat, recent.
    pub(crate) fn hears_preferred(&self, own: Id<Device>, priority: u8, silence: u64) -> bool {
        self.peers.iter().any(|(&from, peer)| {
            self.is_recent(peer.spoke, silence)
                && (peer.priority > priority || (peer.priority == priority && from < own))
        })
    }

    /// Whether the replica, ready to claim but behind a peer on an earlier hub's log, has waited
    /// `silence` periods for it: noted now if it hadn't started waiting.
    pub(crate) fn waited_behind(&mut self, silence: u64) -> bool {
        let since = *self.behind_since.get_or_insert(self.period);
        self.period.saturating_sub(since) >= silence
    }

    /// Stops waiting to catch up: the replica isn't ready to claim, or is caught up.
    pub(crate) fn stop_waiting(&mut self) {
        self.behind_since = None;
    }
}
