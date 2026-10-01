//! Known answers for the election of the Store Hub (ADR-0022), frame by frame, against the model
//! replica.
//!
//! Replicas here are linked as each test says. Frames arrive at once, in the order they were
//! sent, unless the link is cut, its receiver is down, or it is a batch on a link that loses them.
//! Every replica up ticks at each whole second, the heartbeat period, all of them before any frame
//! sent at that tick arrives: a heartbeat sent as a period begins is heard in the period its
//! receiver began at the same tick.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU8;
use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use keel_domain::hub::{Claimed, Cut, Succession};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, Run, SequenceEvent};
use keel_events::event::SignedEvent;
use keel_sync::{Frame, Heartbeat, Outgoing, Replicator, Roles, SyncConfig};
use support::{Model, at, device, here, is_claim, is_record, registry};

/// A heartbeat every second, the hub lost after three silent, and batches large enough for
/// everything here.
const CONFIG: SyncConfig = SyncConfig {
    round: Duration::from_secs(5),
    ack_timeout: Duration::from_secs(2),
    batch_events: 64,
    batch_bytes: 256 * 1024,
    heartbeat: Duration::from_secs(1),
    silence: 3,
};

/// Replicas 1 to 4 at most, linked, with a network that delivers frames at once.
struct Net {
    models: BTreeMap<u8, Model>,
    replicators: BTreeMap<u8, Replicator>,
    roles: BTreeMap<u8, Roles>,
    links: BTreeSet<(u8, u8)>,
    down: BTreeSet<u8>,
    cut: BTreeSet<(u8, u8)>,
    /// Links on which batches are lost, from the first replica to the second.
    lossy: BTreeSet<(u8, u8)>,
    /// How far ahead of the net's time each replica's clock is, in milliseconds.
    ahead: BTreeMap<u8, i64>,
    queue: VecDeque<(u8, u8, Vec<u8>)>,
    now: i64,
    /// The last heartbeat each replica had from each peer: (replica, peer) → heartbeat.
    heard: BTreeMap<(u8, u8), Heartbeat>,
}

/// Replica `n`'s number, from its device.
fn replica_of(id: keel_types::Id<keel_events::envelope::Device>) -> u8 {
    (1..=4).find(|n| device(*n) == id).unwrap()
}

impl Net {
    /// Replicas of `priorities`, each with its priority as hub or none, linked as `links` says,
    /// none started.
    fn new(priorities: &[(u8, Option<u8>)], links: &[(u8, u8)]) -> Net {
        let models = priorities.iter().map(|&(n, _)| (n, Model::new(n, registry(1..=4)))).collect();
        let roles = priorities
            .iter()
            .map(|&(n, priority)| {
                (n, Roles { hub: priority.and_then(NonZeroU8::new), durable: None })
            })
            .collect();
        Net {
            models,
            replicators: BTreeMap::new(),
            roles,
            links: links.iter().map(|&(a, b)| (a.min(b), a.max(b))).collect(),
            down: priorities.iter().map(|&(n, _)| n).collect(),
            cut: BTreeSet::new(),
            lossy: BTreeSet::new(),
            ahead: BTreeMap::new(),
            queue: VecDeque::new(),
            now: 0,
            heard: BTreeMap::new(),
        }
    }

    fn time(&self, n: u8) -> keel_types::Timestamp {
        at(self.now + self.ahead.get(&n).copied().unwrap_or(0))
    }

    /// Starts replica `n` now, or starts it again: its replicator forgets everything, its events
    /// stay.
    fn start(&mut self, n: u8) {
        self.down.remove(&n);
        let peers: Vec<_> = self
            .links
            .iter()
            .filter_map(|&(a, b)| match n {
                _ if n == a => Some(device(b)),
                _ if n == b => Some(device(a)),
                _ => None,
            })
            .collect();
        let time = self.time(n);
        let model = self.models.get_mut(&n).unwrap();
        let (replicator, out) =
            Replicator::start(model, peers, CONFIG, self.roles[&n], time).unwrap();
        self.replicators.insert(n, replicator);
        self.send(n, out);
        self.deliver();
    }

