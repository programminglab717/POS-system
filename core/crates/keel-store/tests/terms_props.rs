//! Property tests for hub terms (ADR-0022) in the store, against a model: three stores, each of
//! a device that can be hub, appending events, taking any part of any log from each other,
//! claiming the hub's role whenever they aren't the hub, and numbering what is pending whenever
//! they are.
//!
//! After every step, each store must answer as the model of what it holds says:
//! - its chain of terms is the one the claims it holds make;
//! - an event's number is its first, by epoch and number, among the runs of records that count
//!   whose last event the store holds; and its confirmed logs follow;
//! - it claims exactly when it isn't the hub and holds the whole chain and every record that
//!   counts, succeeding the winning claim and cutting each device on the chain where it holds its
//!   log to; and only the hub numbers;
//! - the hub numbers, for each device, the events it holds after the last a record that counts
//!   numbers, and its own after its own last record of its epoch, records aside, in the order it
//!   received them, from the number after its last; a record ends where a device's next event
//!   doesn't follow on from its run in it;
//! - no store counts an event under two numbers, or an epoch's numbers with a gap, and the hub
//!   finds no event unnumbered before one numbered.
//!
//! In the end every store takes everything, the hub numbers the rest, and every store confirms
//! every event of every log.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU8;
use std::collections::BTreeMap;

use keel_domain::hub::{Claim, ClaimError, Claimed, HubEvent, Term, chain};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, SequenceEvent};
use keel_events::envelope::Device;
use keel_events::event::SignedEvent;
use keel_events::hash::EventHash;
use keel_events::keys::SoftwareSigner;
use keel_store::{Received, Store, StoreError, StoreSeq};
use keel_types::{Id, SeededEntropy};
use proptest::prelude::*;
use support::{OWN, PEERS, Scratch, at, config_of, device, draft, registry, signer, store_key};

type TestStore = Store<SoftwareSigner, SeededEntropy>;

/// The devices, each with a store: all can be hub.
const DEVICES: [u8; 3] = [OWN, PEERS[0], PEERS[1]];

/// What a log holds at a position.
#[derive(Clone, Debug)]
enum Entry {
    Event,
    Claim(Claimed),
    Record(Assigned),
}

fn entry(event: &SignedEvent) -> Entry {
    let body = event.body();
    if let Ok(HubEvent::Claimed(claimed)) = HubEvent::decode(&body.schema, &body.payload) {
        return Entry::Claim(claimed);
    }
    if let Ok(SequenceEvent::Assigned(record)) = SequenceEvent::decode(&body.schema, &body.payload)
    {
        return Entry::Record(record);
    }
    Entry::Event
}

/// The location, by the model: each device's log, and what each store holds and received, in
/// order.
struct Location {
    stores: Vec<TestStore>,
    priorities: Vec<NonZeroU8>,
    logs: Vec<Vec<SignedEvent>>,
    entries: Vec<Vec<Entry>>,
    /// `held[d][x]`: how far into device `x`'s log store `d` holds.
    held: Vec<Vec<usize>>,
    /// The events each store holds, in the order it stored them.
    arrivals: Vec<Vec<(usize, usize)>>,
}

/// A record that counts in a store's view: its author, position and record.
type Counting<'a> = (usize, usize, &'a Assigned);

/// A run, by the model: its device, first and last positions, and its last event's hash.
type Span = (usize, u64, u64, EventHash);

/// A record, by the model: its first number and its runs.
type Planned = (u64, Vec<Span>);

impl Location {
    fn index_of(device_id: Id<Device>) -> usize {
        DEVICES.iter().position(|&n| device(n) == device_id).unwrap()
    }

    /// Notes events store `d` wrote of its own.
    fn wrote(&mut self, d: usize, events: &[SignedEvent]) {
        for event in events {
            self.logs[d].push(event.clone());
            self.entries[d].push(entry(event));
            self.held[d][d] = self.logs[d].len();
            self.arrivals[d].push((d, self.logs[d].len()));
        }
    }

    /// The claims store `d` holds.
    fn claims(&self, d: usize) -> Vec<Claim> {
        let mut claims = Vec::new();
        for (x, &n) in DEVICES.iter().enumerate() {
            for (index, held) in self.entries[x][..self.held[d][x]].iter().enumerate() {
                if let Entry::Claim(claimed) = held {
                    claims.push(Claim {
                        event: self.logs[x][index].body().event_id,
                        device: device(n),
                        position: u64::try_from(index + 1).unwrap(),
                        claimed: claimed.clone(),
                    });
                }
            }
        }
        claims
    }

