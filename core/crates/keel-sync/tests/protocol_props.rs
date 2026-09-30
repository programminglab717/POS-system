//! Property tests for the replication protocol (ADR-0019), against the model replica.
//!
//! A case is a few replicas, linked as a star, a line or a mesh, appending events at random
//! times, over a network that delays every frame, loses and duplicates some, and cuts links for
//! a while. Replicas restart now and then, losing what their replicator knew but none of their
//! events, and at times decline a batch, as a replica that can't take it for now would, so that
//! its sender stalls. After twenty seconds the faults stop. Within a minute more, every replica
//! must hold exactly every event appended and the hub's records, and none may have refused any.
//! Replica 1 is the Store Hub, sequencing in epoch 1, and the last replica is the durable one,
//! which the replicas linked to it name as their durable peer (ADR-0020). All along, the
//! protocol's rules must hold, checked frame by frame:
//!
//! - no replica sends a peer an event the peer last told it it holds, or sent it since;
//! - a batch holds at most the events and bytes configured, unless its one event is more bytes;
//! - a replica sends a peer a batch only once the last is acknowledged or its acknowledgement
//!   timeout has passed, and numbers its batches to each peer afresh, even across restarts;
//! - a replica whose last batch a peer took none of sends the peer nothing more until its next
//!   round, and a tick a round or more after the stall began is that round;
//! - a replica acknowledges each batch it receives at once;
//! - a replica's `have` asks for the peer's in return exactly until it has heard from the peer
//!   since it started, and a replica answers a `have` that asks at once;
//! - a replica's ticks do nothing before [`Replicator::next_tick`] says they are needed;
//! - a `durable` frame claims no more of any device's log than the durable replica holds, and a
//!   replicator's watermark only rises;
//! - the hub, once its log is settled, leaves nothing it holds unnumbered.
//!
//! And after healing, every event but the records is numbered once, gapless, and every
//! replica's watermark but the durable replica's reaches everything it holds.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};

use keel_events::envelope::Device;
use keel_events::event::SignedEvent;
use keel_sync::{Durable, Frame, Outgoing, Replica, Replicator, Roles, SyncConfig, VersionVector};
use keel_types::{Entropy, Id, SeededEntropy};
use proptest::prelude::*;
use support::{Model, at, device, is_record, registry};

#[derive(Clone, Copy, Debug)]
enum Topology {
    /// Replica 1 is the hub; every other replica is linked to it alone.
    Star,
    /// Each replica is linked to the next.
    Line,
    /// Every replica is linked to every other.
    Mesh,
}

#[derive(Clone, Debug)]
struct Case {
    replicas: u8,
    topology: Topology,
    /// Per thousand frames.
    loss: u64,
    duplication: u64,
    /// Per thousand batches received.
    decline: u64,
    /// The longest a frame takes, in milliseconds.
    delay: u64,
    /// When (in tenths of a second), which replica, and how many events.
    appends: Vec<(u16, u8, u8)>,
    /// Which link, from when, for how long (in tenths of a second).
    cuts: Vec<(u8, u16, u16)>,
    /// When, and which replica, restarts.
    restarts: Vec<(u16, u8)>,
    /// The most events and bytes in a batch.
    batch_events: u32,
    batch_bytes: usize,
    seed: u64,
}

fn any_case() -> impl Strategy<Value = Case> {
    (2_u8..=4).prop_flat_map(|replicas| {
        (
            Just(replicas),
            prop_oneof![Just(Topology::Star), Just(Topology::Line), Just(Topology::Mesh)],
            (
                prop_oneof![1 => Just(0_u64), 3 => 0_u64..300],
                prop_oneof![1 => Just(0_u64), 3 => 0_u64..200],
                prop_oneof![1 => Just(0_u64), 3 => 0_u64..300],
            ),
            1_u64..300,
            prop::collection::vec((0_u16..200, 1..=replicas, 1_u8..4), 1..12),
            prop::collection::vec((any::<u8>(), 0_u16..200, 1_u16..80), 0..4),
            prop::collection::vec((0_u16..200, 1..=replicas), 0..3),
            // A few events a batch, and at times too few bytes for more than one or two: an
            // event is some 300 bytes.
            (1_u32..=6, prop_oneof![Just(256 * 1024), 1_usize..1200]),
            any::<u64>(),
        )
            .prop_map(
                |(
                    replicas,
                    topology,
                    (loss, duplication, decline),
                    delay,
                    appends,
                    cuts,
                    restarts,
                    (batch_events, batch_bytes),
                    seed,
                )| Case {
                    replicas,
                    topology,
                    loss,
                    duplication,
                    decline,
                    delay,
                    appends,
                    cuts,
                    restarts,
                    batch_events,
                    batch_bytes,
                    seed,
                },
            )
    })
}