    /// Starts every replica now.
    fn start_all(&mut self) {
        let all: Vec<u8> = self.models.keys().copied().collect();
        for n in all {
            self.start(n);
        }
    }

    /// Takes replica `n` down: it stops ticking, and frames to it are lost.
    fn crash(&mut self, n: u8) {
        self.down.insert(n);
    }

    fn send(&mut self, from: u8, out: Vec<Outgoing>) {
        for Outgoing { to, frame } in out {
            self.queue.push_back((from, replica_of(to), frame));
        }
    }

    /// Delivers every frame in flight, and every frame sent in answer, until none is left.
    fn deliver(&mut self) {
        while let Some((from, to, frame)) = self.queue.pop_front() {
            let decoded = Frame::decode(&frame).unwrap();
            let lost = matches!(decoded, Frame::Events(_)) && self.lossy.contains(&(from, to));
            let cut = self.cut.contains(&(from.min(to), from.max(to)));
            if lost || cut || self.down.contains(&to) {
                continue;
            }
            if let Frame::Heartbeat(heartbeat) = decoded {
                self.heard.insert((to, from), heartbeat);
            }
            let time = self.time(to);
            let model = self.models.get_mut(&to).unwrap();
            let replicator = self.replicators.get_mut(&to).unwrap();
            let out = replicator.on_frame(model, device(from), &frame, time).unwrap();
            self.send(to, out);
        }
    }

    /// Ticks every replica up at `ms`, then delivers what they sent.
    fn tick(&mut self, ms: i64) {
        self.now = ms;
        let up: Vec<u8> =
            self.replicators.keys().filter(|n| !self.down.contains(n)).copied().collect();
        for n in up {
            let time = self.time(n);
            let model = self.models.get_mut(&n).unwrap();
            let out = self.replicators.get_mut(&n).unwrap().on_tick(model, time).unwrap();
            self.send(n, out);
        }
        self.deliver();
    }

    /// Ticks at each whole second after now, up to `ms`.
    fn run_to(&mut self, ms: i64) {
        let mut next = (self.now / 1000 + 1) * 1000;
        while next <= ms {
            self.tick(next);
            next += 1000;
        }
        self.now = self.now.max(ms);
    }

    /// Replica `n` appends `count` events now.
    fn append(&mut self, n: u8, count: u64) {
        let time = self.time(n);
        let model = self.models.get_mut(&n).unwrap();
        let events: Vec<SignedEvent> = (0..count).map(|note| model.append(note, time)).collect();
        let out = self.replicators.get_mut(&n).unwrap().appended(model, &events, time).unwrap();
        self.send(n, out);
        self.deliver();
    }

    /// The winning claim replica `n` holds: its claimant and epoch.
    fn term(&self, n: u8) -> Option<(u8, u64)> {
        self.replicators[&n].term().map(|term| (replica_of(term.device), term.epoch.get()))
    }

    /// The replicas up serving as the hub.
    fn serving(&self) -> Vec<u8> {
        let up = self.replicators.iter().filter(|(n, _)| !self.down.contains(n));
        up.filter(|(_, r)| r.serving()).map(|(n, _)| *n).collect()
    }

    /// The claims replica `n` has made since it last started.
    fn claims(&self, n: u8) -> u64 {
        self.replicators[&n].stats().claims
    }

    /// The last heartbeat replica `to` had from `from`.
    fn heard(&self, to: u8, from: u8) -> Heartbeat {
        self.heard[&(to, from)]
    }

    /// Replica `n`'s own log.
    fn log(&self, n: u8) -> &[SignedEvent] {
        self.models[&n].logs().get(&device(n)).map_or(&[][..], Vec::as_slice)
    }

    /// The claim that is the `position`th event of replica `n`'s log.
    fn claim_at(&self, n: u8, position: usize) -> Claimed {
        let event = &self.log(n)[position - 1];
        assert!(is_claim(event), "{n}'s event {position} isn't a claim");
        let claims = self.models[&n].claims();
        claims.into_iter().find(|claim| claim.event == event.body().event_id).unwrap().claimed
    }