    /// The records that count in store `d`'s view, given its chain.
    fn counting(&self, d: usize, chain: &[Term]) -> Vec<Counting<'_>> {
        let mut counting = Vec::new();
        for term in chain {
            let x = Location::index_of(term.device);
            for (index, held) in self.entries[x][..self.held[d][x]].iter().enumerate() {
                if let Entry::Record(record) = held
                    && record.epoch == term.epoch.get()
                    && term.counts(u64::try_from(index + 1).unwrap())
                {
                    counting.push((x, index + 1, record));
                }
            }
        }
        counting
    }

    fn is_record(&self, x: usize, position: usize) -> bool {
        matches!(self.entries[x][position - 1], Entry::Record(_))
    }

    /// Each event store `d` holds, records aside, with its first number among the runs that
    /// count whose last event it holds: the store's numbers, by the model.
    fn numbers(&self, d: usize) -> BTreeMap<(usize, usize), Option<StoreSeq>> {
        let chain = chain(&self.claims(d));
        let mut numbers: BTreeMap<(usize, usize), Option<StoreSeq>> = BTreeMap::new();
        for x in 0..DEVICES.len() {
            for position in 1..=self.held[d][x] {
                if !self.is_record(x, position) {
                    numbers.insert((x, position), None);
                }
            }
        }
        for (_, _, record) in self.counting(d, &chain) {
            for (first, run) in record.numbered() {
                let x = Location::index_of(run.device);
                let to = usize::try_from(run.to).unwrap();
                if self.held[d][x] < to || self.logs[x][to - 1].hash() != run.last {
                    continue;
                }
                for position in run.from..=run.to {
                    let number =
                        StoreSeq { epoch: record.epoch, number: first + (position - run.from) };
                    let slot = numbers.get_mut(&(x, usize::try_from(position).unwrap())).unwrap();
                    if slot.is_none_or(|earlier| {
                        (number.epoch, number.number) < (earlier.epoch, earlier.number)
                    }) {
                        *slot = Some(number);
                    }
                }
            }
        }
        numbers
    }

    /// The records hub `d` writes next, by the model: the runs of each, with first numbers.
    fn pending_records(&self, d: usize, term: &Term) -> Vec<Planned> {
        let chain = chain(&self.claims(d));
        let counting = self.counting(d, &chain);
        let mut after = vec![0_u64; DEVICES.len()];
        let mut next = 1;
        for &(author, position, record) in &counting {
            for run in &record.runs {
                let x = Location::index_of(run.device);
                after[x] = after[x].max(run.to);
            }
            if author == d && record.epoch == term.epoch.get() {
                after[d] = after[d].max(u64::try_from(position).unwrap());
                next = next.max(record.last() + 1);
            }
        }
        let mut records: Vec<Planned> = Vec::new();
        let mut runs: Vec<Span> = Vec::new();
        let mut first = next;
        for &(x, position) in &self.arrivals[d] {
            let at = u64::try_from(position).unwrap();
            if at <= after[x] || self.is_record(x, position) {
                continue;
            }
            let hash = self.logs[x][position - 1].hash();
            match runs.last_mut() {
                Some(run) if run.0 == x && run.2 + 1 == at => {
                    run.2 = at;
                    run.3 = hash;
                }
                _ => {
                    let before = runs.iter().rev().find(|run| run.0 == x);
                    if before.is_some_and(|run| run.2 + 1 != at) {
                        let count: u64 = runs.iter().map(|run| run.2 - run.1 + 1).sum();
                        records.push((first, core::mem::take(&mut runs)));
                        first += count;
                    }
                    runs.push((x, at, at, hash));
                }
            }
        }
        if !runs.is_empty() {
            records.push((first, runs));
        }
        records
    }

    /// Checks store `d` against the model.
    fn check(&self, d: usize, when: &str) -> Result<(), TestCaseError> {
        let store = &self.stores[d];
        let chain = chain(&self.claims(d));
        prop_assert_eq!(store.terms().unwrap(), chain.clone(), "{} store {}", when, d);
        let numbers = self.numbers(d);
        for (&(x, position), &number) in &numbers {
            let found =
                store.store_seq(device(DEVICES[x]), u64::try_from(position).unwrap()).unwrap();
            prop_assert_eq!(found, number, "{} store {} event {:?}", when, d, (x, position));
        }
        // Confirmed: for each device, the longest start of its log each event of which is
        // numbered or a record.
        let mut confirmed = BTreeMap::new();
        for x in 0..DEVICES.len() {
            let reach = (1..=self.held[d][x])
                .take_while(|&position| {
                    self.is_record(x, position) || numbers[&(x, position)].is_some()
                })
                .count();
            if reach > 0 {
                confirmed.insert(device(DEVICES[x]), u64::try_from(reach).unwrap());
            }
        }
        prop_assert_eq!(store.confirmed().unwrap(), confirmed, "{} store {}", when, d);
        // No event counts under two numbers, and each epoch's numbers that count run from 1.
        let mut given: BTreeMap<(usize, u64), Vec<u64>> = BTreeMap::new();
        let mut counted: BTreeMap<(usize, u64), usize> = BTreeMap::new();
        for (_, _, record) in self.counting(d, &chain) {
            for (first, run) in record.numbered() {
                let x = Location::index_of(run.device);
                for position in run.from..=run.to {
                    *counted.entry((x, position)).or_default() += 1;
                    given.entry((0, record.epoch)).or_default().push(first + (position - run.from));
                }
            }
        }
        prop_assert!(
            counted.values().all(|&count| count == 1),
            "{} store {}: {:?}",
            when,
            d,
            counted
        );
        for ((_, epoch), mut numbers) in given {
            numbers.sort_unstable();
            let expected: Vec<u64> = (1..=u64::try_from(numbers.len()).unwrap()).collect();
            prop_assert_eq!(numbers, expected, "{} store {} epoch {}", when, d, epoch);
        }
        // The hub finds no event unnumbered before one that counts.
        if chain.first().is_some_and(|term| term.device == device(DEVICES[d])) {
            for x in 0..DEVICES.len() {
                let last = counted.keys().filter(|(of, _)| *of == x).map(|&(_, at)| at).max();
                let reach = usize::try_from(last.unwrap_or(0)).unwrap().min(self.held[d][x]);
                for position in 1..=reach {
                    let numbered = counted.contains_key(&(x, u64::try_from(position).unwrap()));
                    prop_assert!(
                        numbered || self.is_record(x, position),
                        "{} hub {} finds {:?} unnumbered",
                        when,
                        d,
                        (x, position)
                    );
                }
            }
        }
        Ok(())
    }

    fn append(&mut self, d: usize, now: i64) {
        let note = u64::try_from(self.logs[d].len()).unwrap();
        let event = self.stores[d].write(|w| w.append(draft(1, note), at(now))).unwrap();
        self.wrote(d, &[event]);
    }

    /// Store `to` takes from store `from` up to `most` more events of device `of`'s log.
    fn replicate(&mut self, to: usize, from: usize, of: usize, most: usize, now: i64) {
        if to == of {
            return;
        }
        let start = self.held[to][of];
        let end = self.held[from][of].min(start.saturating_add(most));
        if end <= start {
            return;
        }
        let events = &self.logs[of][start..end];
        self.stores[to]
            .write(|w| {
                for event in events {
                    let received = w.receive(&event.to_bytes(), &registry(), at(now))?;
                    assert!(matches!(received, Received::Stored(_)), "{received:?}");
                }
                Ok::<_, StoreError>(())
            })
            .unwrap();
        self.held[to][of] = end;
        self.arrivals[to].extend((start + 1..=end).map(|position| (of, position)));
    }

    /// Store `d` claims the role, as the model says it may.
    fn claim(&mut self, d: usize, now: i64) -> Result<(), TestCaseError> {
        let terms = chain(&self.claims(d));
        let held = |x: Id<Device>| u64::try_from(self.held[d][Location::index_of(x)]).unwrap();
        let hub = terms.first().is_some_and(|term| term.device == device(DEVICES[d]));
        let expected = Claimed::succeeding(&terms, self.priorities[d], held);
        let claimed = self.stores[d].claim(self.priorities[d], at(now));
        match (claimed, expected) {
            (Ok(None), _) => prop_assert!(hub, "store {} claimed nothing", d),
            (Ok(Some(event)), Ok(expected)) => {
                prop_assert!(!hub);
                let Entry::Claim(made) = entry(&event) else { panic!("not a claim") };
                prop_assert_eq!(made, expected);
                self.wrote(d, &[event]);
            }
            (Err(StoreError::Behind), Err(ClaimError::Behind)) => prop_assert!(!hub),
            (claimed, expected) => {
                prop_assert!(false, "store {} claimed {:?}, not {:?}", d, claimed, expected);
            }
        }
        Ok(())
    }

    /// Each store that is the hub numbers what is pending, as the model says; the others refuse.
    fn sequence(&mut self, now: i64) -> Result<(), TestCaseError> {
        for (d, &n) in DEVICES.iter().enumerate() {
            let chain = chain(&self.claims(d));
            let hub = chain.first().filter(|term| term.device == device(n)).copied();
            let Some(term) = hub else {
                prop_assert!(matches!(self.stores[d].sequence(at(now)), Err(StoreError::NotHub)));
                continue;
            };
            let expected = self.pending_records(d, &term);
            let written = self.stores[d].sequence(at(now)).unwrap();
            prop_assert_eq!(written.len(), expected.len(), "store {}", d);
            for (event, (first, runs)) in written.iter().zip(&expected) {
                let Entry::Record(record) = entry(event) else { panic!("not a record") };
                prop_assert_eq!(record.epoch, term.epoch.get());
                prop_assert_eq!(record.first, *first);
                let spans: Vec<Span> = record
                    .runs
                    .iter()
                    .map(|run| (Location::index_of(run.device), run.from, run.to, run.last))
                    .collect();
                prop_assert_eq!(&spans, runs, "store {}", d);
            }
            self.wrote(d, &written);
        }
        Ok(())
    }
}

