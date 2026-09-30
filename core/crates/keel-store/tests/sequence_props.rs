//! Property tests for sequencing (ADR-0020), against a model: a hub numbering what it receives,
//! another replica confirming from the hub's records, and that replica numbering in turn, as the
//! next epoch's hub would.
//!
//! A case is a run of steps at the hub: receiving the next events of two devices' logs,
//! appending events of its own, and sequencing, now and then interrupted as it commits. Another
//! replica then takes in the hub's log and the two devices' logs in any interleaving, so that
//! records arrive before or after the events they cover; the first device may have forked its
//! log, and the replica hold the other version. The replica then sequences in the next epoch, and
//! the hub takes in the replica's records and the rest of the two devices' logs.
//!
//! After every step, each store's numbers, confirmed logs and feeds must be the model's:
//! - the hub numbers what it holds that no record covers, in the order it received it, from 1 in
//!   its epoch, leaving records out;
//! - the next epoch's sequencer numbers, for each device, the events after the last position any
//!   record it holds covers, records aside, in the order it received them, from 1; a record of its
//!   ends where a device's next event doesn't follow on from the device's run in it;
//! - a store confirms the events of a run once it holds the record and the run's last event, as
//!   the run's sequencer held it, and counts each event under its first number, by epoch and
//!   number.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use keel_domain::schema::DomainEvent;
use keel_domain::sequence::SequenceEvent;
use keel_events::envelope::Device;
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::keys::SoftwareSigner;
use keel_events::log::LogHead;
use keel_store::{Faults, Point, Received, Store, StoreError, StoreSeq};
use keel_types::{Id, SeededEntropy};
use proptest::prelude::*;
use support::{
    OWN, PEERS, REPLICA, Scratch, at, config_of, device, draft, here, next_event, registry, signer,
    store_key,
};

const EPOCH: u64 = 1;
/// The epoch the replica sequences in, after the hub.
const NEXT: u64 = 2;

type TestStore = Store<SoftwareSigner, SeededEntropy>;

/// Refuses the write in progress as it is about to commit, once, when armed.
#[derive(Clone, Default)]
struct Plan(Arc<Mutex<bool>>);

impl Faults for Plan {
    fn proceed(&mut self, point: Point) -> bool {
        let mut armed = self.0.lock().unwrap();
        if point == Point::Committing && *armed {
            *armed = false;
            return false;
        }
        true
    }
}

/// A step at the hub.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// Receives the next `count` events of peer `peer`'s log, in one write.
    Receive { peer: usize, count: usize },
    /// Appends `count` events of its own.
    Own(u8),
    /// Sequences; interrupted as it commits, if so.
    Sequence { interrupted: bool },
}

fn any_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0_usize..2, 1_usize..4).prop_map(|(peer, count)| Step::Receive { peer, count }),
        1 => (1_u8..3).prop_map(Step::Own),
        3 => Just(Step::Sequence { interrupted: false }),
        1 => Just(Step::Sequence { interrupted: true }),
    ]
}

/// Device `n`'s log of `count` events: variant 0 up to `fork`, and variant 1 from there on.
fn log(n: u8, count: usize, fork: Option<usize>) -> Vec<SignedEvent> {
    let mut log: Vec<SignedEvent> = Vec::new();
    for k in 1..=count {
        let head = log.last().map_or(LogHead::EMPTY, LogHead::of);
        let variant = u64::from(fork.is_some_and(|fork| k >= fork));
        let now = at(1_000 + i64::try_from(k).unwrap() * 10 + i64::try_from(variant).unwrap());
        log.push(next_event(n, here(), head, now, 1, variant));
    }
    log
}

/// A case: the peers' logs as the hub holds them, the first's other version if it forked, the
/// hub's steps, and the replica's schedule: which log it takes the next event of at each turn.
#[derive(Clone, Debug)]
struct Case {
    lengths: [usize; 2],
    fork: Option<usize>,
    steps: Vec<Step>,
    schedule: Vec<usize>,
}

fn any_case() -> impl Strategy<Value = Case> {
    (
        [1_usize..6, 1_usize..6],
        prop::option::of(1_usize..6),
        prop::collection::vec(any_step(), 1..14),
        prop::collection::vec(0_usize..3, 0..30),
    )
        .prop_map(|(lengths, fork, steps, schedule)| {
            // A fork at position 1 would be another log altogether: from 2 on.
            let fork = fork.map(|fork| fork.clamp(2, lengths[0].max(2)));
            Case { lengths, fork, steps, schedule }
        })
}

