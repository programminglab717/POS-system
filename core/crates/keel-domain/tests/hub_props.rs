//! Property tests for hub terms (ADR-0022).
//!
//! - **The rules.** The chain, and each term's cut, that any set of claims makes, well formed or
//!   not, match a model that follows the documented rules literally, whatever order the claims
//!   come in; so does which claim beats which. Epochs follow on up to the largest.
//! - **What the rules are for.** A model of a location: devices appending events, replicating
//!   any part of any log to each other, claiming the hub's role whenever they aren't the hub and
//!   hold the whole chain and every record that counts on it, and numbering, as the hub, each
//!   device's events after the last one a record that counts numbers. At every step: no replica
//!   counts an event under two numbers, or a term's numbers with a gap; no hub finds an event it
//!   holds that no record that counts numbers, before one that a record that counts does; and no
//!   claim changes what its claimant counts. Once every replica holds everything and the hub
//!   numbers what is left, every event but the records counts under exactly one number.

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

use keel_domain::codec::PayloadError;
use keel_domain::hub::{Claim, ClaimError, Claimed, Cut, Epoch, Succession, Term, chain};
use keel_events::envelope::{Device, Event};
use keel_types::Id;
use proptest::prelude::*;

/// The largest epoch: 2^63 − 1.
const LARGEST: u64 = (1 << 63) - 1;

fn device(n: usize) -> Id<Device> {
    support::id(u64::try_from(n).unwrap() + 1)
}

/// The identifier of the event at `position` of device `n`'s log.
fn event_at(n: usize, position: usize) -> Id<Event> {
    support::id(0x10_0000 * (u64::try_from(n).unwrap() + 1) + u64::try_from(position).unwrap())
}

// ---------------------------------------------------------------------------------------------
// The rules, literally.

/// How `a` ranks against `b`: `Less` when it wins. The higher epoch wins, then the higher
/// priority, then the lower device identifier, in byte order, then the earlier position in its
/// log.
fn model_rank(a: &Claim, b: &Claim) -> core::cmp::Ordering {
    b.claimed
        .epoch
        .cmp(&a.claimed.epoch)
        .then(b.claimed.priority.cmp(&a.claimed.priority))
        .then(a.device.to_bytes().cmp(&b.device.to_bytes()))
        .then(a.position.cmp(&b.position))
}

/// The chain `claims` make, by the documented rules, step by step.
fn model_chain(claims: &[Claim]) -> Vec<Term> {
    let mut ranked: Vec<&Claim> = claims.iter().collect();
    ranked.sort_by(|a, b| model_rank(a, b));
    let Some(&winning) = ranked.first() else { return Vec::new() };
    // The chain: each claim the one after it succeeds, while the replica holds it, and of a
    // lower epoch.
    let mut links = vec![winning];
    loop {
        let last = links.last().unwrap();
        let Some(succession) = &last.claimed.succeeds else { break };
        let Some(previous) = claims.iter().find(|claim| claim.event == succession.previous) else {
            break;
        };
        if previous.claimed.epoch >= last.claimed.epoch {
            break;
        }
        links.push(previous);
    }
    // Each term's cut: the least any later claim on the chain gives of its device, 0 where one
    // gives none.
    links
        .iter()
        .enumerate()
        .map(|(index, link)| Term {
            epoch: link.claimed.epoch,
            device: link.device,
            claim: link.event,
            after: link.position,
            through: links[..index]
                .iter()
                .map(|later| {
                    let cuts = &later.claimed.succeeds.as_ref().unwrap().cuts;
                    cuts.iter().find(|cut| cut.device == link.device).map_or(0, |cut| cut.position)
                })
                .min(),
        })
        .collect()
}