    /// The record that is the `position`th event of replica `n`'s log.
    fn record_at(&self, n: u8, position: usize) -> Assigned {
        let event = &self.log(n)[position - 1];
        assert!(is_record(event), "{n}'s event {position} isn't a record");
        let body = event.body();
        let Ok(SequenceEvent::Assigned(record)) =
            SequenceEvent::decode(&body.schema, &body.payload)
        else {
            panic!("an unreadable record");
        };
        record
    }
}

/// A heartbeat from a replica of `priority`, holding the claim of `term`, its hub and epoch, if
/// any, acting as the hub or not, with the hub's beat it gave, or had directly, at `beat_at`
/// milliseconds.
fn beat(priority: u8, term: Option<(u8, u64)>, acting: bool, beat_at: Option<i64>) -> Heartbeat {
    let beat = beat_at.map(|ms| u64::try_from(at(ms).as_micros()).unwrap());
    let (epoch, hub) = term.map_or((0, None), |(hub, epoch)| (epoch, Some(device(hub))));
    Heartbeat { location: here(), priority, epoch, hub, acting, beat }
}

/// A run of device `n`'s log.
fn run(model: &Model, n: u8, from: u64, to: u64) -> Run {
    let log = &model.logs()[&device(n)];
    Run { device: device(n), from, to, last: log[usize::try_from(to - 1).unwrap()].hash() }
}

/// Replica 1, of priority 2, the hub; replica 2, of priority 1, the standby; and replica 3, which
/// can't be the hub: each linked to the others, started at 0 and run to 4 s.
fn elected() -> Net {
    let mut net = Net::new(&[(1, Some(2)), (2, Some(1)), (3, None)], &[(1, 2), (1, 3), (2, 3)]);
    net.start_all();
    net.run_to(4_000);
    net
}

#[test]
fn the_most_preferred_candidate_claims_once_it_has_listened_for_the_periods_of_silence() {
    let mut net = Net::new(&[(1, Some(2)), (2, Some(1)), (3, None)], &[(1, 2), (1, 3), (2, 3)]);
    net.start_all();
    // Every replica heard from every peer at once, so each one's log is settled.
    assert!(net.replicators.values().all(Replicator::settled));
    // Through the first periods, no one claims, and each says its priority, and that it has no
    // claim and no hub.
    net.run_to(2_000);
    assert!(net.replicators.keys().all(|n| net.claims(*n) == 0 && net.term(*n).is_none()));
    assert_eq!(net.heard(2, 1), beat(2, None, false, None));
    assert_eq!(net.heard(1, 2), beat(1, None, false, None));
    assert_eq!(net.heard(1, 3), beat(0, None, false, None));
    // At the third, replica 1 claims epoch 1; replica 2, hearing replica 1, preferred, doesn't.
    net.run_to(3_000);
    assert_eq!((net.claims(1), net.claims(2)), (1, 0));
    assert!((1..=3).all(|n| net.term(n) == Some((1, 1))));
    assert_eq!(net.serving(), [1]);
    let claimed = Claimed::new(hub_epoch(1), NonZeroU8::new(2).unwrap(), None).unwrap();
    assert_eq!(net.claim_at(1, 1), claimed);
    // The hub numbers its claim like any event.
    let numbered = Assigned::new(1, 1, vec![run(&net.models[&1], 1, 1, 1)]).unwrap();
    assert_eq!(net.record_at(1, 2), numbered);
    assert_eq!(net.log(1).len(), 2);
    // Its heartbeats say it acts as the hub of epoch 1, with its beat, its clock's reading; the
    // others pass on the beat they had from it.
    net.run_to(4_000);
    assert_eq!(net.heard(2, 1), beat(2, Some((1, 1)), true, Some(4_000)));
    assert_eq!(net.heard(1, 2), beat(1, Some((1, 1)), false, Some(3_000)));
    assert_eq!(net.heard(2, 3), beat(0, Some((1, 1)), false, Some(3_000)));
    assert!(net.replicators.values().all(Replicator::hub_reachable));
}