/// A run of a record, by the model: its epoch, device, first and last positions, first number,
/// and the hash of its last event, as the store that wrote the record held it.
#[derive(Clone, Copy, Debug)]
struct ModelRun {
    epoch: u64,
    device: Id<Device>,
    from: u64,
    to: u64,
    number: u64,
    last: EventHash,
}

/// What the hub holds and numbered, by the model.
#[derive(Default)]
struct Hub {
    /// The events it holds, except records, in the order it received or appended them.
    arrivals: Vec<(Id<Device>, u64)>,
    /// The number of each event a record covers.
    numbers: BTreeMap<(Id<Device>, u64), u64>,
    /// The hash of each event it holds, records aside.
    hashes: BTreeMap<(Id<Device>, u64), EventHash>,
    /// Its own log: whether each position is a record, and if so, the record's runs.
    own: Vec<Option<Vec<ModelRun>>>,
    next: u64,
}

impl Hub {
    fn hub() -> Id<Device> {
        device(OWN)
    }

    /// Numbers what no record covers, in the order it arrived: a record of runs, if anything is
    /// new. Each device's events side by side make one run.
    fn sequence(&mut self) -> Option<Vec<ModelRun>> {
        let pending: Vec<(Id<Device>, u64)> = self
            .arrivals
            .iter()
            .copied()
            .filter(|event| !self.numbers.contains_key(event))
            .collect();
        if pending.is_empty() {
            return None;
        }
        let mut runs: Vec<ModelRun> = Vec::new();
        for (device, position) in pending {
            let number = self.next.max(1);
            self.next = number + 1;
            self.numbers.insert((device, position), number);
            let last = self.hashes[&(device, position)];
            match runs.last_mut() {
                Some(run) if run.device == device && run.to + 1 == position => {
                    run.to = position;
                    run.last = last;
                }
                _ => runs.push(ModelRun {
                    epoch: EPOCH,
                    device,
                    from: position,
                    to: position,
                    number,
                    last,
                }),
            }
        }
        self.own.push(Some(runs.clone()));
        Some(runs)
    }

    /// The records in the first `held` positions of the hub's log.
    fn records_in(&self, held: usize) -> impl Iterator<Item = &ModelRun> {
        self.own.iter().take(held).flatten().flatten()
    }
}

/// What a store must answer, by the model.
struct Expected {
    /// Each event it holds, records aside, and the event's first number, if confirmed.
    numbers: BTreeMap<(Id<Device>, u64), Option<StoreSeq>>,
    /// The records it holds: their authors and positions.
    records: Vec<(Id<Device>, u64)>,
}

impl Expected {
    /// For each device, the longest start of its log in which each event is confirmed or a
    /// record.
    fn confirmed(&self) -> BTreeMap<Id<Device>, u64> {
        let mut settled: BTreeMap<Id<Device>, Vec<u64>> = BTreeMap::new();
        for (&(device, position), number) in &self.numbers {
            if number.is_some() {
                settled.entry(device).or_default().push(position);
            }
        }
        for &(device, position) in &self.records {
            settled.entry(device).or_default().push(position);
        }
        let mut confirmed = BTreeMap::new();
        for (device, mut positions) in settled {
            positions.sort_unstable();
            let reach =
                positions.iter().zip(1..).take_while(|&(&position, k)| position == k).count();
            if reach > 0 {
                confirmed.insert(device, u64::try_from(reach).unwrap());
            }
        }
        confirmed
    }

    /// The feed of `epoch`: every event whose first number is of the epoch, by number.
    fn feed(&self, epoch: u64) -> Vec<(u64, Id<Device>, u64)> {
        let mut feed: Vec<(u64, Id<Device>, u64)> = self
            .numbers
            .iter()
            .filter_map(|(&(device, position), number)| {
                number
                    .filter(|number| number.epoch == epoch)
                    .map(|number| (number.number, device, position))
            })
            .collect();
        feed.sort_unstable();
        feed
    }
}

