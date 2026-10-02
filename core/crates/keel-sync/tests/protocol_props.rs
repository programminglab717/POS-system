//! Property tests for the replication protocol (ADR-0019, ADR-0022), against the model replica.
//!
//! A case is a few replicas, linked as a star, a line or a mesh, appending events at random
//! times, over a network that delays every frame, loses and duplicates some, and cuts links for
//! a while. Replicas crash now and then, losing what their replicator knew but none of their
//! events, some starting again at once and some after seconds down, a third of them with their
//! clock set back, by up to half a minute; and at times decline a batch, as a replica that can't
//! take it for now would, so that its sender stalls. Their clocks jump forwards now and then, by
//! up to half a minute. After twenty seconds the faults stop. Every replica but the last may be
//! the Store Hub, with a priority of its own, and the first always may; the last is the durable
//! one, which the replicas linked to it name as their durable peer (ADR-0020). Heartbeats come
//! every second, as configured, or in a third of the cases at another period, from half a second
//! to a second and a half, so that they fall due apart from the rounds. All along, the protocol's
//! rules must hold, checked frame by frame:
//!
//! - no replica sends a peer an event the peer last told it it holds, or sent it since;
//! - a batch holds at most the events and bytes configured, unless its one event is more bytes;
//! - a replica sends a peer a batch only once the last is acknowledged or its acknowledgement
//!   timeout has passed, and numbers its batches to each peer in rising order;
//! - a replica takes a `have` for the acknowledgement of its last batch only if it acknowledges
//!   that very batch, and not one of the same number sent before the replica restarted, its
//!   clock set back;
//! - a replica whose last batch a peer took none of sends the peer nothing more until its next
//!   round, and a tick a round or more after the stall began is that round;
//! - a replica acknowledges each batch it receives at once;
//! - a replica's `have` asks for the peer's in return exactly until it has heard from the peer
//!   since it started, and a replica answers a `have` that asks at once;
//! - a replica's ticks do nothing before [`Replicator::next_tick`] says they are needed;
//! - a `durable` frame claims no more of any device's log than the durable replica holds, and a
//!   replicator's watermark only rises;
//! - a replica's own log settles exactly as the rule says: once some peer has said what it holds
//!   since the replica started, every peer that has holds no more of the log than the replica,
//!   and either all have said so or a heartbeat period has begun since the first did;
//! - a heartbeat gives its sender's priority, 0 while its log is unsettled or forked, and its
//!   term, the epoch and hub of the winning claim it holds; says it acts as the hub exactly while
//!   it serves; gives the hub's beat: its own, which rises every period it serves, and past
//!   every floor naming it as the hub that a peer gave it, or else the latest it had directly
//!   from its term's hub, if that is recent; and gives the term's floor, the latest beat of it it
//!   knows, however old, from beats and floors;
//! - a replica hears its hub exactly as the rule says: while it serves, or the latest beat it
//!   knows of its term's hub, or of a hub of a later epoch, came recently, from the hub or from a
//!   peer that had it directly, or it learned of the winning claim it holds as recently. Beats of
//!   two hubs of one epoch, which a split leaves, are never compared;
//! - a replica claims the hub's role only as a heartbeat period begins, settled, having listened
//!   for the periods of silence since it started and since it learned of the winning claim it
//!   held, hearing no hub and no recent heartbeat from a peer that would be preferred as hub; and
//!   it claims the next epoch, at its own priority;
//! - a replica writes records only while it serves, in the epoch of the term it holds, and leaves
//!   nothing it holds unnumbered.
//!
//! Within a minute of healing, the replicas must agree, and go on agreeing with nothing more
//! written: every replica holds exactly every event appended and every claim and record written;
//! all hold the same winning claim, whose device alone serves as the hub; the replicas within two
//! hops of the hub hear it, and no others; the chain of terms is whole; every event but the
//! records is numbered exactly once by the records that count, each epoch's numbers gapless from
//! 1 and each device's log in order; and every replica's watermark but the durable replica's
//! reaches everything.

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

use keel_domain::hub::Term;
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, SequenceEvent};
use keel_events::envelope::Device;
use keel_events::event::SignedEvent;
use keel_sync::{
    Durable, Frame, Heartbeat, Outgoing, Replica, Replicator, Roles, SyncConfig, VersionVector,
};
use keel_types::{Entropy, Id, SeededEntropy};
use proptest::prelude::*;
use support::{Model, at, device, here, is_claim, is_record, registry};

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
    /// When, which replica crashes, for how long it stays down (in tenths of a second): 0 to
    /// start again at once; and how far its clock is set back when it starts again.
    crashes: Vec<(u16, u8, u16, u16)>,
    /// When, whose clock jumps forwards, and how far (in tenths of a second).
    jumps: Vec<(u16, u8, u16)>,
    /// Each replica's priority as hub, `None` if it can never be.
    priorities: Vec<Option<u8>>,
    /// The most events and bytes in a batch.
    batch_events: u32,
    batch_bytes: usize,
    /// The heartbeat period, in milliseconds: mostly a second, as configured, and otherwise from
    /// half a second to a second and a half, so that rounds and heartbeats fall due apart.
    heartbeat: i64,
    seed: u64,
}

/// Each replica's priority as hub: the first's from 1 to 3, each other's but the last's the
/// same or none, and the last's, the durable replica's, none.
fn any_priorities(replicas: u8) -> impl Strategy<Value = Vec<Option<u8>>> {
    let others = usize::from(replicas - 2);
    let other = prop_oneof![1 => Just(None), 2 => (1_u8..=3).prop_map(Some)];
    (1_u8..=3, prop::collection::vec(other, others)).prop_map(|(first, others)| {
        let mut priorities = vec![Some(first)];
        priorities.extend(others);
        priorities.push(None);
        priorities
    })
}