fn hub_epoch(epoch: u64) -> keel_domain::hub::Epoch {
    keel_domain::hub::Epoch::new(epoch).unwrap()
}

#[test]
fn the_standby_takes_over_after_three_missed_heartbeats_and_not_before() {
    let mut net = elected();
    // The hub's last heartbeat went at 4 s; it fails just after, and replica 3 writes.
    net.crash(1);
    net.now = 4_500;
    net.append(3, 1);
    // Its heartbeats at 5, 6 and 7 s are missed: until the third, the standby still hears it.
    net.run_to(7_000);
    assert_eq!(net.claims(2), 0);
    assert_eq!(net.term(2), Some((1, 1)));
    assert!(net.replicators[&2].hub_reachable());
    assert_eq!(net.heard(2, 3), beat(0, Some((1, 1)), false, Some(4_000)));
    // As the period after the third begins, it claims epoch 2: 3.5 s after the hub failed.
    net.run_to(8_000);
    assert_eq!(net.claims(2), 1);
    assert_eq!((net.term(2), net.term(3)), (Some((2, 2)), Some((2, 2))));
    assert_eq!(net.serving(), [2]);
    // Its claim succeeds the hub's, cutting the hub's log where the standby held it: the claim
    // and its record.
    let previous = net.log(1)[0].body().event_id;
    let cuts = vec![Cut { device: device(1), position: 2 }];
    let succession = Succession { previous, cuts };
    let claimed = Claimed::new(hub_epoch(2), NonZeroU8::MIN, Some(succession)).unwrap();
    assert_eq!(net.claim_at(2, 1), claimed);
    // And it numbers, in epoch 2 and from 1, replica 3's event and its own claim, in the order
    // they came.
    let model = &net.models[&2];
    let runs = vec![run(model, 3, 1, 1), run(model, 2, 1, 1)];
    assert_eq!(net.record_at(2, 2), Assigned::new(2, 1, runs).unwrap());
    assert_eq!(net.heard(3, 2), beat(1, Some((2, 2)), true, Some(8_000)));
}

#[test]
fn a_candidate_defers_to_a_preferred_one_until_it_falls_silent() {
    // Of two candidates of one priority, the lower device identifier is preferred: replica 1
    // claims, and replica 2, hearing it, defers.
    let mut net = Net::new(&[(1, Some(1)), (2, Some(1))], &[(1, 2)]);
    net.start_all();
    net.run_to(3_000);
    assert_eq!((net.claims(1), net.claims(2)), (1, 0));
    assert_eq!(net.term(2), Some((1, 1)));
    // A preferred candidate silent since its heartbeat at 1 s holds replica 2 off until it has
    // missed three.
    let mut net = Net::new(&[(1, Some(1)), (2, Some(1))], &[(1, 2)]);
    net.start_all();
    net.run_to(1_000);
    net.crash(1);
    net.run_to(4_000);
    assert_eq!(net.claims(2), 0);
    net.run_to(5_000);
    assert_eq!(net.claims(2), 1);
    assert_eq!(net.term(2), Some((2, 1)));
}