/// Checks `store` against what the model expects of it.
fn check(store: &TestStore, expected: &Expected, when: &str) -> Result<(), TestCaseError> {
    for (&(device, position), &number) in &expected.numbers {
        let found = store.store_seq(device, position).unwrap();
        prop_assert_eq!(found, number, "{} {:?} {}", when, device, position);
    }
    for &(device, position) in &expected.records {
        let found = store.store_seq(device, position).unwrap();
        prop_assert_eq!(found, None, "{} the record {:?} {}", when, device, position);
    }
    prop_assert_eq!(store.confirmed().unwrap(), expected.confirmed(), "{}", when);
    for epoch in [EPOCH, NEXT] {
        let feed: Vec<(u64, Id<Device>, u64)> = store
            .sequenced(epoch, 0, 1_000)
            .unwrap()
            .into_iter()
            .map(|entry| {
                let body = entry.event.body();
                (entry.number, body.origin_device, body.origin_seq.get())
            })
            .collect();
        prop_assert_eq!(&feed, &expected.feed(epoch), "{} epoch {}", when, epoch);
        // A page from the middle.
        let after = feed.get(feed.len() / 2).map_or(0, |&(number, _, _)| number);
        let page: Vec<u64> = store
            .sequenced(epoch, after, 2)
            .unwrap()
            .into_iter()
            .map(|entry| entry.number)
            .collect();
        let wanted: Vec<u64> = feed
            .iter()
            .map(|&(number, _, _)| number)
            .filter(|&number| number > after)
            .take(2)
            .collect();
        prop_assert_eq!(page, wanted, "{} epoch {} after {}", when, epoch, after);
    }
    Ok(())
}

/// The hub's records among the first `held` positions of its log.
fn hub_records(model: &Hub, held: usize) -> Vec<(Id<Device>, u64)> {
    (1..)
        .zip(model.own.iter().take(held))
        .filter(|(_, record)| record.is_some())
        .map(|(k, _)| (Hub::hub(), k))
        .collect()
}

/// What the hub itself must answer: it holds what it numbered, as it numbered it.
fn at_hub(model: &Hub) -> Expected {
    let numbers = model
        .arrivals
        .iter()
        .map(|event| {
            (*event, model.numbers.get(event).map(|&number| StoreSeq { epoch: EPOCH, number }))
        })
        .collect();
    Expected { numbers, records: hub_records(model, model.own.len()) }
}

/// What a store holding the events `hashes` gives, records aside, and the records `records`, of
/// runs `runs`, must answer: each run covering an event whose last event the store holds as the
/// run's sequencer held it numbers the event, and the event counts under its first number, by
/// epoch and number.
fn confirming(
    hashes: &BTreeMap<(Id<Device>, u64), EventHash>,
    records: Vec<(Id<Device>, u64)>,
    runs: &[ModelRun],
) -> Expected {
    let mut numbers: BTreeMap<(Id<Device>, u64), Option<StoreSeq>> =
        hashes.keys().map(|&event| (event, None)).collect();
    for run in runs {
        if hashes.get(&(run.device, run.to)) != Some(&run.last) {
            continue;
        }
        for position in run.from..=run.to {
            let number = StoreSeq { epoch: run.epoch, number: run.number + (position - run.from) };
            let first = numbers.get_mut(&(run.device, position)).unwrap();
            let earlier =
                |first: &StoreSeq| (first.epoch, first.number) < (number.epoch, number.number);
            if !first.as_ref().is_some_and(earlier) {
                *first = Some(number);
            }
        }
    }
    Expected { numbers, records }
}

/// What a replica holding the first `held` events of each log must answer: `logs` are its
/// versions of the peers' logs, and `hub_log` the hub's log. Also the events it holds, records
/// aside, with their hashes.
fn at_replica(
    model: &Hub,
    logs: &[Vec<SignedEvent>; 2],
    hub_log: &[SignedEvent],
    held: [usize; 3],
) -> (Expected, BTreeMap<(Id<Device>, u64), EventHash>) {
    let mut hashes: BTreeMap<(Id<Device>, u64), EventHash> = BTreeMap::new();
    for (log, &count) in logs.iter().zip(&held) {
        for event in &log[..count] {
            hashes
                .insert((event.body().origin_device, event.body().origin_seq.get()), event.hash());
        }
    }
    for ((position, event), record) in (1..).zip(&hub_log[..held[2]]).zip(&model.own) {
        if record.is_none() {
            hashes.insert((Hub::hub(), position), event.hash());
        }
    }
    let runs: Vec<ModelRun> = model.records_in(held[2]).copied().collect();
    (confirming(&hashes, hub_records(model, held[2]), &runs), hashes)
}

