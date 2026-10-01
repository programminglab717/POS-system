//! Hub terms (ADR-0022): how a replica takes its location's Store Hub role, which replica holds
//! it, and which hub's sequencing records count.
//!
//! A replica that becomes hub first records a claim in its own log. Each claim is a stream of its
//! own, of kind [`STREAM`], holding one `hub.claimed` event:
//!
//! | Schema | Keys |
//! |---|---|
//! | `hub.claimed` | 1 epoch, 2 priority, 3 previous, 4 cuts |
//!
//! - The **epoch** numbers the term, and the **priority** is the claimant's as hub.
//! - A claim of a later epoch than 1 succeeds the winning claim its claimant held: **previous** is
//!   that claim's event identifier.
//! - Its **cuts** say, for each device holding a term on the chain it succeeds, how far into the
//!   device's log the claimant held: each is `[device, position]`, a 16-byte identifier and a
//!   position.
//!
//! Beyond the types, a claim must satisfy these rules:
//! - the epoch and every position are from 1 to 2^63 − 1, and the priority from 1 to 255;
//! - a claim of epoch 1 has no previous claim and no cuts; a claim of a later epoch has both;
//! - it has from 1 to [`MAX_CUTS`] cuts, in ascending order of device, one for each.
//!
//! Every replica works out the same terms from the claims it holds ([`chain`]):
//! - The **winning claim** has the highest epoch; between claims of one epoch, the higher
//!   priority, then the lower device identifier, wins ([`Claim::beats`]). Its device is the hub.
//! - The **chain** runs from the winning claim to the claim it succeeds, and on, as far as the
//!   replica holds them, each of a lower epoch than the claim after it.
//! - Each claim on the chain begins a **term**. A sequencing record of the term's epoch, by the
//!   term's device, counts when it follows the claim in the device's log and lies within the
//!   term's **cut**: no further into the log than any later claim on the chain cuts the device. A
//!   later claim that doesn't cut the device held none of its log.
//!
//! A replica claims only when it holds the whole chain, back to the first claim, and every
//! record that counts on it ([`Claimed::succeeding`]). So its claim fences only what it didn't
//! hold: a hub's records that its successor didn't hold when it claimed never count, anywhere.
//! Every record that counts, every later claimant held, and counted, and numbered what came after
//! it: no event gets two numbers that count, each epoch's numbers that count run from 1 without a
//! gap, and the hub's numbers cover each device's log from its start without a gap.

use core::cmp::Reverse;
use core::num::NonZeroU8;
use std::collections::{BTreeMap, BTreeSet};

use keel_events::cbor::Value;
use keel_events::envelope::{Device, Event, SchemaRef};
use keel_types::Id;

use crate::codec::{Field, Fields, PayloadError, Record};
use crate::schema::{DecodeError, DomainEvent, SchemaId};
use crate::sequence::MAX_NUMBER;

#[cfg(test)]
mod tests;

/// The kind of stream a claim is.
pub const STREAM: &str = "hub";

/// The most cuts in one claim: the most devices that can hold terms on one chain.
pub const MAX_CUTS: usize = 1024;

const CLAIMED: SchemaId = SchemaId { name: "hub.claimed", version: 1 };

/// A hub's epoch: the number of its term, from 1 to 2^63 − 1, so that it fits the store. Each
/// claim takes the epoch after the winning claim its claimant held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(u64);

impl Epoch {
    /// The first epoch: that of a claim that succeeds none.
    pub const FIRST: Epoch = Epoch(1);

    /// The largest epoch.
    pub const MAX: Epoch = Epoch(MAX_NUMBER);

    /// The epoch numbered `n`, if it is from 1 to 2^63 − 1.
    pub const fn new(n: u64) -> Option<Epoch> {
        if n >= 1 && n <= MAX_NUMBER { Some(Epoch(n)) } else { None }
    }

    /// Its number.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The epoch after it: `None` after the largest.
    pub const fn next(self) -> Option<Epoch> {
        match self.0.checked_add(1) {
            Some(n) => Epoch::new(n),
            None => None,
        }
    }
}

impl Field for Epoch {
    fn to_value(&self) -> Value {
        Value::Unsigned(self.0)
    }

    fn from_value(value: &Value) -> Option<Epoch> {
        Epoch::new(value.as_u64()?)
    }
}

/// How far into a device's log a claimant held when it claimed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cut {
    /// The device.
    pub device: Id<Device>,
    /// The position of the last event of the device's log the claimant held.
    pub position: u64,
}

/// What a claim of a later epoch than 1 succeeds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Succession {
    /// The claim it succeeds: the winning claim its claimant held.
    pub previous: Id<Event>,
    /// For each device holding a term on the chain it succeeds, how far into its log the
    /// claimant held, in ascending order of device.
    pub cuts: Vec<Cut>,
}