#[test]
fn a_replica_hears_the_hub_through_a_peer_that_hears_it_directly_and_no_further() {
    // A line: replica 1, the hub; replica 3; replica 2, a candidate; and replica 4.
    let mut net =
        Net::new(&[(1, Some(2)), (2, Some(1)), (3, None), (4, None)], &[(1, 3), (3, 2), (2, 4)]);
    net.start(1);
    net.start(3);
    net.run_to(3_000);
    assert_eq!(net.term(3), Some((1, 1)));
    // Replicas 2 and 4 start later. Replica 2 hears the hub through replica 3 alone, which passes
    // on the hub's beats, and never claims.
    net.now = 3_500;
    net.start(2);
    net.start(4);
    net.run_to(10_000);
    assert_eq!(net.claims(2), 0);
    assert_eq!(net.term(2), Some((1, 1)));
    assert!(net.replicators[&2].hub_reachable());
    assert_eq!(net.heard(2, 3), beat(0, Some((1, 1)), false, Some(9_000)));
    // Replica 2 passes on nothing it hears through a peer: replica 4 hears no hub.
    assert_eq!(net.heard(4, 2), beat(1, Some((1, 1)), false, None));
    assert!(!net.replicators[&4].hub_reachable());
    // The hub fails after its heartbeat at 10 s. Replica 3 passes its last beat on until it has
    // missed three, and replica 2, which had it from replica 3 at 11 s, a period late, claims a
    // period later than a replica hearing the hub directly would: as the period after its third
    // without a new beat begins.
    net.crash(1);
    net.run_to(13_000);
    assert_eq!(net.heard(2, 3), beat(0, Some((1, 1)), false, Some(10_000)));
    net.run_to(14_000);
    assert_eq!(net.heard(2, 3), beat(0, Some((1, 1)), false, None));
    assert_eq!(net.claims(2), 0);
    net.run_to(15_000);
    assert_eq!(net.claims(2), 1);
    assert_eq!(net.term(2), Some((2, 2)));
}

/// The hub of [`elected`] writes two events at 4.5 s and fails, the batches carrying them reaching
/// replica 3 and not replica 2.
fn behind() -> Net {
    let mut net = elected();
    net.lossy.extend([(1, 2), (3, 2)]);
    net.now = 4_500;
    net.append(1, 2);
    net.crash(1);
    // The hub numbered them: its log is its claim, a record, the events and their record.
    assert_eq!(net.log(1).len(), 5);
    assert_eq!(net.models[&3].logs()[&device(1)].len(), 5);
    assert_eq!(net.models[&2].logs()[&device(1)].len(), 2);
    net
}

#[test]
fn a_candidate_behind_a_peer_on_the_hubs_log_waits_to_catch_up() {
    let mut net = behind();
    // At 8 s replica 2 would claim, but replica 3 has said, in its round at 5 s, that it holds
    // more of the hub's log: replica 2 waits.
    net.run_to(8_000);
    assert_eq!(net.claims(2), 0);
    // Replica 3's batches get through again: the next, at 9 s, after replica 2's tick, catches it
    // up, and it claims at 10 s, cutting the hub's log after all of it.
    net.lossy.clear();
    net.run_to(9_000);
    assert_eq!(net.claims(2), 0);
    net.run_to(10_000);
    assert_eq!(net.claims(2), 1);
    let previous = net.log(1)[0].body().event_id;
    let succession = Succession { previous, cuts: vec![Cut { device: device(1), position: 5 }] };
    let claimed = Claimed::new(hub_epoch(2), NonZeroU8::MIN, Some(succession)).unwrap();
    assert_eq!(net.claim_at(2, 1), claimed);
    // The hub's record of its events counts, so replica 2 numbers only its claim.
    let numbered = Assigned::new(2, 1, vec![run(&net.models[&2], 2, 1, 1)]).unwrap();
    assert_eq!(net.record_at(2, 2), numbered);
    assert_eq!(net.log(2).len(), 2);
}

#[test]
fn a_candidate_that_cant_catch_up_claims_after_waiting_the_periods_of_silence() {
    let mut net = behind();
    // Replica 2 starts waiting at 8 s; at 11 s it has waited three periods, and claims, cutting
    // the hub's log where it holds it.
    net.run_to(10_000);
    assert_eq!(net.claims(2), 0);
    net.run_to(11_000);
    assert_eq!(net.claims(2), 1);
    let previous = net.log(1)[0].body().event_id;
    let succession = Succession { previous, cuts: vec![Cut { device: device(1), position: 2 }] };
    let claimed = Claimed::new(hub_epoch(2), NonZeroU8::MIN, Some(succession)).unwrap();
    assert_eq!(net.claim_at(2, 1), claimed);
    // Once it has the hub's events, it numbers them again: the hub's record of them lies past
    // its cut.
    net.lossy.clear();
    net.run_to(14_000);
    assert_eq!(net.models[&2].logs()[&device(1)].len(), 5);
    let numbered = Assigned::new(2, 2, vec![run(&net.models[&2], 1, 3, 4)]).unwrap();
    assert_eq!(net.record_at(2, 3), numbered);
}