fn links(case: &Case) -> Vec<(u8, u8)> {
    let n = case.replicas;
    match case.topology {
        Topology::Star => (2..=n).map(|b| (1, b)).collect(),
        Topology::Line => (1..n).map(|a| (a, a + 1)).collect(),
        Topology::Mesh => (1..=n).flat_map(|a| (a + 1..=n).map(move |b| (a, b))).collect(),
    }
}

fn peers_of(links: &[(u8, u8)], replica: u8) -> Vec<Id<Device>> {
    links
        .iter()
        .filter_map(|&(a, b)| match replica {
            r if r == a => Some(device(b)),
            r if r == b => Some(device(a)),
            _ => None,
        })
        .collect()
}

/// Replica 1 sequences, in epoch 1; the last replica is the durable one, and each replica
/// linked to it names it its durable peer.
fn roles_of(case: &Case, links: &[(u8, u8)], replica: u8) -> Roles {
    let durable = case.replicas;
    let linked = links.iter().any(|&link| link == (replica, durable) || link == (durable, replica));
    Roles {
        sequencer: (replica == 1).then_some(1),
        durable: (linked && replica != durable).then(|| device(durable)),
    }
}

const ACK_TIMEOUT: i64 = 2_000;
const ROUND: i64 = 5_000;

fn config(case: &Case) -> SyncConfig {
    SyncConfig {
        round: Duration::from_millis(ROUND.unsigned_abs()),
        ack_timeout: Duration::from_millis(ACK_TIMEOUT.unsigned_abs()),
        batch_events: case.batch_events,
        batch_bytes: case.batch_bytes,
    }
}

/// Faults stop, and the network heals, after this many milliseconds.
const HEAL: i64 = 20_000;
/// The replicas must agree this long after healing.
const AGREE_WITHIN: i64 = 60_000;

struct World {
    case: Case,
    config: SyncConfig,
    links: Vec<(u8, u8)>,
    models: BTreeMap<u8, Model>,
    replicators: BTreeMap<u8, Replicator>,
    /// Frames in flight: when they arrive, in what order, from, to, and the frame.
    queue: BTreeMap<(i64, u64), (u8, u8, Vec<u8>)>,
    sent: u64,
    rng: SeededEntropy,
    now: i64,
    /// What each replica may take each peer to hold, since it last started: what the peer last
    /// told it it holds, raised by the events the peer sent it that it stored since.
    /// (replica, peer) → version vector.
    told: BTreeMap<(u8, u8), VersionVector>,
    /// The batch each replica sent each peer and hasn't yet had acknowledged, since it last
    /// started, unless its acknowledgement timeout has passed.
    in_flight: BTreeMap<(u8, u8), InFlight>,
    /// Since when each replica has been stalled towards a peer, since it last started.
    stalled: BTreeMap<(u8, u8), i64>,
    /// The number of the last batch each replica sent each peer, ever.
    numbered: BTreeMap<(u8, u8), u64>,
    /// Each replica and a peer it has had a `have` from since it last started.
    heard: BTreeSet<(u8, u8)>,
    /// Each replicator's watermark, as last seen, since it last started.
    watermarks: BTreeMap<u8, VersionVector>,
    appended: Vec<SignedEvent>,
}

/// A batch sent and not yet acknowledged: its number, when it was sent, and the first position of
/// each device's events in it.
struct InFlight {
    batch: u64,
    sent: i64,
    first: VersionVector,
}

fn replica_of(id: Id<Device>) -> u8 {
    (1..=8).find(|n| device(*n) == id).unwrap()
}