/// Why a replica can't claim the hub's role.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ClaimError {
    /// The replica doesn't hold the whole chain, back to the first claim, or every record that
    /// counts on it. Its claim would cut what it doesn't hold: it must catch up first.
    #[error("the replica doesn't hold the whole chain of claims and every record that counts")]
    Behind,
    /// The claim would break a rule: the epochs have run out, or more devices hold terms on the
    /// chain than a claim can cut.
    #[error(transparent)]
    Invalid(#[from] PayloadError),
}

/// A replica claimed its location's hub role, for a term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claimed {
    /// The term's epoch.
    pub epoch: Epoch,
    /// The claimant's priority as hub: higher is preferred.
    pub priority: NonZeroU8,
    /// What it succeeds: `None` for a claim of epoch 1.
    pub succeeds: Option<Succession>,
}

impl Claimed {
    /// A claim, checked against the rules above.
    ///
    /// # Errors
    /// [`PayloadError::Invalid`] naming the rule the claim breaks.
    pub fn new(
        epoch: Epoch,
        priority: NonZeroU8,
        succeeds: Option<Succession>,
    ) -> Result<Claimed, PayloadError> {
        let claimed = Claimed { epoch, priority, succeeds };
        claimed.check()?;
        Ok(claimed)
    }

    /// The claim that succeeds the winning claim of `chain`, the chain a replica works out from
    /// the claims it holds ([`chain`]), by a claimant of `priority` that holds `held(device)`
    /// events of each device's log: the first claim, of epoch 1, when `chain` is empty. It cuts
    /// each of the chain's devices where the claimant holds its log to.
    ///
    /// # Errors
    /// [`ClaimError::Behind`] unless the chain is whole, back to a claim of epoch 1, and the
    /// claimant holds each term's claim, and its device's log as far as the term's cut.
    /// [`ClaimError::Invalid`] if the epochs have run out, or more devices hold terms on the chain
    /// than a claim can cut.
    pub fn succeeding(
        chain: &[Term],
        priority: NonZeroU8,
        held: impl Fn(Id<Device>) -> u64,
    ) -> Result<Claimed, ClaimError> {
        let (Some(winning), Some(first)) = (chain.first(), chain.last()) else {
            return Ok(Claimed::new(Epoch::FIRST, priority, None)?);
        };
        let holds_what_counts = chain.iter().all(|term| {
            let held = held(term.device);
            held >= term.after && term.through.is_none_or(|through| held >= through)
        });
        if first.epoch != Epoch::FIRST || !holds_what_counts {
            return Err(ClaimError::Behind);
        }
        let epoch = winning.epoch.next().ok_or(PayloadError::Invalid("epoch"))?;
        let devices: BTreeSet<Id<Device>> = chain.iter().map(|term| term.device).collect();
        let cuts = devices.into_iter().map(|device| Cut { device, position: held(device) });
        let succession = Succession { previous: winning.claim, cuts: cuts.collect() };
        Ok(Claimed::new(epoch, priority, Some(succession))?)
    }

    fn check(&self) -> Result<(), PayloadError> {
        let Some(succession) = &self.succeeds else {
            return if self.epoch == Epoch::FIRST {
                Ok(())
            } else {
                Err(PayloadError::Invalid("previous"))
            };
        };
        if self.epoch == Epoch::FIRST {
            return Err(PayloadError::Invalid("previous"));
        }
        let cuts = &succession.cuts;
        if cuts.is_empty() || cuts.len() > MAX_CUTS {
            return Err(PayloadError::Invalid("cuts"));
        }
        if cuts.iter().any(|cut| !(1..=MAX_NUMBER).contains(&cut.position)) {
            return Err(PayloadError::Invalid("cut"));
        }
        let ascending = cuts.windows(2).all(|pair| match pair {
            [left, right] => left.device < right.device,
            _ => true,
        });
        if !ascending {
            return Err(PayloadError::Invalid("cuts out of order"));
        }
        Ok(())
    }
}

/// What can happen to a claim: it is made, once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubEvent {
    /// A replica claimed the hub role.
    Claimed(Claimed),
}

impl Field for Cut {
    fn to_value(&self) -> Value {
        Value::Array(vec![self.device.to_value(), Value::Unsigned(self.position)])
    }

    /// A cut's shape; the claim's rules judge its position ([`Claimed::new`]).
    fn from_value(value: &Value) -> Option<Cut> {
        let [device, position] = value.as_array()? else { return None };
        Some(Cut { device: Id::from_value(device)?, position: position.as_u64()? })
    }
}