/// Found by the simulator: a hub that had told the standby it held records no one else did, then
/// crashed, held the standby off for the periods of silence more, waiting to catch up on a log
/// only the dead hub held.
#[test]
fn a_candidate_doesnt_wait_for_what_only_the_silent_hub_held() {
    let mut net = elected();
    // The hub writes two events at 4.5 s that reach no one; its round at 5 s tells the others
    // what it holds, and it fails just after.
    net.lossy.extend([(1, 2), (1, 3)]);
    net.now = 4_500;
    net.append(1, 2);
    net.run_to(5_000);
    assert_eq!(net.replicators[&2].known(device(1)).map(|vv| vv[&device(1)]), Some(5));
    net.crash(1);
    // Its last heartbeat was at 5 s: the standby claims at 9 s, cutting the hub's log where it
    // and replica 3 hold it, without waiting for the hub's events.
    net.run_to(8_000);
    assert_eq!(net.claims(2), 0);
    net.run_to(9_000);
    assert_eq!(net.claims(2), 1);
    let previous = net.log(1)[0].body().event_id;
    let succession = Succession { previous, cuts: vec![Cut { device: device(1), position: 2 }] };
    let claimed = Claimed::new(hub_epoch(2), NonZeroU8::MIN, Some(succession)).unwrap();
    assert_eq!(net.claim_at(2, 1), claimed);
}

#[test]
fn a_hub_steps_down_when_a_better_claim_reaches_it() {
    let mut net = elected();
    // The store's network splits, replica 1 alone on one side. On the other, replica 2 claims
    // epoch 2 at 8 s; replica 1 goes on serving epoch 1, numbering what its device writes.
    net.cut.extend([(1, 2), (1, 3)]);
    net.run_to(8_000);
    assert_eq!(net.claims(2), 1);
    net.now = 8_500;
    net.append(1, 1);
    assert_eq!(net.serving(), [1, 2]);
    let numbered = Assigned::new(1, 2, vec![run(&net.models[&1], 1, 3, 3)]).unwrap();
    assert_eq!(net.record_at(1, 4), numbered);
    // The split heals. At 10 s, replica 2's claim reaches replica 1, which stops serving at once.
    net.cut.clear();
    net.run_to(10_000);
    assert_eq!(net.serving(), [2]);
    assert_eq!(net.term(1), Some((2, 2)));
    // Replica 2 numbers replica 1's event again, in epoch 2: replica 1's record of it lies past
    // replica 2's cut.
    net.run_to(12_000);
    let numbered = Assigned::new(2, 2, vec![run(&net.models[&2], 1, 3, 3)]).unwrap();
    assert_eq!(net.record_at(2, 3), numbered);
    // Replica 1 wrote nothing more, hears replica 2 directly, and, though it is preferred, never
    // takes the role back: its only claim is its first.
    net.run_to(20_000);
    assert_eq!(net.log(1).len(), 4);
    assert_eq!(net.heard(2, 1), beat(2, Some((2, 2)), false, Some(19_000)));
    assert_eq!(net.claims(1), 1);
    assert_eq!(net.serving(), [2]);
}