/// Any set of claims, each by one of four devices at a position of its own, of epochs 1 to 6,
/// succeeding another of the set or a claim not in it, and cutting any of the four devices
/// anywhere.
fn any_claims() -> impl Strategy<Value = Vec<Claim>> {
    let claim = (
        0_usize..4,
        1_u64..=6,
        1_u8..=3,
        (any::<prop::sample::Index>(), prop::bool::weighted(0.8)),
        prop::collection::btree_map(0_usize..4, 1_u64..=25, 1..=4),
    );
    prop::collection::vec(claim, 0..=8).prop_map(|drawn| {
        let count = drawn.len();
        drawn
            .iter()
            .enumerate()
            .map(|(k, (n, epoch, priority, (previous, held), cuts))| {
                let epoch = Epoch::new(*epoch).unwrap();
                let succeeds = (epoch > Epoch::FIRST).then(|| Succession {
                    previous: if *held {
                        event_at(0, previous.index(count) + 1)
                    } else {
                        support::id(0xDEAD)
                    },
                    cuts: cuts
                        .iter()
                        .map(|(&n, &position)| Cut { device: device(n), position })
                        .collect(),
                });
                Claim {
                    // Every claim's event identifier is distinct, and so is its place in a log.
                    event: event_at(0, k + 1),
                    device: device(*n),
                    position: u64::try_from(k + 1).unwrap(),
                    claimed: Claimed::new(epoch, NonZeroU8::new(*priority).unwrap(), succeeds)
                        .unwrap(),
                }
            })
            .collect()
    })
}

/// The claim a replica holding `held[n]` of each device `n`'s log makes at `priority`,
/// succeeding `chain`, by the documented rules: refused unless the chain is whole and the replica
/// holds each term's claim and its device's log to the term's cut.
fn model_succeeding(
    chain: &[Term],
    priority: NonZeroU8,
    held: &[u64],
) -> Result<Claimed, ClaimError> {
    let held_of = |of: Id<Device>| held[(0..held.len()).find(|&n| device(n) == of).unwrap()];
    let Some(winning) = chain.first() else {
        return Ok(Claimed::new(Epoch::FIRST, priority, None).unwrap());
    };
    let whole = chain.last().unwrap().epoch == Epoch::FIRST;
    let holds = chain.iter().all(|term| {
        let held = held_of(term.device);
        held >= term.after && term.through.is_none_or(|through| held >= through)
    });
    if !whole || !holds {
        return Err(ClaimError::Behind);
    }
    let mut devices: Vec<Id<Device>> = chain.iter().map(|term| term.device).collect();
    devices.sort_by_key(|device| device.to_bytes());
    devices.dedup();
    let cuts = devices.into_iter().map(|device| Cut { device, position: held_of(device) });
    let epoch = winning.epoch.next().ok_or(PayloadError::Invalid("epoch"))?;
    let succession = Succession { previous: winning.claim, cuts: cuts.collect() };
    Ok(Claimed::new(epoch, priority, Some(succession))?)
}