impl World {
    fn new(case: Case) -> World {
        let links = links(&case);
        let mut models = BTreeMap::new();
        let mut replicators = BTreeMap::new();
        let mut world = World {
            rng: SeededEntropy::new(case.seed),
            config: config(&case),
            case,
            links,
            models: BTreeMap::new(),
            replicators: BTreeMap::new(),
            queue: BTreeMap::new(),
            sent: 0,
            now: 0,
            told: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            stalled: BTreeMap::new(),
            numbered: BTreeMap::new(),
            heard: BTreeSet::new(),
            watermarks: BTreeMap::new(),
            appended: Vec::new(),
        };
        let mut starting = Vec::new();
        for n in 1..=world.case.replicas {
            let mut model = Model::new(n, registry(1..=world.case.replicas));
            let roles = roles_of(&world.case, &world.links, n);
            let (replicator, out) = Replicator::start(
                &mut model,
                peers_of(&world.links, n),
                world.config,
                roles,
                at(0),
            )
            .unwrap();
            models.insert(n, model);
            replicators.insert(n, replicator);
            starting.push((n, out));
        }
        world.models = models;
        world.replicators = replicators;
        for (n, out) in starting {
            world.send(n, out);
        }
        world
    }

    fn random(&mut self, below: u64) -> u64 {
        self.rng.next_u64().unwrap() % below
    }

    fn cut(&self, a: u8, b: u8, now: i64) -> bool {
        now < HEAL
            && self.case.cuts.iter().any(|&(link, start, len)| {
                let (x, y) = self.links[usize::from(link) % self.links.len()];
                let (start, end) = (i64::from(start) * 100, i64::from(start + len) * 100);
                ((x, y) == (a, b) || (x, y) == (b, a)) && (start..end).contains(&now)
            })
    }

    /// Sends `out` from replica `from`: each frame after a random delay, perhaps lost or
    /// duplicated while the network is faulty. Checks each batch against the protocol's rules.
    fn send(&mut self, from: u8, out: Vec<Outgoing>) {
        for Outgoing { to, frame } in out {
            let to = replica_of(to);
            match Frame::decode(&frame).unwrap() {
                Frame::Events(events) => self.check_batch(from, to, events.batch, &events.events),
                Frame::Have(have) => {
                    let heard = self.heard.contains(&(from, to));
                    assert_eq!(have.asks, !heard, "{from}'s have to {to}, having heard: {heard}");
                }
                Frame::Durable(durable) => self.check_durable(from, to, &durable),
            }
            let faulty = self.now < HEAL;
            let copies = if faulty && self.random(1000) < self.case.duplication { 2 } else { 1 };
            for _ in 0..copies {
                if faulty && self.random(1000) < self.case.loss {
                    continue;
                }
                let delay = i64::try_from(1 + self.random(self.case.delay)).unwrap();
                self.sent += 1;
                self.queue.insert((self.now + delay, self.sent), (from, to, frame.clone()));
            }
        }
    }

    /// Checks a batch `from` sends `to`: it is no larger than configured, none of its events is
    /// one `to` holds as far as `from` may know, the last batch is acknowledged or its
    /// acknowledgement timeout has passed, `from` isn't stalled towards `to`, and the batch's
    /// number is new.
    fn check_batch(&mut self, from: u8, to: u8, batch: u64, events: &[Vec<u8>]) {
        let bytes: usize = events.iter().map(Vec::len).sum();
        assert!(
            events.len() <= usize::try_from(self.config.batch_events).unwrap()
                && (events.len() == 1 || bytes <= self.config.batch_bytes),
            "{from} sent {to} {} events, {bytes} bytes, in batch {batch}",
            events.len(),
        );
        let told = self.told.get(&(from, to)).cloned().unwrap_or_default();
        let mut first = VersionVector::new();
        for bytes in events {
            let event = SignedEvent::from_stored(bytes).unwrap();
            let body = event.body();
            let held = told.get(&body.origin_device).copied().unwrap_or(0);
            assert!(
                body.origin_seq.get() > held,
                "{from} sent {to} event {} of {:?}, which {to} holds up to {held}, as it knows",
                body.origin_seq,
                body.origin_device,
            );
            first.entry(body.origin_device).or_insert(body.origin_seq.get());
        }
        if let Some(waiting) = self.in_flight.get(&(from, to)) {
            panic!(
                "{from} sent {to} batch {batch} at {}, while batch {}, sent at {}, waits",
                self.now, waiting.batch, waiting.sent,
            );
        }
        if let Some(since) = self.stalled.get(&(from, to)) {
            panic!("{from} sent {to} batch {batch} at {}, stalled since {since}", self.now);
        }
        let last = self.numbered.insert((from, to), batch);
        assert!(
            last.is_none_or(|last| batch > last),
            "{from} numbered a batch {batch} after {last:?}"
        );
        self.in_flight.insert((from, to), InFlight { batch, sent: self.now, first });
    }