#[test]
fn a_replica_goes_on_hearing_its_hub_beside_a_losing_hub_of_its_epoch() {
    // Replicas 1 and 2, of one priority; replica 3, linked to both, and replica 4, to replica 2
    // alone. Replica 2's clock is 5 s ahead, and its beats with it.
    let mut net =
        Net::new(&[(1, Some(1)), (2, Some(1)), (3, None), (4, None)], &[(1, 3), (2, 3), (2, 4)]);
    net.ahead.insert(2, 5_000);
    // Split from replicas 1 and 3, replica 2 claims epoch 1 beside replica 1: two hubs of one
    // epoch, each with a replica that hears it.
    net.cut.insert((2, 3));
    net.start_all();
    net.run_to(4_000);
    assert_eq!((net.claims(1), net.claims(2)), (1, 1));
    assert_eq!((net.term(3), net.term(4)), (Some((1, 1)), Some((2, 1))));
    assert_eq!(net.serving(), [1, 2]);
    // The split heals. Replica 1's claim wins, of the lower device, and replica 2 stops serving
    // once it learns of it. Replica 3 hears replica 2 acting as a hub of epoch 1 until then, its
    // beats seconds ahead of replica 1's; it goes on hearing its own hub, replica 1, period after
    // period, and no one claims again.
    net.cut.clear();
    for ms in (5..=20).map(|seconds| seconds * 1_000) {
        net.run_to(ms);
        assert!(net.replicators[&3].hub_reachable(), "at {ms} ms");
    }
    assert_eq!(net.serving(), [1]);
    assert_eq!((net.claims(1), net.claims(2)), (1, 1));
    assert_eq!((net.term(2), net.term(3)), (Some((1, 1)), Some((1, 1))));
}

#[test]
fn a_heartbeat_acting_as_another_devices_hub_gives_no_beat() {
    // Replica 1 holds no claim, and hears no hub.
    let mut net = Net::new(&[(1, None), (2, None), (3, None)], &[(1, 2), (1, 3)]);
    net.start_all();
    let acting_for = |hub: u8| {
        let heartbeat = Heartbeat {
            location: here(),
            priority: 1,
            epoch: 1,
            hub: Some(device(hub)),
            acting: true,
            beat: Some(5_000),
        };
        Frame::Heartbeat(heartbeat).encode()
    };
    let hear = |net: &mut Net, from: u8| {
        let (model, replicator) =
            (net.models.get_mut(&1).unwrap(), net.replicators.get_mut(&1).unwrap());
        replicator.on_frame(model, device(from), &acting_for(3), at(net.now)).unwrap();
        replicator.hub_reachable()
    };
    // Replica 2 says it acts as the hub of replica 3's term, as no replica says of another's:
    // that gives no beat. Replica 3 saying it of its own term gives one.
    assert!(!hear(&mut net, 2));
    assert!(hear(&mut net, 3));
}

#[test]
fn a_hub_that_restarts_resumes_its_term_once_settled() {
    // Replica 1, of priority 1, claims while replica 2, of priority 2, is down.
    let mut net = Net::new(&[(1, Some(1)), (2, Some(2)), (3, None)], &[(1, 2), (1, 3), (2, 3)]);
    net.start(1);
    net.start(3);
    net.run_to(3_000);
    assert_eq!(net.term(1), Some((1, 1)));
    // Replica 2 starts later, and, hearing a hub, never claims, though it is preferred.
    net.now = 3_500;
    net.start(2);
    net.run_to(6_000);
    assert_eq!(net.term(2), Some((1, 1)));
    // The hub restarts. Its first heartbeats give no priority, its log not yet settled, and say
    // it isn't acting; but its peers answer at once, so it serves again, in the same term.
    net.now = 6_500;
    net.start(1);
    assert_eq!(net.heard(2, 1), beat(0, Some((1, 1)), false, None));
    assert_eq!(net.serving(), [1]);
    // Replica 2 still had its last heartbeat acting as the hub, and didn't claim meanwhile.
    net.run_to(12_000);
    assert_eq!((net.claims(1), net.claims(2)), (0, 0));
    assert_eq!(net.term(2), Some((1, 1)));
    assert_eq!(net.heard(2, 1), beat(1, Some((1, 1)), true, Some(12_000)));
    assert_eq!(net.log(1).len(), 2);
}

