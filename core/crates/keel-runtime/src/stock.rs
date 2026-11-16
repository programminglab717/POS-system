//! Stock at the register (ADR-0036): receiving, adjusting, wasting, counting, reserving and
//! releasing an item's stock; its levels and what moved it since its latest count; and the
//! day's waste and counts.

use keel_domain::codec::Note;
use keel_domain::order::Reason;
use keel_domain::refs::Variant;
use keel_domain::staff::{Action, Request};
use keel_domain::stock::{
    Entry, EntryKind, Flow, Movement, Stock, StockCommand, StockCounted, StockMoved, levels,
};
use keel_events::keys::Signer;
use keel_types::{BusinessDate, Clock, Entropy, Id, Quantity, Timestamp, Unit};

use crate::error::RuntimeError;
use crate::runtime::{Runtime, append};

/// An item whose stock is tracked, and its levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockView {
    /// The variant.
    pub variant: Id<Variant>,
    /// What it is called, such as "Croissant".
    pub name: String,
    /// The unit its stock is kept in.
    pub unit: Unit,
    /// On hand: below zero when more sold than was there.
    pub on_hand: Quantity,
    /// Set aside.
    pub reserved: Quantity,
    /// On hand less set aside.
    pub available: Quantity,
    /// Each, as the locale shows it.
    pub on_hand_text: String,
    /// Reserved, as the locale shows it.
    pub reserved_text: String,
    /// Available, as the locale shows it.
    pub available_text: String,
    /// When it was last counted, if it was.
    pub counted_at: Option<Timestamp>,
}

/// What moved an item's stock: a movement, a count, a sale or a return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StockChange {
    /// A movement.
    Moved(Movement),
    /// A count of what was on hand.
    Counted,
    /// Sold.
    Sold,
    /// Returned.
    Returned,
}

/// One thing that moved an item's stock, as the register lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockLine {
    /// When it happened: a count's as-of time.
    pub at: Timestamp,
    /// What it was.
    pub change: StockChange,
    /// How much: a count's what was counted; a sale's negative.
    pub quantity: Quantity,
    /// How much, as the locale shows it.
    pub quantity_text: String,
}

/// What a count found: what was counted, what was expected then, and the difference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountVariance {
    /// The variant.
    pub variant: Id<Variant>,
    /// Its name.
    pub name: String,
    /// What was counted.
    pub counted: Quantity,
    /// What was expected at the count's time.
    pub expected: Quantity,
    /// Counted less expected.
    pub variance: Quantity,
    /// Counted, expected and the variance, as the locale shows them.
    pub texts: [String; 3],
}

/// What was wasted of an item in a day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wasted {
    /// The variant.
    pub variant: Id<Variant>,
    /// Its name.
    pub name: String,
    /// How much.
    pub quantity: Quantity,
    /// How much, as the locale shows it.
    pub quantity_text: String,
}