    /// Checks a `durable` frame: it claims no more than the durable replica holds, and doesn't
    /// go to the durable replica itself.
    fn check_durable(&self, from: u8, to: u8, durable: &Durable) {
        let holder = self.case.replicas;
        assert_ne!(to, holder, "{from} told the durable replica its own watermark");
        let held = self.models[&holder].logs();
        for (origin, &position) in &durable.vv {
            let holds = held.get(origin).map_or(0, Vec::len);
            assert!(
                position <= u64::try_from(holds).unwrap(),
                "{from} told {to} the durable replica holds {origin:?} up to {position}; it holds {holds}",
            );
        }
    }

    /// Checks replica `n` after it handled something: its watermark only rose, and, if it is the
    /// hub and its log is settled, it left nothing it holds unnumbered.
    fn check_replica(&mut self, n: u8) {
        let replicator = &self.replicators[&n];
        let watermark = replicator.durable().clone();
        if let Some(before) = self.watermarks.get(&n) {
            for (origin, &position) in before {
                let now = watermark.get(origin).copied().unwrap_or(0);
                assert!(
                    now >= position,
                    "{n}'s watermark for {origin:?} fell from {position} to {now}"
                );
            }
        }
        self.watermarks.insert(n, watermark);
        if n == 1 && replicator.settled() {
            let unsequenced = self.models[&n].unsequenced();
            assert!(unsequenced.is_empty(), "the settled hub left {unsequenced:?} unnumbered");
        }
    }

    fn deliver(&mut self, from: u8, to: u8, frame: &[u8]) {
        if self.cut(from, to, self.now) {
            return;
        }
        let decoded = Frame::decode(frame).unwrap();
        if let Frame::Have(have) = &decoded {
            self.heard.insert((to, from));
            self.told.insert((to, from), have.vv.clone());
            // A `have` acknowledging the batch waiting ends the wait, and stalls its recipient
            // towards its sender if the sender took none of it.
            let acked = self.in_flight.get(&(to, from)).is_some_and(|w| w.batch == have.acked);
            if acked && let Some(waiting) = self.in_flight.remove(&(to, from)) {
                let took_some = waiting
                    .first
                    .iter()
                    .any(|(device, first)| have.vv.get(device).is_some_and(|held| held >= first));
                if !took_some {
                    self.stalled.insert((to, from), self.now);
                }
            }
        }
        let decline = matches!(decoded, Frame::Events(_))
            && self.now < HEAL
            && self.random(1000) < self.case.decline;
        let model = self.models.get_mut(&to).unwrap();
        model.decline_next = decline;
        let held =
            |model: &Model, origin: &Id<Device>| model.logs().get(origin).map_or(0, Vec::len);
        let before: VersionVector = model
            .logs()
            .keys()
            .map(|origin| (*origin, u64::try_from(held(model, origin)).unwrap()))
            .collect();
        let replicator = self.replicators.get_mut(&to).unwrap();
        let out = replicator.on_frame(model, device(from), frame, at(self.now)).unwrap();
        let answered = out.iter().any(|out| {
            out.to == device(from) && matches!(Frame::decode(&out.frame), Ok(Frame::Have(_)))
        });
        if let Frame::Have(have) = &decoded {
            assert!(answered || !have.asks, "{to} didn't answer {from}'s have, which asked");
        }
        if let Frame::Events(events) = decoded {
            // Acknowledged at once.
            let acknowledged = out.iter().any(|out| {
                out.to == device(from)
                    && matches!(Frame::decode(&out.frame), Ok(Frame::Have(have)) if have.acked == events.batch)
            });
            assert!(acknowledged, "{to} didn't acknowledge batch {} from {from}", events.batch);
            // The events it stored, `from` holds.
            let told = self.told.entry((to, from)).or_default();
            for bytes in &events.events {
                let body = SignedEvent::from_stored(bytes).unwrap().body().clone();
                let (origin, position) = (body.origin_device, body.origin_seq.get());
                let now_held = u64::try_from(held(model, &origin)).unwrap();
                let stored =
                    before.get(&origin).copied().unwrap_or(0) < position && position <= now_held;
                if stored {
                    let known = told.entry(origin).or_insert(0);
                    *known = (*known).max(position);
                }
            }
        }
        self.send(to, out);
        self.check_replica(to);
    }