/// The records a store sequencing in a new epoch `epoch` writes, having received `arrivals`, of
/// hashes `hashes`, in that order, records aside, and holding records of runs `runs`: for each
/// device, it numbers the events after the last position any run covers, in the order it
/// received them, from 1. A record ends where a device's next event doesn't follow on from the
/// device's run in it.
fn sequence_anew(
    epoch: u64,
    arrivals: &[(Id<Device>, u64)],
    hashes: &BTreeMap<(Id<Device>, u64), EventHash>,
    runs: &[ModelRun],
) -> Vec<Vec<ModelRun>> {
    let mut covered: BTreeMap<Id<Device>, u64> = BTreeMap::new();
    for run in runs {
        let end = covered.entry(run.device).or_default();
        *end = (*end).max(run.to);
    }
    let mut records: Vec<Vec<ModelRun>> = Vec::new();
    let mut record: Vec<ModelRun> = Vec::new();
    let mut number = 1;
    for &(device, position) in arrivals {
        if position <= covered.get(&device).copied().unwrap_or(0) {
            continue;
        }
        let last = hashes[&(device, position)];
        match record.last_mut() {
            Some(run) if run.device == device && run.to + 1 == position => {
                run.to = position;
                run.last = last;
            }
            _ => {
                let before = record.iter().rev().find(|run| run.device == device);
                if before.is_some_and(|run| run.to + 1 != position) {
                    records.push(core::mem::take(&mut record));
                }
                record.push(ModelRun { epoch, device, from: position, to: position, number, last });
            }
        }
        number += 1;
    }
    if !record.is_empty() {
        records.push(record);
    }
    records
}

/// Checks that `written` are the records `wanted`, of epoch `epoch`.
fn same_records(
    written: &[SignedEvent],
    wanted: &[Vec<ModelRun>],
    epoch: u64,
) -> Result<(), TestCaseError> {
    prop_assert_eq!(written.len(), wanted.len());
    for (event, runs) in written.iter().zip(wanted) {
        let body = event.body();
        let Ok(SequenceEvent::Assigned(record)) =
            SequenceEvent::decode(&body.schema, &body.payload)
        else {
            panic!("not a record");
        };
        prop_assert_eq!(record.epoch, epoch);
        prop_assert_eq!(record.first, runs[0].number);
        let spans: Vec<(Id<Device>, u64, u64, EventHash)> =
            record.runs.iter().map(|run| (run.device, run.from, run.to, run.last)).collect();
        let wanted: Vec<(Id<Device>, u64, u64, EventHash)> =
            runs.iter().map(|run| (run.device, run.from, run.to, run.last)).collect();
        prop_assert_eq!(spans, wanted);
    }
    Ok(())
}

fn open(scratch: &Scratch, n: u8, plan: &Plan) -> TestStore {
    Store::open_with_faults(
        scratch.db(),
        store_key(),
        config_of(n),
        signer(n),
        SeededEntropy::new(u64::from(n)),
        Box::new(plan.clone()),
    )
    .unwrap()
}

fn receive(store: &mut TestStore, events: &[SignedEvent], now: i64) {
    store
        .write(|w| {
            for event in events {
                let received = w.receive(&event.to_bytes(), &registry(), at(now))?;
                assert!(matches!(received, Received::Stored(_)), "{received:?}");
            }
            Ok::<_, StoreError>(())
        })
        .unwrap();
}