#[test]
fn a_log_settles_against_every_peer_that_answers() {
    // Replica 1 holds two events of its own, which replica 2 holds too.
    let net_of = || {
        let mut net = Net::new(&[(1, None), (2, None), (3, None)], &[(1, 2), (1, 3)]);
        let events: Vec<_> =
            (1..=2).map(|note| net.models.get_mut(&1).unwrap().append(note, at(-1_000))).collect();
        for event in &events {
            net.models.get_mut(&2).unwrap().receive_one(&event.to_bytes());
        }
        net.start(2);
        net.start(3);
        net
    };
    // When every peer answers, it is settled at once.
    let mut net = net_of();
    net.start(1);
    assert!(net.replicators[&1].settled());
    // Replica 3 can't be reached: replica 2's answer alone settles the log only once a period
    // has passed since it came.
    let mut net = net_of();
    net.cut.insert((1, 3));
    net.start(1);
    assert!(!net.replicators[&1].settled());
    net.run_to(1_000);
    assert!(net.replicators[&1].settled());
}

#[test]
fn a_peer_holding_more_of_a_replicas_log_holds_it_unsettled_until_it_has_it() {
    // Replica 1 was restored from a copy holding one of its three events; replica 2 holds all.
    let mut net = Net::new(&[(1, None), (2, None)], &[(1, 2)]);
    let mut old = Model::new(1, registry(1..=4));
    let events: Vec<_> = (1..=3).map(|note| old.append(note, at(-1_000))).collect();
    for event in &events {
        net.models.get_mut(&2).unwrap().receive_one(&event.to_bytes());
    }
    net.models.get_mut(&1).unwrap().receive_one(&events[0].to_bytes());
    // Replica 2's batch of the rest is lost: replica 1's log isn't settled, however long.
    net.lossy.insert((2, 1));
    net.start(2);
    net.start(1);
    net.run_to(1_000);
    assert!(!net.replicators[&1].settled());
    // Replica 2 sends it again when the batch's acknowledgement times out, at 2 s, and replica 1,
    // holding it all, is settled.
    net.lossy.clear();
    net.run_to(2_000);
    assert!(net.replicators[&1].settled());
    assert_eq!(net.models[&1].logs()[&device(1)].len(), 3);
}

#[test]
fn a_clock_jumping_begins_one_heartbeat_period_and_no_more() {
    let mut net = elected();
    // Replica 2's clock jumps an hour forwards: its next tick begins one period, not an hour of
    // them, so the hub isn't taken for silent.
    net.ahead.insert(2, 3_600_000);
    net.run_to(12_000);
    assert_eq!(net.claims(2), 0);
    assert!(net.replicators[&2].hub_reachable());
    // And two hours back: a clock set back begins a period at once, and then one a second.
    net.ahead.insert(2, -3_600_000);
    net.run_to(20_000);
    assert_eq!(net.claims(2), 0);
    assert!(net.replicators[&2].hub_reachable());
    assert_eq!(net.serving(), [1]);
}

#[test]
fn a_replica_whose_log_forked_gives_no_priority_and_never_claims() {
    // Replica 1, of priority 2, was restored from a copy holding the first of its two events, and
    // wrote one before it could reach replica 2, which holds both: its log forks.
    let mut net = Net::new(&[(1, Some(2)), (2, Some(1))], &[(1, 2)]);
    let mut old = Model::new(1, registry(1..=4));
    let events: Vec<_> = (1..=2).map(|note| old.append(note, at(-2_000))).collect();
    for event in &events {
        net.models.get_mut(&2).unwrap().receive_one(&event.to_bytes());
    }
    net.models.get_mut(&1).unwrap().receive_one(&events[0].to_bytes());
    net.models.get_mut(&1).unwrap().append(99, at(-1_000));
    // Holding as much of its log as replica 2, its log is settled; nothing shows the fork yet.
    net.start_all();
    assert!(net.replicators[&1].settled());
    assert!(!net.replicators[&1].forked());
    // Replica 2 refuses its next event, which follows what replica 2 holds of its log.
    net.append(1, 1);
    assert!(net.replicators[&1].forked());
    // Its heartbeats give no priority, so replica 2 doesn't defer to it, and claims; it never
    // does.
    net.run_to(1_000);
    assert_eq!(net.heard(2, 1), beat(0, None, false, None));
    net.run_to(10_000);
    assert_eq!((net.claims(1), net.claims(2)), (0, 1));
    assert_eq!(net.serving(), [2]);
}