    fn append(&mut self, replica: u8, count: u8) {
        let model = self.models.get_mut(&replica).unwrap();
        let events: Vec<SignedEvent> =
            (0..count).map(|k| model.append(u64::from(k), at(self.now))).collect();
        self.appended.extend(events.iter().cloned());
        let replicator = self.replicators.get_mut(&replica).unwrap();
        let out = replicator.appended(model, &events, at(self.now)).unwrap();
        self.send(replica, out);
        self.check_replica(replica);
    }

    /// Restarts `replica`: its replicator forgets everything, its events stay.
    fn restart(&mut self, replica: u8) {
        let model = self.models.get_mut(&replica).unwrap();
        let roles = roles_of(&self.case, &self.links, replica);
        let (replicator, out) = Replicator::start(
            model,
            peers_of(&self.links, replica),
            self.config,
            roles,
            at(self.now),
        )
        .unwrap();
        self.replicators.insert(replica, replicator);
        self.watermarks.remove(&replica);
        self.told.retain(|(r, _), _| *r != replica);
        self.in_flight.retain(|(r, _), _| *r != replica);
        self.stalled.retain(|(r, _), _| *r != replica);
        self.heard.retain(|(r, _)| *r != replica);
        self.send(replica, out);
    }

    /// Ticks every replica, checking that a tick does something only once `next_tick` said
    /// one was needed, and that a tick a round or more after a stall began is that round.
    fn tick(&mut self) {
        let now = self.now;
        // A batch unacknowledged in time is lost, as the replicator takes it at its tick.
        self.in_flight.retain(|_, waiting| now < waiting.sent + ACK_TIMEOUT);
        for n in 1..=self.case.replicas {
            let model = self.models.get_mut(&n).unwrap();
            let replicator = self.replicators.get_mut(&n).unwrap();
            let (due, timeouts) = (replicator.next_tick(), replicator.stats().timeouts);
            let out = replicator.on_tick(model, at(now)).unwrap();
            if !out.is_empty() || replicator.stats().timeouts != timeouts {
                assert!(
                    due.is_some_and(|due| due <= at(now)),
                    "replica {n} had work at {now} ms, before its next tick, {due:?}",
                );
            }
            // A `have` a tick sends begins a round with its peer, which ends a stall.
            let rounds: BTreeSet<u8> = out
                .iter()
                .filter(|out| matches!(Frame::decode(&out.frame), Ok(Frame::Have(_))))
                .map(|out| replica_of(out.to))
                .collect();
            for (&(replica, peer), &since) in &self.stalled {
                assert!(
                    replica != n || rounds.contains(&peer) || now < since + ROUND,
                    "{n} stalled towards {peer} at {since} ms, and ticked at {now} ms without a round",
                );
            }
            self.stalled.retain(|&(replica, peer), _| replica != n || !rounds.contains(&peer));
            self.send(n, out);
            self.check_replica(n);
        }
    }

    /// Whether every replica holds every event appended and every record of the hub, and every
    /// replica's watermark but the durable replica's reaches everything.
    fn agreed(&self) -> bool {
        let hub = &self.models[&1];
        let records = hub.logs().values().flatten().filter(|event| is_record(event)).count();
        let total = self.appended.len() + records;
        let holder = &self.models[&self.case.replicas];
        let everything =
            holder.logs().iter().map(|(origin, log)| (*origin, u64::try_from(log.len()).unwrap()));
        let everything: VersionVector = everything.collect();
        self.models
            .values()
            .all(|model| model.logs().values().map(Vec::len).sum::<usize>() == total)
            && self.replicators.iter().all(|(n, replicator)| {
                *n == self.case.replicas || replicator.durable() == &everything
            })
    }