proptest! {
    /// The hub numbers what it receives in order, and a replica confirms exactly what the hub's
    /// records cover of the version the hub held, whatever order it receives things in.
    #[test]
    fn stores_number_and_confirm_as_the_model_does(case in any_case()) {
        let (hub_scratch, replica_scratch) = (Scratch::new("sequence-hub"), Scratch::new("sequence-replica"));
        let plan = Plan::default();
        let mut hub = open(&hub_scratch, OWN, &plan);
        let peers = [log(PEERS[0], case.lengths[0], None), log(PEERS[1], case.lengths[1], None)];
        let mut model = Hub::default();
        let mut received = [0_usize; 2];
        for (i, step) in case.steps.iter().enumerate() {
            let now = 10_000 + i64::try_from(i).unwrap() * 100;
            match *step {
                Step::Receive { peer, count } => {
                    let upto = (received[peer] + count).min(peers[peer].len());
                    let events = &peers[peer][received[peer]..upto];
                    receive(&mut hub, events, now);
                    for event in events {
                        let key = (event.body().origin_device, event.body().origin_seq.get());
                        model.arrivals.push(key);
                        model.hashes.insert(key, event.hash());
                    }
                    received[peer] = upto;
                }
                Step::Own(count) => {
                    let drafts = (0..count).map(|k| draft(2, u64::from(k)));
                    let events = hub
                        .write(|w| drafts.map(|draft| w.append(draft, at(now))).collect::<Result<Vec<_>, _>>())
                        .unwrap();
                    for event in &events {
                        let key = (Hub::hub(), event.body().origin_seq.get());
                        model.arrivals.push(key);
                        model.hashes.insert(key, event.hash());
                        model.own.push(None);
                    }
                }
                Step::Sequence { interrupted: true } => {
                    *plan.0.lock().unwrap() = true;
                    let sequenced = hub.sequence(EPOCH, at(now));
                    prop_assert!(matches!(sequenced, Err(StoreError::Interrupted(Point::Committing))), "{:?}", sequenced);
                }
                Step::Sequence { interrupted: false } => {
                    let written = hub.sequence(EPOCH, at(now)).unwrap();
                    let runs = model.sequence();
                    same_records(&written, runs.as_slice(), EPOCH)?;
                    if let Some(event) = written.first() {
                        prop_assert_eq!(event.body().origin_seq.get(), u64::try_from(model.own.len()).unwrap());
                    }
                }
            }
            check(&hub, &at_hub(&model), &format!("hub, after {step:?}"))?;
        }
        prop_assert!(hub.check().unwrap().is_empty());

        // Another replica: the hub's log, and its versions of the peers' logs, in any order.
        let hub_log = hub.log(Hub::hub(), 0, 1_000).unwrap();
        prop_assert_eq!(hub_log.len(), model.own.len());
        let versions = [log(PEERS[0], case.lengths[0], case.fork), peers[1].clone()];
        let mut replica = open(&replica_scratch, REPLICA, &plan);
        let mut held = [0_usize; 3];
        let lengths = [versions[0].len(), versions[1].len(), hub_log.len()];
        let rest = (0..3).flat_map(|log| core::iter::repeat_n(log, lengths[log]));
        // What the replica received, records aside, in order.
        let mut arrivals: Vec<(Id<Device>, u64)> = Vec::new();
        for (turn, log) in case.schedule.iter().copied().chain(rest).enumerate() {
            if held[log] == lengths[log] {
                continue;
            }
            let event = if log == 2 { &hub_log[held[log]] } else { &versions[log][held[log]] };
            receive(&mut replica, core::slice::from_ref(event), 20_000 + i64::try_from(turn).unwrap());
            if log != 2 || model.own[held[log]].is_none() {
                arrivals.push((event.body().origin_device, event.body().origin_seq.get()));
            }
            held[log] += 1;
            let (expected, _) = at_replica(&model, &versions, &hub_log, held);
            check(&replica, &expected, &format!("replica, holding {held:?}"))?;
        }
        prop_assert!(replica.check().unwrap().is_empty());
        // Without a fork, the replica ends holding what the hub holds, confirmed alike.
        if case.fork.is_none() {
            prop_assert_eq!(replica.confirmed().unwrap(), hub.confirmed().unwrap());
        }

        // The replica then sequences in the next epoch, as the hub after this one would.
        let (_, hashes) = at_replica(&model, &versions, &hub_log, held);
        let mut runs: Vec<ModelRun> = model.records_in(hub_log.len()).copied().collect();
        let anew = sequence_anew(NEXT, &arrivals, &hashes, &runs);
        let written = replica.sequence(NEXT, at(30_000)).unwrap();
        same_records(&written, &anew, NEXT)?;
        runs.extend(anew.iter().flatten());
        let replica_records: Vec<(Id<Device>, u64)> =
            (1..).zip(&anew).map(|(k, _)| (device(REPLICA), k)).collect();
        let records = [hub_records(&model, hub_log.len()), replica_records.clone()].concat();
        check(&replica, &confirming(&hashes, records, &runs), "replica, after sequencing")?;
        // Nothing is left to number.
        prop_assert!(replica.sequence(NEXT, at(30_100)).unwrap().is_empty());
        prop_assert!(replica.check().unwrap().is_empty());

        // The hub takes in the replica's records, then the rest of the two devices' logs, as it
        // has them: it confirms the replica's runs that end in events it holds as the replica
        // held them.
        let replica_log = replica.log(device(REPLICA), 0, 1_000).unwrap();
        prop_assert_eq!(replica_log.len(), anew.len());
        let mut hashes = model.hashes.clone();
        let mut records = hub_records(&model, model.own.len());
        let rest: Vec<&SignedEvent> = replica_log
            .iter()
            .chain(&peers[0][received[0]..])
            .chain(&peers[1][received[1]..])
            .collect();
        for (turn, event) in rest.into_iter().enumerate() {
            receive(&mut hub, core::slice::from_ref(event), 40_000 + i64::try_from(turn).unwrap());
            let key = (event.body().origin_device, event.body().origin_seq.get());
            if key.0 == device(REPLICA) {
                records.push(key);
            } else {
                hashes.insert(key, event.hash());
            }
            let expected = confirming(&hashes, records.clone(), &runs);
            check(&hub, &expected, &format!("hub, holding {key:?} of the rest"))?;
        }
        prop_assert!(hub.check().unwrap().is_empty());
    }
}