/// What happens next.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// A store appends an event.
    Append(usize),
    /// A store takes more of a log from another.
    Replicate { to: usize, from: usize, of: usize, most: usize },
    /// A store claims the hub's role.
    Claim(usize),
    /// Each hub numbers what is pending.
    Sequence,
}

fn any_step() -> impl Strategy<Value = Step> {
    let most = prop_oneof![3 => 1_usize..=2, 1 => Just(usize::MAX)];
    prop_oneof![
        3 => (0_usize..3).prop_map(Step::Append),
        6 => (0_usize..3, 0_usize..3, 0_usize..3, most)
            .prop_map(|(to, from, of, most)| Step::Replicate { to, from, of, most }),
        2 => (0_usize..3).prop_map(Step::Claim),
        3 => Just(Step::Sequence),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Every store claims, numbers and confirms as the model of what it holds says.
    #[test]
    fn stores_claim_number_and_confirm_as_the_model_does(
        priorities in [1_u8..=2, 1_u8..=2, 1_u8..=2],
        steps in prop::collection::vec(any_step(), 1..40),
    ) {
        let scratches: Vec<Scratch> = (0..3).map(|n| Scratch::new(&format!("terms-{n}"))).collect();
        let stores = DEVICES
            .iter()
            .zip(&scratches)
            .map(|(&n, scratch)| {
                Store::open(scratch.db(), store_key(), config_of(n), signer(n), SeededEntropy::new(u64::from(n))).unwrap()
            })
            .collect();
        let mut location = Location {
            stores,
            priorities: priorities.iter().map(|&n| NonZeroU8::new(n).unwrap()).collect(),
            logs: vec![Vec::new(); 3],
            entries: vec![Vec::new(); 3],
            held: vec![vec![0; 3]; 3],
            arrivals: vec![Vec::new(); 3],
        };
        for (i, step) in steps.iter().enumerate() {
            let now = 1_000 + i64::try_from(i).unwrap() * 100;
            match *step {
                Step::Append(d) => location.append(d, now),
                Step::Replicate { to, from, of, most } => location.replicate(to, from, of, most, now),
                Step::Claim(d) => location.claim(d, now)?,
                Step::Sequence => location.sequence(now)?,
            }
            for d in 0..3 {
                location.check(d, &format!("after {step:?}"))?;
            }
        }
        // Every store takes everything; the hub, or the first store, if none is, numbers the
        // rest; and every store takes that.
        let everything = |location: &mut Location, now: i64| {
            for to in 0..3 {
                for of in 0..3 {
                    location.replicate(to, of, of, usize::MAX, now);
                }
            }
        };
        everything(&mut location, 50_000);
        if (0..3).all(|d| location.stores[d].term().unwrap().is_none_or(|term| term.device != device(DEVICES[d]))) {
            location.claim(0, 50_100)?;
            everything(&mut location, 50_200);
        }
        location.sequence(50_300)?;
        everything(&mut location, 50_400);
        for d in 0..3 {
            location.check(d, "in the end")?;
            let numbers = location.numbers(d);
            prop_assert!(numbers.values().all(Option::is_some), "store {}: {:?}", d, numbers);
            let confirmed: BTreeMap<Id<Device>, u64> = (0..3)
                .filter(|&x| !location.logs[x].is_empty())
                .map(|x| (device(DEVICES[x]), u64::try_from(location.logs[x].len()).unwrap()))
                .collect();
            prop_assert_eq!(location.stores[d].confirmed().unwrap(), confirmed);
            prop_assert!(location.stores[d].check().unwrap().is_empty());
        }
    }
}