/// The unit of `entries`: the first's.
fn unit_of(entries: &[Entry]) -> Option<Unit> {
    entries.first().map(|entry| match entry.kind {
        EntryKind::Moved(_, quantity) | EntryKind::Counted { quantity, .. } => quantity.unit(),
    })
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Moves `variant`'s stock: received, adjusted (signed), wasted, reserved or released, by
    /// `quantity`, for `reason` (ADR-0036). Adjusting and wasting are `stock.adjust`, which the
    /// member's role permits or a manager approves; the rest need someone signed in.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownItem`] if the catalog has no such variant;
    /// [`RuntimeError::Stock`] for a quantity of nothing, in another unit than the item's stock,
    /// or a release beyond what is set aside; [`RuntimeError::ApprovalRequired`] or
    /// [`RuntimeError::NotApprover`] if it needs an approval it doesn't have.
    pub fn move_stock(
        &mut self,
        variant: Id<Variant>,
        movement: Movement,
        quantity: Quantity,
        reason: Option<Reason>,
    ) -> Result<StockView, RuntimeError> {
        let gated = matches!(movement, Movement::Adjusted | Movement::Wasted);
        let command = StockCommand::Move(StockMoved { movement, quantity, reason });
        self.record_stock(variant, command, gated)
    }

    /// Counts `variant`'s stock: `quantity` on hand as of `as_of`, or now; what moved after that
    /// time still counts (ADR-0036). `stock.adjust`, as an adjustment is.
    ///
    /// # Errors
    /// As [`Runtime::move_stock`]; [`RuntimeError::Stock`] for a count as of a time after now.
    pub fn count_stock(
        &mut self,
        variant: Id<Variant>,
        quantity: Quantity,
        as_of: Option<Timestamp>,
        note: Option<Note>,
    ) -> Result<StockView, RuntimeError> {
        let as_of = as_of.unwrap_or_else(|| self.clock.now());
        let command = StockCommand::Count(StockCounted { quantity, as_of, note });
        self.record_stock(variant, command, true)
    }

    fn record_stock(
        &mut self,
        variant: Id<Variant>,
        command: StockCommand,
        gated: bool,
    ) -> Result<StockView, RuntimeError> {
        let meta = self.meta()?;
        if self.profile.variant(variant).is_none() {
            return Err(RuntimeError::UnknownItem(variant.to_string()));
        }
        let id: Id<Stock> = variant.cast();
        let location = self.location();
        // The stock's own rules come before an approval is asked for what they would refuse.
        self.store.load(Stock::new(id))?.decide(location, meta.now, command.clone())?;
        let granted = if gated {
            self.gate(Request::of(Action::StockAdjust), Some(id.cast()))?
        } else {
            None
        };
        let stream = self.approval_stream(granted.as_ref())?;
        self.store.write(|w| -> Result<(), RuntimeError> {
            let meta = crate::staff::approved(w, granted.as_ref(), stream, &meta)?;
            let event = w.load(Stock::new(id))?.decide(location, meta.now, command)?;
            append(w, id.cast(), &event, &meta)?;
            Ok(())
        })?;
        self.spend(granted.as_ref());
        self.stock_view(variant)?.ok_or(RuntimeError::UnknownItem(variant.to_string()))
    }

    /// The items whose stock is tracked, with their levels: those below zero first, then by name.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn stock(&self) -> Result<Vec<StockView>, RuntimeError> {
        let mut views = Vec::new();
        for variant in self.store.tracked_stock()? {
            views.extend(self.stock_view(variant)?);
        }
        views.sort_by(|a, b| {
            b.on_hand.is_negative().cmp(&a.on_hand.is_negative()).then_with(|| a.name.cmp(&b.name))
        });
        Ok(views)
    }

    /// `variant`'s stock and levels; `None` if it isn't tracked.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn stock_view(&self, variant: Id<Variant>) -> Result<Option<StockView>, RuntimeError> {
        let Some((unit, levels)) = self.store.stock_levels(variant)? else { return Ok(None) };
        let name = self
            .profile
            .variant_name(variant)
            .map_or_else(|| variant.to_string(), ToString::to_string);
        let text = |quantity: Quantity| self.locale.quantity(quantity);
        Ok(Some(StockView {
            variant,
            name,
            unit,
            on_hand: levels.on_hand,
            reserved: levels.reserved,
            available: levels.available,
            on_hand_text: text(levels.on_hand),
            reserved_text: text(levels.reserved),
            available_text: text(levels.available),
            counted_at: levels.counted.map(|count| count.as_of),
        }))
    }

    /// What moved `variant`'s stock since its latest count, the latest first: the count itself,
    /// the movements recorded after its time, and the sales and returns after it; with no count,
    /// everything since its first stock event.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn stock_history(&self, variant: Id<Variant>) -> Result<Vec<StockLine>, RuntimeError> {
        let entries = self.store.stock_entries(variant)?;
        let Some(unit) = unit_of(&entries) else { return Ok(Vec::new()) };
        let flows = self.store.stock_flows(variant)?;
        let counted = levels(unit, &entries, &flows)?.counted;
        let first = entries.iter().map(|entry| entry.at).min();
        let after = |at: Timestamp| match (&counted, first) {
            (Some(count), _) => at > count.as_of,
            (None, Some(first)) => at >= first,
            (None, None) => false,
        };
        let text = |quantity: Quantity| self.locale.quantity(quantity);
        let mut lines = Vec::new();
        if let Some(count) = &counted {
            lines.push(StockLine {
                at: count.as_of,
                change: StockChange::Counted,
                quantity: count.quantity,
                quantity_text: text(count.quantity),
            });
        }
        for entry in entries.iter().filter(|entry| after(entry.at)) {
            if let EntryKind::Moved(movement, quantity) = entry.kind
                && quantity.unit() == unit
            {
                lines.push(StockLine {
                    at: entry.at,
                    change: StockChange::Moved(movement),
                    quantity,
                    quantity_text: text(quantity),
                });
            }
        }
        for flow in flows.iter().filter(|flow| flow.delta.unit() == unit && after(flow.at)) {
            let change =
                if flow.delta.is_negative() { StockChange::Sold } else { StockChange::Returned };
            lines.push(StockLine {
                at: flow.at,
                change,
                quantity: flow.delta,
                quantity_text: text(flow.delta),
            });
        }
        lines.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(lines)
    }

    /// What was wasted on business day `date`, by item, and what each count made that day found
    /// against what was expected at its time (ADR-0036, decision 6).
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read, or a level overflows.
    pub fn stock_report(
        &self,
        date: BusinessDate,
    ) -> Result<(Vec<Wasted>, Vec<CountVariance>), RuntimeError> {
        let day = self.profile.business_day();
        let (mut wasted, mut counts) = (Vec::new(), Vec::new());
        for variant in self.store.tracked_stock()? {
            let entries = self.store.stock_entries(variant)?;
            let Some(unit) = unit_of(&entries) else { continue };
            let name = self
                .profile
                .variant_name(variant)
                .map_or_else(|| variant.to_string(), ToString::to_string);
            let flows = self.store.stock_flows(variant)?;
            let mut waste = Quantity::zero(unit);
            for entry in &entries {
                if day.business_date_of(entry.at)? != date {
                    continue;
                }
                match entry.kind {
                    EntryKind::Moved(Movement::Wasted, quantity) if quantity.unit() == unit => {
                        waste = waste.checked_add(quantity)?;
                    }
                    EntryKind::Counted { quantity, as_of } if quantity.unit() == unit => {
                        let expected = expected_at(unit, &entries, &flows, entry, as_of)?;
                        let variance = quantity.checked_sub(expected)?;
                        let texts = [quantity, expected, variance].map(|q| self.locale.quantity(q));
                        counts.push(CountVariance {
                            variant,
                            name: name.clone(),
                            counted: quantity,
                            expected,
                            variance,
                            texts,
                        });
                    }
                    _ => {}
                }
            }
            if waste.is_positive() {
                let quantity_text = self.locale.quantity(waste);
                wasted.push(Wasted { variant, name, quantity: waste, quantity_text });
            }
        }
        Ok((wasted, counts))
    }
}

/// What was expected on hand at `as_of`, the time of the count `count`: the levels of what was
/// recorded by then, the counts as of an earlier time among them, and what sold up to then.
fn expected_at(
    unit: Unit,
    entries: &[Entry],
    flows: &[Flow],
    count: &Entry,
    as_of: Timestamp,
) -> Result<Quantity, RuntimeError> {
    let before: Vec<Entry> = entries
        .iter()
        .filter(|entry| entry.event != count.event)
        .filter(|entry| match entry.kind {
            EntryKind::Counted { as_of: other, .. } => {
                other < as_of || (other == as_of && entry.hlc < count.hlc)
            }
            EntryKind::Moved(..) => entry.at <= as_of,
        })
        .cloned()
        .collect();
    let flows: Vec<Flow> = flows.iter().filter(|flow| flow.at <= as_of).copied().collect();
    Ok(levels(unit, &before, &flows)?.on_hand)
}