/// What a replica could hold of each of the four devices' logs, given the chain: around each
/// term's claim and cut, or anywhere.
fn any_held(chain: &[Term], picks: &[(prop::sample::Index, u64)]) -> Vec<u64> {
    (0..4)
        .map(|n| {
            let mut near = vec![0];
            for term in chain.iter().filter(|term| term.device == device(n)) {
                near.extend([term.after - 1, term.after, term.after + 1]);
                if let Some(through) = term.through {
                    near.extend([through.saturating_sub(1), through, through + 1]);
                }
            }
            let (pick, anywhere) = picks[n];
            near.push(anywhere);
            near[pick.index(near.len())]
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// A location.

/// What a log holds at a position.
#[derive(Clone, Debug)]
enum Entry {
    /// An ordinary event.
    Event,
    /// A claim of the hub's role.
    Claim(Claimed),
    /// A sequencing record: its epoch, and the events it numbers, in order, from its first
    /// number on.
    Record { epoch: Epoch, first: u64, numbers: Vec<(usize, usize)> },
}

/// Devices, their logs, and how much of each other's logs each holds.
#[derive(Clone, Debug)]
struct Location {
    /// Each device's priority as hub: none for a device that can never be.
    priorities: Vec<Option<NonZeroU8>>,
    logs: Vec<Vec<Entry>>,
    /// `held[d][x]`: how far into device `x`'s log device `d` holds.
    held: Vec<Vec<usize>>,
}

/// A record that counts: its author and position, its term's epoch, its first number, and what
/// it numbers.
type Counting<'a> = (usize, usize, Epoch, u64, &'a [(usize, usize)]);

impl Location {
    fn new(priorities: Vec<Option<NonZeroU8>>) -> Location {
        let count = priorities.len();
        Location { priorities, logs: vec![Vec::new(); count], held: vec![vec![0; count]; count] }
    }

    fn devices(&self) -> usize {
        self.logs.len()
    }

    fn index_of(&self, of: Id<Device>) -> usize {
        (0..self.devices()).find(|&n| device(n) == of).unwrap()
    }

    fn append(&mut self, d: usize, entry: Entry) {
        self.logs[d].push(entry);
        self.held[d][d] = self.logs[d].len();
    }

    /// The claims device `d` holds.
    fn claims(&self, d: usize) -> Vec<Claim> {
        let mut claims = Vec::new();
        for (x, log) in self.logs.iter().enumerate() {
            for (index, entry) in log[..self.held[d][x]].iter().enumerate() {
                if let Entry::Claim(claimed) = entry {
                    claims.push(Claim {
                        event: event_at(x, index + 1),
                        device: device(x),
                        position: u64::try_from(index + 1).unwrap(),
                        claimed: claimed.clone(),
                    });
                }
            }
        }
        claims
    }

    /// The records that count in device `d`'s view, given its chain.
    fn counting(&self, d: usize, chain: &[Term]) -> Vec<Counting<'_>> {
        let mut counting = Vec::new();
        for term in chain {
            let x = self.index_of(term.device);
            for (index, entry) in self.logs[x][..self.held[d][x]].iter().enumerate() {
                if let Entry::Record { epoch, first, numbers } = entry
                    && *epoch == term.epoch
                    && term.counts(u64::try_from(index + 1).unwrap())
                {
                    counting.push((x, index + 1, *epoch, *first, numbers.as_slice()));
                }
            }
        }
        counting
    }

    /// Whether device `d` is the hub in its own view: it holds the winning claim.
    fn is_hub(&self, d: usize) -> bool {
        chain(&self.claims(d)).first().is_some_and(|term| term.device == device(d))
    }

    /// Device `d` claims the hub's role, if it can be hub, isn't, and holds the whole chain and
    /// every record that counts on it. Checks that its claim changes nothing it counts: a claim
    /// fences only what its claimant didn't hold.
    fn claim(&mut self, d: usize) -> Result<(), TestCaseError> {
        let Some(priority) = self.priorities[d] else { return Ok(()) };
        let terms = chain(&self.claims(d));
        if terms.first().is_some_and(|term| term.device == device(d)) {
            return Ok(());
        }
        let held = |x: Id<Device>| u64::try_from(self.held[d][self.index_of(x)]).unwrap();
        let claimed = match Claimed::succeeding(&terms, priority, held) {
            Ok(claimed) => claimed,
            Err(ClaimError::Behind) => return Ok(()),
            Err(error) => panic!("device {d} can't claim: {error}"),
        };
        let before = format!("{:?}", self.counting(d, &terms));
        self.append(d, Entry::Claim(claimed));
        let after = format!("{:?}", self.counting(d, &chain(&self.claims(d))));
        prop_assert_eq!(before, after, "device {}'s claim changed what it counts", d);
        Ok(())
    }

    /// What device `d` numbers next, as the hub in `epoch`: each device's events it holds after
    /// the last one a record that counts numbers, except records, device by device. Its own log
    /// is pending only after its own last record of the epoch, too, which numbered all before it
    /// with the records it wrote with it. (Not after its own last record that counts: a later
    /// claim may have cut off a record written with it.)
    fn pending(&self, d: usize, epoch: Epoch, counting: &[Counting<'_>]) -> Vec<(usize, usize)> {
        let mut after = vec![0; self.devices()];
        for &(author, position, of, _, numbers) in counting {
            for &(x, numbered) in numbers {
                after[x] = after[x].max(numbered);
            }
            if author == d && of == epoch {
                after[d] = after[d].max(position);
            }
        }
        (0..self.devices())
            .flat_map(|x| ((after[x] + 1)..=self.held[d][x]).map(move |position| (x, position)))
            .filter(|&(x, position)| !matches!(self.logs[x][position - 1], Entry::Record { .. }))
            .collect()
    }

    /// Every device that is the hub in its own view numbers what is pending, in records of up
    /// to `most` events each, one after another.
    fn sequence(&mut self, most: usize) {
        for d in 0..self.devices() {
            let chain = chain(&self.claims(d));
            let Some(term) = chain.first().filter(|term| term.device == device(d)) else {
                continue;
            };
            let counting = self.counting(d, &chain);
            let mut next = counting
                .iter()
                .filter(|(_, _, epoch, _, _)| *epoch == term.epoch)
                .map(|(_, _, _, first, numbers)| first + u64::try_from(numbers.len()).unwrap())
                .max()
                .unwrap_or(1);
            let pending = self.pending(d, term.epoch, &counting);
            for numbers in pending.chunks(most) {
                let (epoch, first) = (term.epoch, next);
                next += u64::try_from(numbers.len()).unwrap();
                self.append(d, Entry::Record { epoch, first, numbers: numbers.to_vec() });
            }
        }
    }

    /// Device `d` takes from device `from` up to `most` more events of device `x`'s log.
    fn replicate(&mut self, d: usize, from: usize, x: usize, most: usize) {
        if d != x {
            let reach = self.held[from][x].min(self.held[d][x].saturating_add(most));
            self.held[d][x] = self.held[d][x].max(reach);
        }
    }

    /// Checks, in device `d`'s view, that no event counts under two numbers, and that each
    /// term's numbers that count run from 1 without a gap; and, if `d` is the hub, that every
    /// event it holds before one a record that counts numbers is numbered too, but records.
    fn check(&self, d: usize) -> Result<(), TestCaseError> {
        let chain = chain(&self.claims(d));
        let counting = self.counting(d, &chain);
        let mut numbered: BTreeMap<(usize, usize), (Epoch, u64)> = BTreeMap::new();
        let mut numbers: BTreeMap<Epoch, Vec<u64>> = BTreeMap::new();
        for &(_, _, epoch, first, events) in &counting {
            for (offset, &event) in events.iter().enumerate() {
                let number = first + u64::try_from(offset).unwrap();
                let earlier = numbered.insert(event, (epoch, number));
                prop_assert!(
                    earlier.is_none(),
                    "device {} counts event {:?} under {:?} and {:?}",
                    d,
                    event,
                    earlier,
                    (epoch, number)
                );
                numbers.entry(epoch).or_default().push(number);
            }
        }
        for (epoch, mut given) in numbers {
            given.sort_unstable();
            let expected: Vec<u64> = (1..=u64::try_from(given.len()).unwrap()).collect();
            prop_assert_eq!(given, expected, "device {}'s view of epoch {:?}", d, epoch);
        }
        if chain.first().is_some_and(|term| term.device == device(d)) {
            for x in 0..self.devices() {
                let last = numbered.keys().filter(|(of, _)| *of == x).map(|&(_, at)| at).max();
                let reach = last.unwrap_or(0).min(self.held[d][x]);
                for position in 1..=reach {
                    let record = matches!(self.logs[x][position - 1], Entry::Record { .. });
                    prop_assert!(
                        record || numbered.contains_key(&(x, position)),
                        "hub {} finds event {:?} unnumbered before {:?}",
                        d,
                        (x, position),
                        last
                    );
                }
            }
        }
        Ok(())
    }
}

/// What happens next at the location.
#[derive(Clone, Debug)]
enum Step {
    /// A device appends an event.
    Append(usize),
    /// A device takes more of a log from another.
    Replicate { to: usize, from: usize, of: usize, most: usize },
    /// A device takes everything another holds.
    Catch { to: usize, from: usize },
    /// A device claims the hub's role.
    Claim(usize),
    /// Each hub numbers what is pending, in records of up to `most` events.
    Sequence { most: usize },
}

/// Three or four devices, at least two of which can be hub, at priorities that may tie, in any
/// order of their identifiers.
fn any_priorities() -> impl Strategy<Value = Vec<Option<NonZeroU8>>> {
    let eligible = || (1_u8..=2).prop_map(NonZeroU8::new);
    let maybe = prop_oneof![3 => (1_u8..=2).prop_map(NonZeroU8::new), 1 => Just(None)];
    let others = prop::collection::vec(maybe, 1..=2);
    (eligible(), eligible(), others, any::<prop::sample::Index>()).prop_map(
        |(first, second, others, turn)| {
            let mut priorities = vec![first, second];
            priorities.extend(others);
            let by = turn.index(priorities.len());
            priorities.rotate_left(by);
            priorities
        },
    )
}

/// Steps at a location of `devices` devices: mostly replicating a little of one log at a time,
/// so that replicas often hold a claim but not all that comes before or after it.
fn any_steps(devices: usize) -> impl Strategy<Value = Vec<Step>> {
    let most = || prop_oneof![3 => 1_usize..=2, 1 => Just(usize::MAX)];
    let step = prop_oneof![
        3 => (0..devices).prop_map(Step::Append),
        5 => (0..devices, 0..devices, 0..devices, most())
            .prop_map(|(to, from, of, most)| Step::Replicate { to, from, of, most }),
        1 => (0..devices, 0..devices).prop_map(|(to, from)| Step::Catch { to, from }),
        3 => (0..devices).prop_map(Step::Claim),
        3 => (1_usize..=3).prop_map(|most| Step::Sequence { most }),
    ];
    prop::collection::vec(step, 1..=80)
}

/// Runs `steps` at a location of devices of `priorities`, checking every replica after each;
/// then lets every replica take everything, and the hub, or the first that can be, number the
/// rest; and checks that every event but the records counts under exactly one number.
fn run(priorities: Vec<Option<NonZeroU8>>, steps: Vec<Step>) -> Result<(), TestCaseError> {
    let mut location = Location::new(priorities);
    let devices = location.devices();
    for step in steps {
        match step {
            Step::Append(d) => location.append(d, Entry::Event),
            Step::Replicate { to, from, of, most } => location.replicate(to, from, of, most),
            Step::Catch { to, from } => {
                for of in 0..devices {
                    location.replicate(to, from, of, usize::MAX);
                }
            }
            Step::Claim(d) => location.claim(d)?,
            Step::Sequence { most } => location.sequence(most),
        }
        for d in 0..devices {
            location.check(d)?;
        }
    }
    for to in 0..devices {
        for of in 0..devices {
            location.replicate(to, of, of, usize::MAX);
        }
    }
    if !(0..devices).any(|d| location.is_hub(d)) {
        let first = (0..devices).find(|&d| location.priorities[d].is_some()).unwrap();
        location.claim(first)?;
        for to in 0..devices {
            location.replicate(to, first, first, usize::MAX);
        }
    }
    location.sequence(usize::MAX);
    let hub = (0..devices).find(|&d| location.is_hub(d)).unwrap();
    for to in 0..devices {
        location.replicate(to, hub, hub, usize::MAX);
    }
    for d in 0..devices {
        location.check(d)?;
        let chain = chain(&location.claims(d));
        let counted: usize =
            location.counting(d, &chain).iter().map(|counting| counting.4.len()).sum();
        let events = location
            .logs
            .iter()
            .flatten()
            .filter(|entry| !matches!(entry, Entry::Record { .. }))
            .count();
        prop_assert_eq!(counted, events, "device {}", d);
    }
    Ok(())
}

fn any_location() -> impl Strategy<Value = (Vec<Option<NonZeroU8>>, Vec<Step>)> {
    any_priorities().prop_flat_map(|priorities| {
        let devices = priorities.len();
        (Just(priorities), any_steps(devices))
    })
}

proptest! {
    /// The chain and its cuts are as the rules say, whatever order the claims come in, and so
    /// is which claim beats which.
    #[test]
    fn the_chain_is_as_the_rules_say(
        claims in any_claims(),
        order in any::<prop::sample::Index>(),
    ) {
        let expected = model_chain(&claims);
        prop_assert_eq!(chain(&claims), expected.clone());
        for a in &claims {
            for b in &claims {
                prop_assert_eq!(a.beats(b), model_rank(a, b).is_lt(), "{:?} against {:?}", a, b);
            }
        }
        let mut rotated = claims.clone();
        rotated.reverse();
        if !rotated.is_empty() {
            let by = order.index(rotated.len());
            rotated.rotate_left(by);
        }
        prop_assert_eq!(chain(&rotated), expected.clone());
        // A term counts a record just when it follows the claim and lies within the cut.
        for term in &expected {
            for position in [0, 1, term.after, term.after + 1, 24, 25, 26] {
                let within = term.through.is_none_or(|through| position <= through);
                prop_assert_eq!(term.counts(position), position > term.after && within);
            }
        }
    }

    /// A replica claims exactly when it holds the whole chain, each claim on it, and every record
    /// that counts, and then claims as the rules say.
    #[test]
    fn a_replica_claims_as_the_rules_say_and_only_when_it_holds_what_counts(
        claims in any_claims(),
        priority in (1_u8..=255).prop_map(|n| NonZeroU8::new(n).unwrap()),
        picks in prop::collection::vec((any::<prop::sample::Index>(), 0_u64..=30), 4),
    ) {
        let chain = chain(&claims);
        let held = any_held(&chain, &picks);
        let of = |device: Id<Device>| held[(0..4).find(|&n| self::device(n) == device).unwrap()];
        prop_assert_eq!(
            Claimed::succeeding(&chain, priority, of),
            model_succeeding(&chain, priority, &held)
        );
    }

    /// Each epoch is followed by the next, up to the largest, which is followed by none.
    #[test]
    fn epochs_follow_on_up_to_the_largest(
        n in prop_oneof![1_u64..=3, (LARGEST - 3)..=LARGEST, 1..=LARGEST],
    ) {
        let next = Epoch::new(n).unwrap().next();
        prop_assert_eq!(next.map(Epoch::get), (n < LARGEST).then(|| n + 1));
    }

    /// No replica ever counts an event under two numbers, or a term's numbers with a gap; no hub
    /// finds a hole in what counts; no claim changes what its claimant counts; and in the end,
    /// every event counts under exactly one number.
    #[test]
    fn every_event_counts_under_one_number_at_most_and_in_the_end_exactly_one(
        (priorities, steps) in any_location(),
    ) {
        run(priorities, steps)?;
    }
}

/// A hub elected again numbers its own events that only a record a later claim cut off had
/// numbered. Device 1 numbers device 0's event and its own claim in two records, written
/// together; device 0 claims holding only the first; device 1 claims again. Its first claim,
/// numbered by its second record, which no longer counts, is pending again: device 1's own log
/// is pending after its last record of its current term, not after its last record that counts.
/// Found by the property above.
#[test]
fn a_hub_elected_again_numbers_what_only_a_record_of_its_cut_off_numbered() {
    let one = NonZeroU8::new(1);
    let steps = vec![
        Step::Append(0),
        Step::Claim(1),
        Step::Catch { to: 1, from: 0 },
        Step::Catch { to: 0, from: 1 },
        Step::Sequence { most: 1 },
        Step::Replicate { to: 0, from: 1, of: 1, most: 1 },
        Step::Claim(0),
        Step::Replicate { to: 1, from: 0, of: 0, most: 1 },
        Step::Claim(1),
    ];
    run(vec![one, one, one], steps).unwrap();
}