    /// Runs the case until the replicas agree, or they fail to in time.
    fn run(&mut self) -> Result<i64, String> {
        let mut appends: Vec<(i64, u8, u8)> = self
            .case
            .appends
            .iter()
            .map(|&(t, replica, count)| (i64::from(t) * 100, replica, count))
            .collect();
        appends.sort_by_key(|&(t, _, _)| t);
        let mut restarts: Vec<(i64, u8)> =
            self.case.restarts.iter().map(|&(t, replica)| (i64::from(t) * 100, replica)).collect();
        restarts.sort_by_key(|&(t, _)| t);
        let (mut next_append, mut next_restart) = (0, 0);
        while self.now <= HEAL + AGREE_WITHIN {
            while let Some(&(t, replica, count)) = appends.get(next_append)
                && t <= self.now
            {
                self.append(replica, count);
                next_append += 1;
            }
            while let Some(&(t, replica)) = restarts.get(next_restart)
                && t <= self.now
            {
                self.restart(replica);
                next_restart += 1;
            }
            let due: Vec<(i64, u64)> =
                self.queue.range(..=(self.now, u64::MAX)).map(|(k, _)| *k).collect();
            for key in due {
                let (from, to, frame) = self.queue.remove(&key).unwrap();
                self.deliver(from, to, &frame);
            }
            if self.now % 100 == 0 {
                self.tick();
            }
            let settled = self.replicators.values().all(Replicator::settled);
            if self.now > HEAL && next_append == appends.len() && self.agreed() && settled {
                return Ok(self.now);
            }
            // On to the next thing to happen: a frame arriving, a tick, an append or a restart.
            let next_tick = (self.now / 100 + 1) * 100;
            let next_frame = self.queue.keys().next().map(|&(t, _)| t);
            let next_append_at = appends.get(next_append).map(|&(t, _, _)| t);
            let next_restart_at = restarts.get(next_restart).map(|&(t, _)| t);
            self.now = [next_frame, next_append_at, next_restart_at]
                .into_iter()
                .flatten()
                .fold(next_tick, i64::min)
                .max(self.now + 1);
        }
        Err(format!("no agreement, or an unsettled log, {AGREE_WITHIN} ms after healing"))
    }
}

proptest! {
    /// Replicas exchanging frames over a faulty network converge to exactly the events appended.
    #[test]
    fn replicas_converge_over_a_faulty_network(case in any_case()) {
        let mut world = World::new(case);
        let agreed = world.run();
        prop_assert!(agreed.is_ok(), "{agreed:?}");
        // Every replica holds exactly the events appended, each device's in order.
        let mut expected: BTreeMap<Id<Device>, Vec<SignedEvent>> = BTreeMap::new();
        for event in &world.appended {
            expected.entry(event.body().origin_device).or_default().push(event.clone());
        }
        // And the hub's records, which number every other event once, gapless.
        let records: Vec<SignedEvent> = world.models[&1].logs()[&device(1)].iter().filter(|event| is_record(event)).cloned().collect();
        for record in &records {
            expected.entry(device(1)).or_default().push(record.clone());
        }
        for log in expected.values_mut() {
            log.sort_by_key(|event| event.body().origin_seq);
        }
        let mut numbered: BTreeMap<(Id<Device>, u64), u64> = BTreeMap::new();
        for (_, record) in world.models[&1].records() {
            prop_assert_eq!(record.epoch, 1);
            for (number, run) in record.numbered() {
                for position in run.from..=run.to {
                    let again = numbered.insert((run.device, position), number + (position - run.from));
                    prop_assert!(again.is_none(), "{:?} {} numbered twice", run.device, position);
                }
            }
        }
        let others: BTreeSet<(Id<Device>, u64)> = expected.values().flatten().filter(|event| !is_record(event)).map(|event| (event.body().origin_device, event.body().origin_seq.get())).collect();
        prop_assert_eq!(numbered.keys().copied().collect::<BTreeSet<_>>(), others);
        let numbers: BTreeSet<u64> = numbered.values().copied().collect();
        prop_assert_eq!(numbers, (1..=u64::try_from(numbered.len()).unwrap()).collect());
        for (n, model) in &mut world.models {
            prop_assert_eq!(model.logs(), &expected, "replica {}", n);
            prop_assert!(model.quarantine.is_empty(), "replica {} refused {:?}", n, model.quarantine);
            // Its replicator knows what it holds.
            let held = model.version_vector().unwrap();
            prop_assert_eq!(world.replicators[n].version_vector(), &held, "replica {}", n);
        }
        // And every replica's own log is settled.
        let unsettled: BTreeSet<u8> = world.replicators.iter().filter(|(_, r)| !r.settled()).map(|(n, _)| *n).collect();
        prop_assert!(unsettled.is_empty(), "{unsettled:?}");
    }
}