impl DomainEvent for HubEvent {
    const STREAM: &'static str = STREAM;

    const SCHEMAS: &'static [SchemaId] = &[CLAIMED];

    fn schema(&self) -> SchemaId {
        match self {
            HubEvent::Claimed(_) => CLAIMED,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            HubEvent::Claimed(claimed) => {
                let succeeds = claimed.succeeds.as_ref();
                Record::default()
                    .field(1, &claimed.epoch)
                    .field(2, &claimed.priority)
                    .optional(3, succeeds.map(|succession| &succession.previous))
                    .optional(4, succeeds.map(|succession| &succession.cuts))
                    .build()
            }
        }
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<HubEvent, DecodeError> {
        if !CLAIMED.matches(schema) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let epoch = fields.required(1, "epoch")?;
        let priority = fields.required(2, "priority")?;
        let previous = fields.optional(3, "previous")?;
        let cuts = fields.optional(4, "cuts")?;
        fields.finish()?;
        // A claim that succeeds none has nothing to cut.
        let succeeds = match (previous, cuts) {
            (Some(previous), Some(cuts)) => Some(Succession { previous, cuts }),
            (Some(_), None) => return Err(PayloadError::Missing("cuts").into()),
            (None, Some(_)) => return Err(PayloadError::Invalid("cuts").into()),
            (None, None) => None,
        };
        Ok(HubEvent::Claimed(Claimed::new(epoch, priority, succeeds)?))
    }
}

/// A claim as a replica holds it: the event that records it, where that lies in its claimant's
/// log, and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    /// The event that records it.
    pub event: Id<Event>,
    /// The claimant: the device whose log records it.
    pub device: Id<Device>,
    /// Its position in the claimant's log.
    pub position: u64,
    /// What it says.
    pub claimed: Claimed,
}

impl Claim {
    /// Whether it wins over `other`: it has a higher epoch; or, of one epoch, a higher priority,
    /// then a lower device identifier, then an earlier position in the device's log.
    pub fn beats(&self, other: &Claim) -> bool {
        self.rank() > other.rank()
    }

    /// Its rank among claims: the highest wins.
    fn rank(&self) -> (Epoch, NonZeroU8, Reverse<Id<Device>>, Reverse<u64>) {
        (self.claimed.epoch, self.claimed.priority, Reverse(self.device), Reverse(self.position))
    }
}

/// A term: a claim on the chain, and which of its device's sequencing records count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Term {
    /// The term's epoch.
    pub epoch: Epoch,
    /// The term's hub: the device whose claim began it.
    pub device: Id<Device>,
    /// The claim's event.
    pub claim: Id<Event>,
    /// The claim's position in the device's log: the term's records follow it.
    pub after: u64,
    /// The term's cut: how far into the device's log its records may lie. `None` for the winning
    /// claim's term, which no claim cuts.
    pub through: Option<u64>,
}

impl Term {
    /// Whether a sequencing record of the term's epoch, at `position` of the term's device's log,
    /// counts.
    pub fn counts(&self, position: u64) -> bool {
        position > self.after && self.through.is_none_or(|through| position <= through)
    }
}

/// The terms that `claims`, every claim a replica holds, each once, make: the chain, from the
/// winning claim's term back to the earliest it reaches. Empty when there are no claims. The
/// chain is whole when it reaches a claim of epoch 1.
pub fn chain(claims: &[Claim]) -> Vec<Term> {
    let Some(winning) = claims.iter().max_by_key(|claim| claim.rank()) else {
        return Vec::new();
    };
    let by_event: BTreeMap<Id<Event>, &Claim> =
        claims.iter().map(|claim| (claim.event, claim)).collect();
    let mut terms = Vec::new();
    // How far the claims after the current one all held each device's log: `None` while there
    // are none, and a device one of them didn't cut held none of its log.
    let mut held: Option<BTreeMap<Id<Device>, u64>> = None;
    let mut current = Some(winning);
    while let Some(claim) = current {
        terms.push(Term {
            epoch: claim.claimed.epoch,
            device: claim.device,
            claim: claim.event,
            after: claim.position,
            through: held.as_ref().map(|held| held.get(&claim.device).copied().unwrap_or(0)),
        });
        let Some(succession) = &claim.claimed.succeeds else { break };
        let cuts = succession.cuts.iter().map(|cut| (cut.device, cut.position));
        held = Some(match held {
            None => cuts.collect(),
            Some(later) => cuts
                .filter_map(|(device, position)| {
                    later.get(&device).map(|&reach| (device, position.min(reach)))
                })
                .collect(),
        });
        current = by_event
            .get(&succession.previous)
            .copied()
            .filter(|previous| previous.claimed.epoch < claim.claimed.epoch);
    }
    terms
}
