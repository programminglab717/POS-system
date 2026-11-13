//! Availability at the register (ADR-0033): an item 86'd, until the business day ends, until a
//! time or until put back, and put back; what is sold out now; and a version a shell follows to
//! know when to read the menu again.

use std::collections::BTreeMap;

use keel_domain::availability::{Availability, AvailabilityChanged, AvailabilityCommand, ItemKind};
use keel_domain::codec::Name;
use keel_domain::order::Reason;
use keel_domain::profile::Choice;
use keel_domain::refs::{Modifier, Variant};
use keel_domain::staff::{Action, Request};
use keel_events::keys::Signer;
use keel_types::{Clock, Entropy, Id, LocalTime, Timestamp};

use crate::error::RuntimeError;
use crate::runtime::{Runtime, append};

/// An item whose availability a register changes: a variant, or a modifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemRef {
    /// A variant, rung as a line.
    Variant(Id<Variant>),
    /// A modifier, chosen on a line.
    Modifier(Id<Modifier>),
}

impl ItemRef {
    const fn id(self) -> Id<Availability> {
        match self {
            ItemRef::Variant(variant) => variant.cast(),
            ItemRef::Modifier(modifier) => modifier.cast(),
        }
    }

    const fn kind(self) -> ItemKind {
        match self {
            ItemRef::Variant(_) => ItemKind::Variant,
            ItemRef::Modifier(_) => ItemKind::Modifier,
        }
    }
}

/// How long an 86 lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    /// The rest of the business day: back on sale when the next begins.
    EndOfDay,
    /// Until a time.
    Time(Timestamp),
    /// Until it is put back.
    PutBack,
}

/// An item sold out, and until when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoldOut {
    /// The item.
    pub item: ItemRef,
    /// Its name.
    pub name: Name,
    /// When it is back on sale, in the location's time; `None` until it is put back.
    pub until: Option<LocalTime>,
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// The items sold out now, of the location's catalog, each with when it is back: the
    /// latest-changed first is not promised; they come in the order of their identifiers.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read, or a time is out of the business day's range.
    pub fn sold_out(&self) -> Result<Vec<SoldOut>, RuntimeError> {
        let profile = &self.profile;
        let mut sold_out = Vec::new();
        for summary in self.store.sold_out(self.clock.now())? {
            let Some((kind, _, until)) = summary.latest else { continue };
            let (item, name) = match kind {
                ItemKind::Variant => {
                    let variant: Id<Variant> = summary.id.cast();
                    let Some(name) = profile.variant_name(variant) else { continue };
                    (ItemRef::Variant(variant), name.clone())
                }
                ItemKind::Modifier => {
                    let modifier: Id<Modifier> = summary.id.cast();
                    let Some(found) = profile.modifier(modifier) else { continue };
                    (ItemRef::Modifier(modifier), found.name.clone())
                }
            };
            let until = until.map(|until| profile.business_day().local_time(until)).transpose()?;
            sold_out.push(SoldOut { item, name, until });
        }
        Ok(sold_out)
    }

    /// The items sold out now, by identifier, each with when it is back: what the menu and the
    /// modifier sheet show.
    pub(crate) fn sold_out_map(
        &self,
    ) -> Result<BTreeMap<Id<Availability>, Option<LocalTime>>, RuntimeError> {
        Ok(self
            .sold_out()?
            .into_iter()
            .map(|sold_out| (sold_out.item.id(), sold_out.until))
            .collect())
    }

    /// How many availability changes the store holds: a shell reads the menu again when it moves.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read.
    pub fn availability_version(&self) -> Result<u64, RuntimeError> {
        Ok(self.store.availability_version()?)
    }

    /// 86s `item`, `until` the business day ends, a time, or it is put back, for `reason`:
    /// `item.availability`, which the member's role permits or a manager approves (ADR-0033).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownItem`] if the catalog has no such item; [`RuntimeError::Availability`] if
    /// it is sold out already until then, or the time isn't after now;
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an
    /// approval it doesn't have; [`RuntimeError`] if no one is signed in.
    pub fn eighty_six(
        &mut self,
        item: ItemRef,
        until: Until,
        reason: Option<Reason>,
    ) -> Result<(), RuntimeError> {
        let now = self.clock.now();
        let until = match until {
            Until::EndOfDay => {
                let day = self.profile.business_day();
                Some(day.window(day.business_date_of(now)?)?.1)
            }
            Until::Time(time) => Some(time),
            Until::PutBack => None,
        };
        self.change_availability(item, false, until, reason)
    }

    /// Puts `item` back on sale: `item.availability`, as an 86 is.
    ///
    /// # Errors
    /// As [`Runtime::eighty_six`]; [`RuntimeError::Availability`] if it is on sale already.
    pub fn put_back(&mut self, item: ItemRef) -> Result<(), RuntimeError> {
        self.change_availability(item, true, None, None)
    }

    fn change_availability(
        &mut self,
        item: ItemRef,
        available: bool,
        until: Option<Timestamp>,
        reason: Option<Reason>,
    ) -> Result<(), RuntimeError> {
        let meta = self.meta()?;
        let known = match item {
            ItemRef::Variant(variant) => self.profile.variant(variant).is_some(),
            ItemRef::Modifier(modifier) => self.profile.modifier(modifier).is_some(),
        };
        if !known {
            return Err(RuntimeError::UnknownItem(item.id().to_string()));
        }
        let id = item.id();
        let current = self.store.load(Availability::new(id))?;
        let changed = AvailabilityChanged { kind: item.kind(), available, until, reason };
        let command = AvailabilityCommand::Change(changed);
        // The item's own rules come before an approval is asked for a change they would refuse.
        let event = current.decide(self.location(), meta.now, command.clone())?;
        let granted = self.gate(Request::of(Action::ItemAvailability), Some(id.cast()))?;
        let stream = self.approval_stream(granted.as_ref())?;
        let location = self.location();
        self.store.write(|w| -> Result<(), RuntimeError> {
            let meta = crate::staff::approved(w, granted.as_ref(), stream, &meta)?;
            // Decided again in the write, on the item as it stands.
            let now = w.load(Availability::new(id))?.decide(location, meta.now, command)?;
            if now != event {
                return Err(RuntimeError::OrderChanged);
            }
            append(w, id.cast(), &event, &meta)?;
            Ok(())
        })?;
        self.spend(granted.as_ref());
        Ok(())
    }

    /// Refuses ringing `variant` with `choices` if it, or a modifier chosen at any depth, is sold
    /// out now: a line rung before stays (ADR-0033).
    pub(crate) fn check_on_sale(
        &self,
        variant: Id<Variant>,
        choices: &[Choice],
    ) -> Result<(), RuntimeError> {
        let sold_out = self.sold_out_map()?;
        let profile = &self.profile;
        if sold_out.contains_key(&variant.cast()) {
            let name = profile.variant_name(variant).map(ToString::to_string).unwrap_or_default();
            return Err(RuntimeError::SoldOut(name));
        }
        // The choices, and the choices made under each, however deep.
        let mut left: Vec<&Choice> = choices.iter().collect();
        while let Some(choice) = left.pop() {
            if sold_out.contains_key(&choice.modifier.cast()) {
                let name = profile.modifier(choice.modifier).map(|found| found.name.to_string());
                return Err(RuntimeError::SoldOut(name.unwrap_or_default()));
            }
            left.extend(choice.choices.iter());
        }
        Ok(())
    }
}