fn any_case() -> impl Strategy<Value = Case> {
    (2_u8..=4).prop_flat_map(|replicas| {
        (
            (Just(replicas), any_priorities(replicas)),
            prop_oneof![Just(Topology::Star), Just(Topology::Line), Just(Topology::Mesh)],
            (
                prop_oneof![1 => Just(0_u64), 3 => 0_u64..300],
                prop_oneof![1 => Just(0_u64), 3 => 0_u64..200],
                prop_oneof![1 => Just(0_u64), 3 => 0_u64..300],
            ),
            1_u64..300,
            prop::collection::vec((0_u16..200, 1..=replicas, 1_u8..4), 1..12),
            prop::collection::vec((any::<u8>(), 0_u16..200, 1_u16..80), 0..4),
            (
                // Down long enough for a failover two times in three, and starting again with
                // its clock set back, up to half a minute, one time in three.
                prop::collection::vec(
                    (
                        0_u16..200,
                        1..=replicas,
                        prop_oneof![1 => Just(0_u16), 2 => 1_u16..120],
                        prop_oneof![2 => Just(0_u16), 1 => 1_u16..=300],
                    ),
                    0..5,
                ),
                // Up to half a minute, past the periods of silence.
                prop::collection::vec((0_u16..200, 1..=replicas, 1_u16..=300), 0..3),
            ),
            // A few events a batch, and at times too few bytes for more than one or two: an
            // event is some 300 bytes.
            (1_u32..=6, prop_oneof![Just(256 * 1024), 1_usize..1200]),
            prop_oneof![2 => Just(HEARTBEAT), 1 => (5_i64..=15).prop_map(|tenths| tenths * 100)],
            any::<u64>(),
        )
            .prop_map(
                |(
                    (replicas, priorities),
                    topology,
                    (loss, duplication, decline),
                    delay,
                    appends,
                    cuts,
                    (crashes, jumps),
                    (batch_events, batch_bytes),
                    heartbeat,
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
                    crashes,
                    jumps,
                    priorities,
                    batch_events,
                    batch_bytes,
                    heartbeat,
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

/// A replica's priority as hub, 0 if it can never be.
fn priority_of(case: &Case, replica: u8) -> u8 {
    case.priorities[usize::from(replica - 1)].unwrap_or(0)
}

/// Each replica has its priority as hub; the last replica is the durable one, and each replica
/// linked to it names it its durable peer.
fn roles_of(case: &Case, links: &[(u8, u8)], replica: u8) -> Roles {
    let durable = case.replicas;
    let linked = links.iter().any(|&link| link == (replica, durable) || link == (durable, replica));
    Roles {
        hub: NonZeroU8::new(priority_of(case, replica)),
        durable: (linked && replica != durable).then(|| device(durable)),
    }
}

const ACK_TIMEOUT: i64 = 2_000;
const ROUND: i64 = 5_000;
/// The heartbeat period most cases have, as configured.
const HEARTBEAT: i64 = 1_000;
const SILENCE: u64 = 3;

fn config(case: &Case) -> SyncConfig {
    SyncConfig {
        round: Duration::from_millis(ROUND.unsigned_abs()),
        ack_timeout: Duration::from_millis(ACK_TIMEOUT.unsigned_abs()),
        batch_events: case.batch_events,
        batch_bytes: case.batch_bytes,
        heartbeat: Duration::from_millis(case.heartbeat.unsigned_abs()),
        silence: u32::try_from(SILENCE).unwrap(),
    }
}

/// Faults stop, and the network heals, after this many milliseconds; replicas down come back up
/// by then.
const HEAL: i64 = 20_000;
/// The replicas must agree this long after healing.
const AGREE_WITHIN: i64 = 60_000;
/// Agreement counts only this long after healing, and after the hub began to serve: the periods
/// of silence and two more, so that whatever a replica holds recent of the hub came from it since.
const SETTLE: i64 = 5_000;
/// And must last this long, with nothing more written.
const STABLE: i64 = 5_000;

struct World {
    case: Case,
    config: SyncConfig,
    links: Vec<(u8, u8)>,
    models: BTreeMap<u8, Model>,
    replicators: BTreeMap<u8, Replicator>,
    /// Frames in flight: when they arrive, in what order, and the frame.
    queue: BTreeMap<(i64, u64), InFlightFrame>,
    sent: u64,
    rng: SeededEntropy,
    /// The random bits each start of a replicator numbers its batches from: apart from `rng`,
    /// so that drawing them changes nothing else of a case.
    nonces: SeededEntropy,
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
    /// The number of the last batch each replica sent each peer, since it last started.
    numbered: BTreeMap<(u8, u8), u64>,
    /// How many times each replica has started again.
    starts: BTreeMap<u8, u64>,
    /// The last batch each replica received from each peer, since it last started: its number,
    /// and which start of the peer's sent it. (replica, peer) → (number, start).
    received: BTreeMap<(u8, u8), (u64, u64)>,
    /// Each replica and a peer it has had a `have` from since it last started.
    heard: BTreeSet<(u8, u8)>,
    /// Each replicator's watermark, as last seen, since it last started.
    watermarks: BTreeMap<u8, VersionVector>,
    appended: Vec<SignedEvent>,
    /// The replicas down, and when each comes back up.
    down: BTreeMap<u8, i64>,
    /// The heartbeat periods each replica has begun since it last started.
    periods: BTreeMap<u8, u64>,
    /// What each replica has had from each peer since it last started. (replica, peer) → beats.
    beats: BTreeMap<(u8, u8), Beats>,
    /// How much of its own log each replica held when last checked.
    own: BTreeMap<u8, usize>,
    /// The winning claim's term each replicator held when last checked.
    terms: BTreeMap<u8, Option<Term>>,
    /// Since when each replica has held that term, or since it last started, if later.
    holding_since: BTreeMap<u8, i64>,
    /// The latest beat of each hub's term each replica knows of, since it last started, from the
    /// hub or from a peer that had it directly. (replica, epoch, hub) → beat.
    latest: BTreeMap<(u8, u64, Id<Device>), Heard>,
    /// Each term's floor each replica knows, since it last started: the latest beat of the term
    /// it heard, or heard of in a floor. (replica, epoch, hub) → beat.
    floors: BTreeMap<(u8, u64, Id<Device>), u64>,
    /// The beat each replica last gave acting as the hub, or the latest floor of a term it is
    /// the hub of that a peer gave it if that is later, since it last started: its next beat goes
    /// on above it.
    own_beat: BTreeMap<u8, u64>,
    /// When each replica serving as the hub began to, since it last did not.
    serving_since: BTreeMap<u8, i64>,
    /// The first position of each replica's own log each peer refused, since the replica last
    /// started: it took none of a batch of that log that began just after what it held of it, and
    /// hasn't since said it holds the log that far. (replica, peer) → position.
    refused: BTreeMap<(u8, u8), u64>,
    /// The period in which each replica learned of the winning claim it holds, another
    /// replica's, since it last started.
    learned: BTreeMap<u8, u64>,
    /// The period in which each replica first had a `have`, since it last started.
    first_have: BTreeMap<u8, u64>,
    /// The replicas whose own log has settled, by the rule, since they last started.
    settled: BTreeSet<u8>,
    /// How far each replica's clock is ahead of the world's time, in milliseconds: behind it if
    /// negative.
    ahead: BTreeMap<u8, i64>,
}

/// What a replica has had from a peer: its last heartbeat, with the period it came in; and its
/// latest beat acting as the hub, with the epoch of its term.
#[derive(Clone, Copy, Default)]
struct Beats {
    last: Option<(u64, Heartbeat)>,
    acting: Option<(u64, Heard)>,
}

/// A frame in flight: from, to, the frame, and for a batch its sender's start, and for a `have`
/// the start of the sender of the batch it acknowledges.
type InFlightFrame = (u8, u8, Vec<u8>, u64);

/// A beat of a hub's term, and the period it first came in.
type Heard = (u64, u64);

/// A hub's term, as a heartbeat names it: its epoch and the hub's device.
type HubTerm = (u64, Id<Device>);

/// The later of `kept` and `beat`, of one term, which came in `period`.
fn later(kept: Option<Heard>, beat: u64, period: u64) -> Heard {
    match kept {
        Some(kept) if kept.0 >= beat => kept,
        _ => (beat, period),
    }
}

/// A batch sent and not yet acknowledged: its number, when it was sent, and the first position of
/// each device's events in it.
struct InFlight {
    batch: u64,
    /// When it was sent, by its sender's clock.
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
            nonces: SeededEntropy::new(!case.seed),
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
            starts: BTreeMap::new(),
            received: BTreeMap::new(),
            heard: BTreeSet::new(),
            watermarks: BTreeMap::new(),
            appended: Vec::new(),
            down: BTreeMap::new(),
            periods: BTreeMap::new(),
            beats: BTreeMap::new(),
            own: BTreeMap::new(),
            terms: BTreeMap::new(),
            holding_since: BTreeMap::new(),
            learned: BTreeMap::new(),
            refused: BTreeMap::new(),
            latest: BTreeMap::new(),
            floors: BTreeMap::new(),
            own_beat: BTreeMap::new(),
            serving_since: BTreeMap::new(),
            first_have: BTreeMap::new(),
            settled: BTreeSet::new(),
            ahead: BTreeMap::new(),
        };
        let mut starting = Vec::new();
        for n in 1..=world.case.replicas {
            world.periods.insert(n, 0);
            world.terms.insert(n, None);
            world.holding_since.insert(n, 0);
            let mut model = Model::new(n, registry(1..=world.case.replicas));
            let roles = roles_of(&world.case, &world.links, n);
            let nonce = world.nonces.next_u64().unwrap();
            let (replicator, out) = Replicator::start(
                &mut model,
                peers_of(&world.links, n),
                world.config,
                roles,
                nonce,
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
            let mut start = 0;
            match Frame::decode(&frame).unwrap() {
                Frame::Events(events) => {
                    self.check_batch(from, to, events.batch, &events.events);
                    start = self.starts.get(&from).copied().unwrap_or(0);
                }
                Frame::Have(have) => {
                    let heard = self.heard.contains(&(from, to));
                    assert_eq!(have.asks, !heard, "{from}'s have to {to}, having heard: {heard}");
                    // It acknowledges the last batch `from` received from `to`.
                    let (batch, of) = self.received.get(&(from, to)).copied().unwrap_or((0, 0));
                    assert_eq!(have.acked, batch, "{from}'s have to {to}");
                    start = of;
                }
                Frame::Durable(durable) => self.check_durable(from, to, &durable),
                Frame::Heartbeat(heartbeat) => self.check_heartbeat(from, &heartbeat),
            }
            let faulty = self.now < HEAL;
            let copies = if faulty && self.random(1000) < self.case.duplication { 2 } else { 1 };
            for _ in 0..copies {
                if faulty && self.random(1000) < self.case.loss {
                    continue;
                }
                let delay = i64::try_from(1 + self.random(self.case.delay)).unwrap();
                self.sent += 1;
                let sent = (from, to, frame.clone(), start);
                self.queue.insert((self.now + delay, self.sent), sent);
            }
        }
    }

    /// Checks a batch `from` sends `to`: it is no larger than configured, none of its events is
    /// one `to` holds as far as `from` may know, the last batch is acknowledged or its
    /// acknowledgement timeout has passed, `from` isn't stalled towards `to`, and the batch's
    /// number is higher than any it sent `to` since it started.
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
        let sent = self.clock(from);
        self.in_flight.insert((from, to), InFlight { batch, sent, first });
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

    /// What replica `n` has had from each peer.
    fn beats(&self, n: u8) -> impl Iterator<Item = (u8, &Beats)> {
        self.beats.iter().filter(move |((to, _), _)| *to == n).map(|((_, from), b)| (*from, b))
    }

    /// Whether something replica `n` heard in `period` is recent: it came in its current period
    /// or the periods of silence before it.
    fn recent(&self, n: u8, period: u64) -> bool {
        self.periods[&n] - period <= SILENCE
    }

    /// The term of the winning claim replica `n` holds, as its heartbeats name it.
    fn hub_term(&self, n: u8) -> Option<HubTerm> {
        self.replicators[&n].term().map(|term| (term.epoch.get(), term.device))
    }

    /// The latest beat replica `n` had directly from the hub of its own term, if it is recent:
    /// the beat it passes on.
    fn beat_heard_directly(&self, n: u8) -> Option<u64> {
        let (epoch, hub) = self.hub_term(n)?;
        let (of, (beat, period)) = self.beats.get(&(n, replica_of(hub)))?.acting?;
        (of == epoch && self.recent(n, period)).then_some(beat)
    }

    /// Whether replica `n`, holding the winning claim of `term`, hears its hub by the rule: it
    /// learned of that claim recently, or the latest beat it knows of the term's hub, or of the
    /// hub of a later epoch, first reached it recently.
    fn hears_hub(&self, n: u8, term: Option<HubTerm>) -> bool {
        self.learned.get(&n).is_some_and(|&learned| self.recent(n, learned))
            || self.latest.iter().any(|(&(r, epoch, hub), &(_, period))| {
                r == n
                    && self.recent(n, period)
                    && term.is_none_or(|(own, of)| epoch > own || (epoch == own && hub == of))
            })
    }

    /// Whether replica `n`'s log forked: a peer refused its own log.
    fn forked(&self, n: u8) -> bool {
        self.refused.keys().any(|(r, _)| *r == n)
    }

    /// Whether replica `n` serves as the hub: it holds the winning claim, as its replicator last
    /// read its store, and its log is settled and not forked.
    fn serving(&self, n: u8) -> bool {
        let replicator = &self.replicators[&n];
        let hub = replicator.term().is_some_and(|term| term.device == device(n));
        let serving = hub && self.settled.contains(&n) && !self.forked(n);
        assert_eq!(replicator.serving(), serving, "{n} serving");
        serving
    }

    /// Checks a heartbeat `from` sends: it gives its priority while its log is settled and not
    /// forked, and 0 otherwise, and its term, the epoch and hub of the winning claim it holds; it
    /// says `from` acts as the hub exactly while it serves; it gives the hub's beat, its own as
    /// the hub, raised as each period began, else the latest it had directly from its term's hub,
    /// if recent; and the term's floor, the latest beat of the term it knows, however old.
    fn check_heartbeat(&self, from: u8, heartbeat: &Heartbeat) {
        let replicator = &self.replicators[&from];
        assert_eq!(replicator.forked(), self.forked(from), "{from} forked");
        let term = self.hub_term(from);
        let acting = self.serving(from);
        let beat =
            if acting { self.own_beat.get(&from).copied() } else { self.beat_heard_directly(from) };
        let known = term.and_then(|(epoch, hub)| self.floors.get(&(from, epoch, hub)).copied());
        let floor = known.max(beat);
        let eligible = self.settled.contains(&from) && !self.forked(from);
        let priority = if eligible { priority_of(&self.case, from) } else { 0 };
        let (epoch, hub) = term.map_or((0, None), |(epoch, hub)| (epoch, Some(hub)));
        let expected = Heartbeat { location: here(), priority, epoch, hub, acting, beat, floor };
        assert_eq!(heartbeat, &expected, "{from}'s heartbeat at {} ms", self.now);
    }

    /// Checks a claim replica `n` made as a heartbeat period began, holding the claim of `held`:
    /// it was settled and not forked; it had listened for the periods of silence since it started
    /// and since it learned of the winning claim it held; it heard no hub; and no peer that would
    /// be preferred as hub had said so recently. And it claimed the next epoch, at its own
    /// priority.
    fn check_claim(&self, n: u8, held: Option<HubTerm>) {
        let before = held.map_or(0, |(epoch, _)| epoch);
        let replicator = &self.replicators[&n];
        let period = self.periods[&n];
        assert!(self.settled.contains(&n), "{n} claimed at {} ms, unsettled", self.now);
        assert!(!self.forked(n), "{n} claimed at {} ms, forked", self.now);
        assert!(period >= SILENCE, "{n} claimed in period {period} since it started");
        let learned = self.learned.get(&n);
        assert!(
            learned.is_none_or(|learned| period - learned > SILENCE),
            "{n} claimed in period {period}, having learned of a claim in period {learned:?}",
        );
        assert!(
            !self.hears_hub(n, held),
            "{n} claimed at {} ms, holding {held:?}, having heard its hub recently",
            self.now,
        );
        let priority = priority_of(&self.case, n);
        for (from, beats) in self.beats(n) {
            let Some((heard, last)) = beats.last else { continue };
            let preferred =
                last.priority > priority || (last.priority == priority && device(from) < device(n));
            assert!(
                period - heard > SILENCE || !preferred,
                "{n} claimed at {} ms, hearing {from}: {last:?}",
                self.now,
            );
        }
        let term = replicator.term().unwrap();
        assert_eq!((term.device, term.epoch.get()), (device(n), before + 1), "{n}'s claim");
        let claims = self.models[&n].claims();
        let claim = claims.iter().find(|claim| claim.event == term.claim).unwrap();
        assert_eq!(claim.claimed.priority.get(), priority, "{n}'s claim");
    }

    /// Replica `n`'s clock: the world's time, and as far ahead or behind as its jumps, and its
    /// clock set back as it crashed, have taken it.
    fn clock(&self, n: u8) -> i64 {
        self.now + self.ahead.get(&n).copied().unwrap_or(0)
    }

    /// Settles replica `n`'s own log by the rule, as the replicator does on a `have`, a batch,
    /// and a heartbeat period beginning (ADR-0022, decision 7): once some peer has said what it
    /// holds since the replica started, and every peer that has holds no more of the replica's
    /// log than the replica does, and either every peer has said so or a period has begun since
    /// the first did. It stays settled until the replica restarts.
    fn settle(&mut self, n: u8) {
        let own = self.models[&n].logs().get(&device(n)).map_or(0, Vec::len);
        let own = u64::try_from(own).unwrap();
        let peers: Vec<u8> = peers_of(&self.links, n).into_iter().map(replica_of).collect();
        let heard: Vec<u8> =
            peers.iter().copied().filter(|&peer| self.heard.contains(&(n, peer))).collect();
        let holds_more = heard
            .iter()
            .any(|&peer| self.told[&(n, peer)].get(&device(n)).is_some_and(|&held| held > own));
        let waited = self.first_have.get(&n).is_some_and(|&first| self.periods[&n] > first);
        if !heard.is_empty() && !holds_more && (heard.len() == peers.len() || waited) {
            self.settled.insert(n);
        }
    }

    /// Checks replica `n` after it handled something: its log is settled exactly as the rule
    /// says; its watermark only rose; it wrote records only while it serves, in the epoch of the
    /// term it holds; and, while it serves, it left nothing it holds unnumbered.
    fn check_replica(&mut self, n: u8) {
        let replicator = &self.replicators[&n];
        assert_eq!(
            replicator.settled(),
            self.settled.contains(&n),
            "{n}'s log settled, or didn't, against the rule, at {} ms",
            self.now
        );
        let term = replicator.term().copied();
        let changed = self.terms.insert(n, term) != Some(term);
        if changed {
            self.holding_since.insert(n, self.now);
        }
        if changed
            && let Some(term) = term
            && term.device != device(n)
        {
            self.learned.insert(n, self.periods[&n]);
        }
        assert_eq!(
            replicator.hub_reachable(),
            self.serving(n) || self.hears_hub(n, self.hub_term(n)),
            "{n} hears its hub, or doesn't, against the rule, at {} ms",
            self.now
        );
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
        let model = &self.models[&n];
        let log = model.logs().get(&device(n)).map_or(&[][..], Vec::as_slice);
        let checked = self.own.insert(n, log.len()).unwrap_or(0);
        for event in &log[checked..] {
            if is_record(event) {
                assert!(self.serving(n), "{n} wrote a record, not serving");
                let epoch = record_of(event).epoch;
                assert_eq!(epoch, epoch_of(replicator), "{n} wrote a record of another epoch");
            }
        }
        if self.serving(n) {
            let unsequenced = model.unsequenced();
            assert!(unsequenced.is_empty(), "the hub, {n}, left {unsequenced:?} unnumbered");
            self.serving_since.entry(n).or_insert(self.now);
        } else {
            self.serving_since.remove(&n);
        }
    }

    /// Notes what replica `to` makes of `heartbeat`, from `from`, as its replicator should: the
    /// heartbeat, with the period it came in; a beat, compared only with others of its hub's term,
    /// and none from a heartbeat acting for a hub other than its sender; and the term's floor,
    /// raising `to`'s own beats if `to` is the term's hub.
    fn hear(&mut self, from: u8, to: u8, heartbeat: &Heartbeat) {
        let period = self.periods[&to];
        let beats = self.beats.entry((to, from)).or_default();
        beats.last = Some((period, *heartbeat));
        let Some(hub) = heartbeat.hub.filter(|&hub| !heartbeat.acting || hub == device(from))
        else {
            return;
        };
        let epoch = heartbeat.epoch;
        if let Some(known) = heartbeat.floor.max(heartbeat.beat) {
            let floor = self.floors.entry((to, epoch, hub)).or_insert(known);
            *floor = (*floor).max(known);
            // A floor of a term `to` is the hub of, which a peer gives it: its beats go on above
            // it.
            if !heartbeat.acting && hub == device(to) {
                let floor = self.own_beat.entry(to).or_insert(0);
                *floor = (*floor).max(heartbeat.floor.unwrap_or(0));
            }
        }
        let Some(beat) = heartbeat.beat else { return };
        if heartbeat.acting {
            beats.acting = Some(match beats.acting {
                Some((of, heard)) if of > epoch => (of, heard),
                Some((of, heard)) if of == epoch => (of, later(Some(heard), beat, period)),
                _ => (epoch, (beat, period)),
            });
        }
        let key = (to, epoch, hub);
        let heard = later(self.latest.get(&key).copied(), beat, period);
        self.latest.insert(key, heard);
    }

    /// Delivers `frame` from `from` to `to`, tagged with `start` as [`World::queue`] says.
    fn deliver(&mut self, from: u8, to: u8, frame: &[u8], start: u64) {
        if self.cut(from, to, self.now) || self.down.contains_key(&to) {
            return;
        }
        let clock = self.clock(to);
        let decoded = Frame::decode(frame).unwrap();
        // A `have` and a batch are what a replica settles its log on, besides a period beginning.
        let settling = matches!(decoded, Frame::Have(_) | Frame::Events(_));
        if let Frame::Heartbeat(heartbeat) = &decoded {
            self.hear(from, to, heartbeat);
        }
        if let Frame::Have(have) = &decoded {
            self.heard.insert((to, from));
            self.told.insert((to, from), have.vv.clone());
            self.first_have.entry(to).or_insert(self.periods[&to]);
            // A `have` acknowledging the batch waiting ends the wait, and stalls its recipient
            // towards its sender if the sender took none of it.
            let acked = self.in_flight.get(&(to, from)).is_some_and(|w| w.batch == have.acked);
            // That is the batch it acknowledges, even if `to` restarted since it sent one of that
            // number, its clock set back.
            let starts = self.starts.get(&to).copied().unwrap_or(0);
            assert!(
                !acked || start == starts,
                "{to} took {from}'s acknowledgement of batch {} from before it restarted",
                have.acked,
            );
            if acked && let Some(waiting) = self.in_flight.remove(&(to, from)) {
                let took_some = waiting
                    .first
                    .iter()
                    .any(|(device, first)| have.vv.get(device).is_some_and(|held| held >= first));
                if !took_some {
                    self.stalled.insert((to, from), self.now);
                }
                let held = have.vv.get(&device(to)).copied().unwrap_or(0);
                if let Some(&first) = waiting.first.get(&device(to))
                    && held + 1 == first
                {
                    self.refused.insert((to, from), first);
                }
            }
            let held = have.vv.get(&device(to)).copied().unwrap_or(0);
            if self.refused.get(&(to, from)).is_some_and(|&refused| held >= refused) {
                self.refused.remove(&(to, from));
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
        let claims = replicator.stats().claims;
        let out = replicator.on_frame(model, device(from), frame, at(clock)).unwrap();
        assert_eq!(replicator.stats().claims, claims, "{to} claimed on a frame");
        let answered = out.iter().any(|out| {
            out.to == device(from) && matches!(Frame::decode(&out.frame), Ok(Frame::Have(_)))
        });
        if let Frame::Have(have) = &decoded {
            assert!(answered || !have.asks, "{to} didn't answer {from}'s have, which asked");
        }
        if let Frame::Events(events) = decoded {
            self.received.insert((to, from), (events.batch, start));
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
        if settling {
            self.settle(to);
        }
        self.send(to, out);
        self.check_replica(to);
    }

    /// Appends `count` events at `replica`, unless it is down.
    fn append(&mut self, replica: u8, count: u8) {
        if self.down.contains_key(&replica) {
            return;
        }
        let clock = self.clock(replica);
        let model = self.models.get_mut(&replica).unwrap();
        let events: Vec<SignedEvent> =
            (0..count).map(|k| model.append(u64::from(k), at(clock))).collect();
        self.appended.extend(events.iter().cloned());
        let replicator = self.replicators.get_mut(&replica).unwrap();
        let claims = replicator.stats().claims;
        let out = replicator.appended(model, &events, at(clock)).unwrap();
        assert_eq!(replicator.stats().claims, claims, "{replica} claimed on appending");
        self.send(replica, out);
        self.check_replica(replica);
    }

    /// Crashes `replica` for `down` tenths of a second, but no later than healing: it starts
    /// again at once if 0, its clock set back `back` milliseconds. A replica down stays down.
    fn crash(&mut self, replica: u8, down: u16, back: i64) {
        if self.down.contains_key(&replica) {
            return;
        }
        *self.ahead.entry(replica).or_insert(0) -= back;
        if down == 0 {
            self.restart(replica);
        } else {
            let up = (self.now + i64::from(down) * 100).min(HEAL);
            self.down.insert(replica, up);
        }
    }

    /// Restarts `replica`: its replicator forgets everything, its events stay.
    fn restart(&mut self, replica: u8) {
        let clock = self.clock(replica);
        let nonce = self.nonces.next_u64().unwrap();
        let model = self.models.get_mut(&replica).unwrap();
        let roles = roles_of(&self.case, &self.links, replica);
        let peers = peers_of(&self.links, replica);
        let (replicator, out) =
            Replicator::start(model, peers, self.config, roles, nonce, at(clock)).unwrap();
        self.replicators.insert(replica, replicator);
        self.down.remove(&replica);
        self.watermarks.remove(&replica);
        self.told.retain(|(r, _), _| *r != replica);
        self.in_flight.retain(|(r, _), _| *r != replica);
        self.stalled.retain(|(r, _), _| *r != replica);
        self.numbered.retain(|(r, _), _| *r != replica);
        self.received.retain(|(r, _), _| *r != replica);
        *self.starts.entry(replica).or_insert(0) += 1;
        self.heard.retain(|(r, _)| *r != replica);
        self.periods.insert(replica, 0);
        self.beats.retain(|(r, _), _| *r != replica);
        self.latest.retain(|(r, _, _), _| *r != replica);
        self.floors.retain(|(r, _, _), _| *r != replica);
        self.own_beat.remove(&replica);
        self.serving_since.remove(&replica);
        self.terms.insert(replica, self.replicators[&replica].term().copied());
        self.holding_since.insert(replica, self.now);
        self.learned.remove(&replica);
        self.refused.retain(|(r, _), _| *r != replica);
        self.first_have.remove(&replica);
        self.settled.remove(&replica);
        self.send(replica, out);
    }

    /// Ticks every replica up, checking that a tick does something only once `next_tick` said
    /// one was needed, that a tick a round or more after a stall began is that round, and that
    /// a claim is made as the protocol says.
    fn tick(&mut self) {
        let now = self.now;
        let ahead = self.ahead.clone();
        let clock = |n: u8| now + ahead.get(&n).copied().unwrap_or(0);
        // A batch unacknowledged in time, by its sender's clock, is lost, as the replicator takes
        // it at its tick.
        self.in_flight.retain(|&(from, _), waiting| clock(from) < waiting.sent + ACK_TIMEOUT);
        for n in 1..=self.case.replicas {
            if self.down.contains_key(&n) {
                continue;
            }
            let model = self.models.get_mut(&n).unwrap();
            let replicator = self.replicators.get_mut(&n).unwrap();
            let (due, timeouts) = (replicator.next_tick(), replicator.stats().timeouts);
            let claims = replicator.stats().claims;
            let held = replicator.term().map(|term| (term.epoch.get(), term.device));
            let out = replicator.on_tick(model, at(clock(n))).unwrap();
            if !out.is_empty() || replicator.stats().timeouts != timeouts {
                assert!(
                    due.is_some_and(|due| due <= at(clock(n))),
                    "replica {n} had work at {now} ms, before its next tick, {due:?}",
                );
            }
            // A heartbeat period began if the tick sent heartbeats.
            let claimed = replicator.stats().claims != claims;
            if out.iter().any(|out| matches!(Frame::decode(&out.frame), Ok(Frame::Heartbeat(_)))) {
                *self.periods.get_mut(&n).unwrap() += 1;
                self.settle(n);
                if self.serving(n) {
                    let reading = u64::try_from(at(clock(n)).as_micros()).unwrap();
                    let beat = self.own_beat.get(&n).map_or(reading, |beat| reading.max(beat + 1));
                    self.own_beat.insert(n, beat);
                }
            } else {
                assert!(!claimed, "{n} claimed at {now} ms, not as a heartbeat period began");
            }
            if claimed {
                self.check_claim(n, held);
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

    /// What each replica holds.
    fn held(&self) -> BTreeMap<u8, VersionVector> {
        let held = |model: &Model| {
            let logs = model.logs().iter();
            logs.map(|(origin, log)| (*origin, u64::try_from(log.len()).unwrap())).collect()
        };
        self.models.iter().map(|(n, model)| (*n, held(model))).collect()
    }

    /// How many links each replica is from `from`.
    fn distances(&self, from: u8) -> BTreeMap<u8, u64> {
        let mut distances = BTreeMap::from([(from, 0)]);
        let mut queue = VecDeque::from([from]);
        while let Some(n) = queue.pop_front() {
            for peer in peers_of(&self.links, n) {
                let peer = replica_of(peer);
                if !distances.contains_key(&peer) {
                    distances.insert(peer, distances[&n] + 1);
                    queue.push_back(peer);
                }
            }
        }
        distances
    }

    /// Whether the replicas agree: every replica holds what any holds, and is settled; all hold
    /// the same winning claim, and have for a while, whose device alone serves as the hub, and
    /// has for a while, leaving nothing unnumbered; the replicas within two hops of the hub hear
    /// it, and no others; and every replica's watermark but the durable replica's reaches
    /// everything.
    fn agreed(&self) -> bool {
        self.disagreement().is_none()
    }

    /// Why the replicas don't agree, if they don't: see [`World::agreed`].
    fn disagreement(&self) -> Option<String> {
        self.discord().or_else(|| self.hearing().map(|(hearing, _)| hearing))
    }

    /// Why the replicas don't agree, if they don't, but for whom hears the hub.
    fn discord(&self) -> Option<String> {
        let held = self.held();
        let everything = &held[&1];
        if held.values().any(|vv| vv != everything) {
            return Some(format!("they hold {held:?}"));
        }
        let unsettled: Vec<u8> =
            self.replicators.iter().filter(|(_, r)| !r.settled()).map(|(n, _)| *n).collect();
        if !unsettled.is_empty() {
            return Some(format!("{unsettled:?} unsettled"));
        }
        let terms: Vec<_> = self.replicators.values().map(Replicator::term).collect();
        let Some(term) = self.replicators[&1].term().copied() else {
            return Some("no claim".into());
        };
        if terms.iter().any(|other| *other != Some(&term)) {
            return Some(format!("their terms are {terms:?}"));
        }
        // A replica that took on the term lately may still be telling the hub, through a peer,
        // of a later beat of the term than the hub's, from before the hub restarted with its
        // clock set back: until the hub's beats pass it, it can hear no hub for a period or so.
        let lately: Vec<u8> = self
            .holding_since
            .iter()
            .filter(|&(_, &since)| self.now - since < SETTLE)
            .map(|(n, _)| *n)
            .collect();
        if !lately.is_empty() {
            return Some(format!("{lately:?} took on the term lately"));
        }
        let hub = replica_of(term.device);
        let serving: Vec<u8> =
            self.replicators.iter().filter(|(_, r)| r.serving()).map(|(n, _)| *n).collect();
        if serving != [hub] {
            return Some(format!("{serving:?} serving, {hub} the hub"));
        }
        let since = self.serving_since.get(&hub).copied();
        if since.is_none_or(|since| self.now - since < SETTLE) {
            return Some(format!("{hub} serving only since {since:?}"));
        }
        let unsequenced = self.models[&hub].unsequenced();
        if !unsequenced.is_empty() {
            return Some(format!("{unsequenced:?} unnumbered"));
        }
        let behind: Vec<u8> = self
            .replicators
            .iter()
            .filter(|(n, r)| **n != self.case.replicas && r.durable() != everything)
            .map(|(n, _)| *n)
            .collect();
        (!behind.is_empty()).then(|| format!("{behind:?}'s watermarks behind"))
    }

    /// Unless the replicas within two hops of the winning claim's hub hear it, and no others:
    /// who hears it, and how far each is from it; and whether one within two hops doesn't.
    fn hearing(&self) -> Option<(String, bool)> {
        let hub = replica_of(self.replicators[&1].term()?.device);
        let distances = self.distances(hub);
        let hearing: Vec<(u8, bool, u64)> = self
            .replicators
            .iter()
            .map(|(n, replicator)| (*n, replicator.hub_reachable(), distances[n]))
            .collect();
        let deaf = hearing.iter().any(|&(_, hears, distance)| !hears && distance <= 2);
        let wrong = deaf || hearing.iter().any(|&(_, hears, distance)| hears && distance > 2);
        wrong.then(|| (format!("hearing the hub, and how far: {hearing:?}"), deaf))
    }

    /// Runs the case until the replicas have agreed for a while with nothing more written, or
    /// they fail to agree in time, or stop agreeing. If they fail, and `KEEL_SYNC_DUMP` is set,
    /// prints what each replica wrote.
    fn run(&mut self) -> Result<i64, String> {
        let run = self.run_until_agreed();
        if run.is_err() && std::env::var_os("KEEL_SYNC_DUMP").is_some() {
            self.dump();
        }
        run
    }

    fn run_until_agreed(&mut self) -> Result<i64, String> {
        let mut appends: Vec<(i64, u8, u8)> = self
            .case
            .appends
            .iter()
            .map(|&(t, replica, count)| (i64::from(t) * 100, replica, count))
            .collect();
        appends.sort_by_key(|&(t, _, _)| t);
        let mut crashes: Vec<(i64, u8, u16, i64)> = self
            .case
            .crashes
            .iter()
            .map(|&(t, replica, down, back)| {
                (i64::from(t) * 100, replica, down, i64::from(back) * 100)
            })
            .collect();
        crashes.sort_by_key(|&(t, _, _, _)| t);
        let mut jumps: Vec<(i64, u8, i64)> = self
            .case
            .jumps
            .iter()
            .map(|&(t, replica, by)| (i64::from(t) * 100, replica, i64::from(by) * 100))
            .collect();
        jumps.sort_by_key(|&(t, _, _)| t);
        let (mut next_append, mut next_crash, mut next_jump) = (0, 0, 0);
        let mut agreed: Option<(i64, BTreeMap<u8, VersionVector>)> = None;
        while self.now <= HEAL + AGREE_WITHIN + STABLE {
            while let Some(&(t, replica, count)) = appends.get(next_append)
                && t <= self.now
            {
                self.append(replica, count);
                next_append += 1;
            }
            while let Some(&(t, replica, down, back)) = crashes.get(next_crash)
                && t <= self.now
            {
                self.crash(replica, down, back);
                next_crash += 1;
            }
            while let Some(&(t, replica, by)) = jumps.get(next_jump)
                && t <= self.now
            {
                *self.ahead.entry(replica).or_insert(0) += by;
                next_jump += 1;
            }
            let up: Vec<u8> =
                self.down.iter().filter(|(_, up)| **up <= self.now).map(|(n, _)| *n).collect();
            for replica in up {
                self.restart(replica);
            }
            let due: Vec<(i64, u64)> =
                self.queue.range(..=(self.now, u64::MAX)).map(|(k, _)| *k).collect();
            for key in due {
                let (from, to, frame, start) = self.queue.remove(&key).unwrap();
                self.deliver(from, to, &frame, start);
            }
            if self.now % 100 == 0 {
                self.tick();
            }
            if self.now >= HEAL + SETTLE && next_append == appends.len() {
                // Once they agree on all else, those within two hops hear the hub at once: it has
                // served since healing, or for the periods of silence and two more, and each has
                // held its term as long.
                if self.discord().is_none()
                    && let Some((why, true)) = self.hearing()
                {
                    return Err(format!("at {} ms, agreeing but for {why}", self.now));
                }
                match (&agreed, self.agreed()) {
                    (None, true) => agreed = Some((self.now, self.held())),
                    (Some((since, _)), false) => {
                        let why = self.disagreement().unwrap_or_default();
                        return Err(format!("agreed at {since} ms, not at {} ms: {why}", self.now));
                    }
                    (Some((since, held)), true) if self.now >= since + STABLE => {
                        if held != &self.held() {
                            return Err(format!("agreed at {since} ms, and wrote more since"));
                        }
                        return Ok(self.now);
                    }
                    _ => {}
                }
            }
            if agreed.is_none() && self.now > HEAL + AGREE_WITHIN {
                break;
            }
            // On to the next thing to happen: a frame arriving, a tick, an append, a crash, a
            // clock jumping or a replica coming back up.
            let next_tick = (self.now / 100 + 1) * 100;
            let next_frame = self.queue.keys().next().map(|&(t, _)| t);
            let next_append_at = appends.get(next_append).map(|&(t, _, _)| t);
            let next_crash_at = crashes.get(next_crash).map(|&(t, _, _, _)| t);
            let next_jump_at = jumps.get(next_jump).map(|&(t, _, _)| t);
            let next_up = self.down.values().copied().min();
            self.now = [next_frame, next_append_at, next_crash_at, next_jump_at, next_up]
                .into_iter()
                .flatten()
                .fold(next_tick, i64::min)
                .max(self.now + 1);
        }
        let why = self.disagreement().unwrap_or_default();
        Err(format!("no agreement {AGREE_WITHIN} ms after healing: {why}"))
    }
}

impl World {
    /// Prints each replica's state, and its own log: its events, claims and records.
    #[allow(clippy::print_stderr, reason = "KEEL_SYNC_DUMP asks for each replica's log")]
    fn dump(&self) {
        for (n, model) in &self.models {
            let replicator = &self.replicators[n];
            let (serving, settled) = (replicator.serving(), replicator.settled());
            eprintln!(
                "replica {n}: {:?}, serving: {serving}, settled: {settled}",
                replicator.term()
            );
            let claims = model.claims();
            for event in model.logs().get(&device(*n)).into_iter().flatten() {
                let body = event.body();
                let what = if is_record(event) {
                    let record = record_of(event);
                    let runs = record
                        .runs
                        .iter()
                        .map(|run| format!("{}:{}..={}", replica_of(run.device), run.from, run.to));
                    let runs = runs.collect::<Vec<_>>().join(" ");
                    format!(
                        "record of epoch {}, {}..={}: {runs}",
                        record.epoch,
                        record.first,
                        record.last()
                    )
                } else if let Some(claim) = claims.iter().find(|claim| claim.event == body.event_id)
                {
                    format!("{:?}", claim.claimed)
                } else {
                    "event".into()
                };
                eprintln!("  {} at {} ms: {what}", body.origin_seq, body.hlc.wall_ms());
            }
        }
    }
}

/// The epoch of the winning claim `replicator` holds: 0 if none.
fn epoch_of(replicator: &Replicator) -> u64 {
    replicator.term().map_or(0, |term| term.epoch.get())
}

/// The sequencing record `event` holds.
fn record_of(event: &SignedEvent) -> Assigned {
    let body = event.body();
    let Ok(SequenceEvent::Assigned(record)) = SequenceEvent::decode(&body.schema, &body.payload)
    else {
        panic!("an unreadable record");
    };
    record
}

/// Runs `case`, and checks that the replicas came to agree as they must.
fn converge(case: Case) -> Result<(), TestCaseError> {
    let mut world = World::new(case);
    let agreed = world.run();
    prop_assert!(agreed.is_ok(), "{agreed:?}");
    // Every replica holds exactly what each wrote: the events appended, each in its place,
    // and claims and records.
    let expected: BTreeMap<Id<Device>, Vec<SignedEvent>> = world
        .models
        .iter()
        .filter_map(|(n, model)| Some((device(*n), model.logs().get(&device(*n))?.clone())))
        .collect();
    let appended: BTreeSet<Id<keel_events::envelope::Event>> =
        world.appended.iter().map(|event| event.body().event_id).collect();
    for event in &world.appended {
        let body = event.body();
        let position = usize::try_from(body.origin_seq.get() - 1).unwrap();
        prop_assert_eq!(&expected[&body.origin_device][position], event);
    }
    for event in expected.values().flatten() {
        let written =
            appended.contains(&event.body().event_id) || is_claim(event) || is_record(event);
        prop_assert!(written, "{:?}", event.body());
    }
    for (n, model) in &mut world.models {
        prop_assert_eq!(model.logs(), &expected, "replica {}", n);
        prop_assert!(model.quarantine.is_empty(), "replica {} refused {:?}", n, model.quarantine);
        // Its replicator knows what it holds.
        let held = model.version_vector().unwrap();
        prop_assert_eq!(world.replicators[n].version_vector(), &held, "replica {}", n);
    }
    // Every replica's own log is settled, and all hold the winning claim of the whole chain,
    // whose device alone serves.
    let unsettled: BTreeSet<u8> =
        world.replicators.iter().filter(|(_, r)| !r.settled()).map(|(n, _)| *n).collect();
    prop_assert!(unsettled.is_empty(), "{unsettled:?}");
    let model = &world.models[&1];
    let chain = model.chain();
    let term = chain[0];
    let epochs: Vec<u64> = chain.iter().map(|term| term.epoch.get()).collect();
    prop_assert_eq!(epochs, (1..=term.epoch.get()).rev().collect::<Vec<_>>());
    for (n, replicator) in &world.replicators {
        prop_assert_eq!(replicator.term(), Some(&term), "replica {}", n);
        prop_assert_eq!(replicator.serving(), device(*n) == term.device, "replica {}", n);
    }
    // Every claim is by a replica that may be hub, at its priority.
    for claim in model.claims() {
        let priority = priority_of(&world.case, replica_of(claim.device));
        prop_assert_eq!(claim.claimed.priority.get(), priority, "{:?}", claim);
    }
    // The records that count number every other event exactly once, each epoch's numbers
    // gapless from 1, each device's log in order.
    let mut numbered: BTreeMap<(Id<Device>, u64), (u64, u64)> = BTreeMap::new();
    for (_, _, record) in model.counting_records() {
        for (number, run) in record.numbered() {
            for position in run.from..=run.to {
                let number = (record.epoch, number + (position - run.from));
                let again = numbered.insert((run.device, position), number);
                prop_assert!(again.is_none(), "{:?} {} numbered twice", run.device, position);
            }
        }
    }
    let others: BTreeSet<(Id<Device>, u64)> = expected
        .values()
        .flatten()
        .filter(|event| !is_record(event))
        .map(|event| (event.body().origin_device, event.body().origin_seq.get()))
        .collect();
    prop_assert_eq!(numbered.keys().copied().collect::<BTreeSet<_>>(), others);
    let mut epochs: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for &(epoch, number) in numbered.values() {
        epochs.entry(epoch).or_default().insert(number);
    }
    for (epoch, numbers) in epochs {
        let count = u64::try_from(numbers.len()).unwrap();
        prop_assert_eq!(numbers, (1..=count).collect::<BTreeSet<_>>(), "epoch {}", epoch);
    }
    for ((first, first_number), (second, second_number)) in
        numbered.iter().zip(numbered.iter().skip(1))
    {
        prop_assert!(
            first.0 != second.0 || first_number < second_number,
            "{:?} {:?}, {:?} {:?}",
            first,
            first_number,
            second,
            second_number
        );
    }
    Ok(())
}

proptest! {
    /// Replicas exchanging frames over a faulty network converge to exactly the events appended,
    /// and the claims and records written, with one hub they all agree on, which has numbered
    /// every event once.
    #[test]
    fn replicas_converge_over_a_faulty_network(case in any_case()) {
        converge(case)?;
    }
}

/// Found by the property: replica 2, the most preferred, and replica 3, linked through replica 1
/// alone, took the hub's role from each other every second once replica 3 had stood in while 2
/// was down. Each, deposed, learned of the other's claim before any heartbeat told it of that
/// hub, since replica 1 relays what it hears only as its next period begins, so it heard no hub of
/// its new epoch, and claimed the next at once. Learning of a claim now counts as hearing its hub.
#[test]
fn a_deposed_hub_doesnt_claim_back_before_it_hears_of_its_successor() {
    let case = Case {
        replicas: 4,
        topology: Topology::Star,
        loss: 80,
        duplication: 0,
        decline: 172,
        delay: 77,
        appends: vec![(28, 3, 3), (152, 1, 3), (187, 3, 1), (116, 3, 1), (24, 3, 2)],
        cuts: vec![],
        crashes: vec![(52, 3, 28, 0), (128, 2, 12, 0)],
        jumps: vec![],
        priorities: vec![Some(1), Some(3), Some(2), None],
        batch_events: 2,
        batch_bytes: 371,
        heartbeat: HEARTBEAT,
        seed: 12_174_520_576_721_775_304,
    };
    converge(case).unwrap();
}

/// Found by the property, while a peer that heard the hub said how many of its periods ago, and a
/// replica took that, less one, in its own: replica 3 heard the hub, replica 2, only through
/// replica 1, and stopped hearing it once a heartbeat from replica 1 that had last heard the hub
/// two periods before was followed by one that came two of replica 3's periods later, as frames
/// delayed by up to 110 ms can. A peer now passes on the hub's beat itself, and a replica hears
/// the hub while the latest beat it knows of reached it recently.
#[test]
fn a_replica_hearing_the_hub_through_a_peer_goes_on_hearing_it_through_delays() {
    let case = Case {
        replicas: 3,
        topology: Topology::Star,
        loss: 0,
        duplication: 51,
        decline: 0,
        delay: 110,
        appends: vec![(0, 1, 1)],
        cuts: vec![(2, 101, 31)],
        crashes: vec![(151, 1, 0, 0)],
        jumps: vec![],
        priorities: vec![Some(1), Some(1), None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 3_407_747_264_193_243_812,
    };
    converge(case).unwrap();
}

/// Found by the property, while the hub numbered what waited only as its log settled: replica 2
/// declined a batch of the hub's own log, so the hub, replica 1, took its log for forked and
/// stopped serving; when replica 2 took the log after all, the hub served again, but numbered
/// nothing until it next stored events. A replica now numbers what waits as it begins to serve.
#[test]
fn a_hub_that_serves_again_numbers_what_came_meanwhile() {
    let case = Case {
        replicas: 2,
        topology: Topology::Star,
        loss: 0,
        duplication: 59,
        decline: 219,
        delay: 73,
        appends: vec![(140, 1, 1), (29, 1, 1), (0, 1, 1)],
        cuts: vec![],
        crashes: vec![(61, 1, 29, 0)],
        jumps: vec![],
        priorities: vec![Some(1), None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 6_863_970_644_450_523_106,
    };
    converge(case).unwrap();
}

/// Found by the property, while a refusal of a replica's own log lasted until a peer took a later
/// batch of it: replica 2 declined one copy of a batch of the hub's own log and took a duplicate
/// of it; the hub, replica 1, heard of the refusal first and took its log for forked, and, with
/// nothing more of its own log for replica 2, never sent a batch that could show otherwise. A
/// refusal now ends as soon as the peer says it holds the log that far.
#[test]
fn a_peer_that_holds_a_replicas_log_after_all_no_longer_refuses_it() {
    let case = Case {
        replicas: 2,
        topology: Topology::Star,
        loss: 0,
        duplication: 155,
        decline: 218,
        delay: 214,
        appends: vec![(31, 1, 1)],
        cuts: vec![],
        crashes: vec![],
        jumps: vec![],
        priorities: vec![Some(1), None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 9_369_107_273_393_270_102,
    };
    converge(case).unwrap();
}

/// Found by the property, which counted the replicas as agreeing as soon as the hub served: two
/// replicas claimed epoch 1, and the winner, replica 3, began to serve only seconds later, its
/// log taken for forked after a batch declined. Replica 1, two hops away, had learned of the
/// winning claim, heard no beat of its hub in the periods of silence that followed, and rightly
/// claimed epoch 2, after the property had counted the replicas as agreeing. Agreement now counts
/// once the hub has served for the periods of silence and two more.
#[test]
fn replicas_agree_only_once_the_hub_has_served_a_while() {
    let case = Case {
        replicas: 4,
        topology: Topology::Line,
        loss: 117,
        duplication: 165,
        decline: 275,
        delay: 236,
        appends: vec![
            (31, 1, 2),
            (9, 1, 2),
            (127, 2, 3),
            (16, 4, 1),
            (31, 1, 3),
            (16, 4, 1),
            (166, 3, 1),
            (166, 2, 3),
        ],
        cuts: vec![(24, 77, 58)],
        crashes: vec![(25, 2, 101, 0), (180, 3, 5, 0), (0, 3, 115, 0), (116, 4, 0, 0)],
        jumps: vec![],
        priorities: vec![Some(1), None, Some(2), None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 5_150_049_103_059_654_294,
    };
    converge(case).unwrap();
}

/// Found by the property, once clocks jumped, while beats were compared by epoch and number
/// alone: replica 1, the star's centre, was down while replicas 2 and 3 each claimed epoch 1.
/// When it healed, replica 1 had beats from both; replica 3's, its clock 3.8 s ahead, were
/// higher than replica 2's, the winner's, so that replica 2's beats looked no newer, and four
/// periods later replica 1 took its hub for silent and claimed epoch 2. A heartbeat now names its
/// hub, and beats are compared only within one hub's term.
#[test]
fn a_losing_hubs_beats_dont_drown_out_the_winners() {
    let case = Case {
        replicas: 4,
        topology: Topology::Star,
        loss: 97,
        duplication: 0,
        decline: 141,
        delay: 216,
        appends: vec![(0, 2, 1), (139, 3, 1), (153, 4, 1)],
        cuts: vec![],
        crashes: vec![(89, 3, 31, 0), (18, 1, 103, 0), (190, 2, 0, 0), (12, 3, 17, 0)],
        jumps: vec![(152, 3, 38)],
        priorities: vec![Some(3), Some(1), Some(1), None],
        batch_events: 2,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 6_863_262_935_672_717_334,
    };
    converge(case).unwrap();
}

/// Found reviewing slice 4, once the property set clocks back as replicas restarted, and had the
/// replicas that agree on all else hear the hub at once: a hub's beat began again from its
/// clock's reading as it restarted. Replica 1, the hub and the only candidate, restarted at
/// 13.1 s with its clock set back 11.1 s; its beats were older than the last replica 2 had, and
/// replica 2 heard no hub until 25.1 s. A hub now beats on above every floor of its term a peer
/// gives it.
#[test]
fn a_hub_that_restarts_with_its_clock_set_back_is_heard_at_once() {
    let case = Case {
        replicas: 2,
        topology: Topology::Star,
        loss: 0,
        duplication: 0,
        decline: 0,
        delay: 1,
        appends: vec![(0, 1, 1)],
        cuts: vec![],
        crashes: vec![(131, 1, 0, 111)],
        jumps: vec![],
        priorities: vec![Some(1), None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 0,
    };
    converge(case).unwrap();
}

/// Found by the property against a first fix, in which a replica gave the hub of its term the
/// latest beat of it it knew: replica 1, the hub and the only candidate, came back at 15.6 s
/// after 6.2 s down, its clock set back 15.1 s. Replica 2, between it and replica 3, had
/// restarted meanwhile and forgotten the hub's beats, and replica 3, which knew the last, had no
/// way to tell the hub: it heard no hub until the hub's clock passed that beat. Every replica now
/// gives every peer its term's floor, and keeps the latest floor it hears, so that a hub hears of
/// its last beats from two hops away.
#[test]
fn a_hub_hears_of_its_last_beat_through_a_restarted_peer() {
    let case = Case {
        replicas: 3,
        topology: Topology::Line,
        loss: 0,
        duplication: 0,
        decline: 0,
        delay: 1,
        appends: vec![(0, 1, 1)],
        cuts: vec![],
        crashes: vec![(160, 2, 26, 0), (94, 1, 62, 151)],
        jumps: vec![],
        priorities: vec![Some(1), None, None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 0,
    };
    converge(case).unwrap();
}

/// Found reviewing slice 4, once the property set clocks back as replicas restarted: batches were
/// numbered from the clock's reading as the replicator started. Replica 2 restarted at 13.3 s,
/// and again at 15.2 s with its clock set back 1.9 s, at the same reading; its batches took the
/// numbers of the last run's, and replica 1's acknowledgement of one of those passed for one of
/// these. Each start now numbers its batches from random bits.
#[test]
fn a_restart_that_sets_the_clock_back_takes_no_old_acknowledgement_for_new() {
    let case = Case {
        replicas: 2,
        topology: Topology::Star,
        loss: 152,
        duplication: 150,
        decline: 233,
        delay: 191,
        appends: vec![(30, 1, 3), (102, 1, 2), (134, 2, 1), (31, 1, 3)],
        cuts: vec![],
        crashes: vec![(133, 2, 0, 0), (149, 2, 3, 19)],
        jumps: vec![],
        priorities: vec![Some(1), None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 4_750_328_451_064_403_066,
    };
    converge(case).unwrap();
}

/// Found by the property against a first fix, which numbered a replica's batches to a peer past
/// the peer's last acknowledgement that wasn't of one sent since the replica started: replica 4
/// restarted at once at 5.4 s and again at 15.4 s, both times with batches to replica 2 in
/// flight, and both runs numbered theirs past the same acknowledgement, so that replica 2's
/// acknowledgement of the first run's batch passed for the second's.
#[test]
fn quick_restarts_with_batches_in_flight_take_no_old_acknowledgement_for_new() {
    let case = Case {
        replicas: 4,
        topology: Topology::Mesh,
        loss: 0,
        duplication: 21,
        decline: 41,
        delay: 165,
        appends: vec![(153, 1, 3), (18, 1, 1), (18, 4, 3)],
        cuts: vec![],
        crashes: vec![(54, 4, 0, 0), (154, 4, 0, 0)],
        jumps: vec![],
        priorities: vec![Some(1), Some(2), None, None],
        batch_events: 2,
        batch_bytes: 262_144,
        heartbeat: HEARTBEAT,
        seed: 9_190_104_921_008_549_149,
    };
    converge(case).unwrap();
}

/// Found by the property's check that, once the replicas agree on all else, those within two hops
/// of the hub hear it at once, which asked too much: the hub, replica 2, restarted at 18.1 s with
/// its clock set back 12.5 s, and replica 3, two hops away and behind on the log, took on its
/// term only at 22.5 s, knowing a beat of it from before the restart that the hub's new ones
/// hadn't passed. Its floor took four periods to reach the hub and the hub's next beat to come
/// back, one more than learning of the claim counts as hearing its hub, and at 25.4 s replica 3
/// heard no hub for a moment. Agreement now counts only once every replica has held the hub's
/// term for the periods of silence and two more.
#[test]
fn a_replica_that_has_only_now_taken_on_the_hubs_term_may_hear_it_a_period_late() {
    let case = Case {
        replicas: 3,
        topology: Topology::Star,
        loss: 78,
        duplication: 0,
        decline: 43,
        delay: 250,
        appends: vec![(84, 1, 3), (140, 3, 2), (51, 2, 2), (21, 1, 2), (21, 1, 2)],
        cuts: vec![],
        crashes: vec![(6, 3, 112, 0), (132, 2, 49, 125), (136, 1, 63, 0)],
        jumps: vec![],
        priorities: vec![Some(2), Some(1), None],
        batch_events: 1,
        batch_bytes: 262_144,
        heartbeat: 800,
        seed: 15_468_588_063_414_672_920,
    };
    converge(case).unwrap();
}
