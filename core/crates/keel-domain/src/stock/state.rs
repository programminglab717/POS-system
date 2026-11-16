//! An item's stock, folded from its events, and its levels (ADR-0036).
//!
//! The fold keeps every movement and count that applies, with when it was recorded: the levels
//! are sums over them, so the order the events arrive in changes nothing. An event at another
//! location than the stock's first, or in another unit than its first event's, doesn't apply, and
//! is reported; so is a release beyond what is reserved, which applies.

use core::cmp::Ordering;

use keel_events::envelope::{self, Event, Location, SchemaRef};
use keel_types::{Hlc, Id, Quantity, QuantityError, Timestamp, Unit};

use super::events::{Movement, StockEvent};
use crate::aggregate::{Aggregate, EventMeta, Skipped};
use crate::schema::DecodeError;

/// An item's stock at a location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stock {
    id: Id<Stock>,
    location: Option<Id<Location>>,
    unit: Option<Unit>,
    entries: Vec<Entry>,
    conflicts: Vec<Conflict>,
    skipped: Vec<Skipped>,
}

/// A movement or a count that applied, and when it was recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The event.
    pub event: Id<Event>,
    /// When it was recorded: its HLC's wall time.
    pub at: Timestamp,
    /// Its HLC, which orders events recorded at once.
    pub hlc: Hlc,
    /// What it was.
    pub kind: EntryKind,
}

/// What an entry was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A movement of a quantity.
    Moved(Movement, Quantity),
    /// A count of what was on hand as of a time.
    Counted {
        /// What was counted.
        quantity: Quantity,
        /// As of when.
        as_of: Timestamp,
    },
}

/// What a sale or a return did to an item's stock, read from its order: a sale takes, a return
/// puts back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flow {
    /// When: the order's close, or the return.
    pub at: Timestamp,
    /// How much: negative for a sale, positive for a return.
    pub delta: Quantity,
}

/// The count a level starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Count {
    /// The count's event.
    pub event: Id<Event>,
    /// What was counted.
    pub quantity: Quantity,
    /// As of when.
    pub as_of: Timestamp,
}

/// An item's stock levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Levels {
    /// On hand: the latest count, and what moved, sold and was returned after it; below zero
    /// when more sold than was there.
    pub on_hand: Quantity,
    /// Set aside: reserved, less released.
    pub reserved: Quantity,
    /// On hand less reserved.
    pub available: Quantity,
    /// The count on hand starts from, if there is one.
    pub counted: Option<Count>,
}

/// Something in an item's stock that a person should look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The event that caused it.
    pub event: Id<Event>,
    /// What happened.
    pub kind: ConflictKind,
}

/// What a conflict is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictKind {
    /// An event was recorded at another location than the stock's first; it wasn't applied.
    WrongLocation,
    /// An event's quantity is in another unit than the stock's; it wasn't applied.
    WrongUnit,
    /// A release took reserved below zero, as two devices releasing at once do; it applied.
    OverReleased,
    /// An event's HLC is out of the range of a time; it wasn't applied.
    OutOfRange,
}

/// When an event with `hlc` was recorded: its wall time, as a timestamp.
pub fn time_of(hlc: Hlc) -> Option<Timestamp> {
    let micros = i64::try_from(hlc.wall_ms()).ok()?.checked_mul(1_000)?;
    Timestamp::from_micros(micros).ok()
}

impl Stock {
    /// The stock of variant `id`, before any event: none, and not tracked.
    pub const fn new(id: Id<Stock>) -> Stock {
        Stock {
            id,
            location: None,
            unit: None,
            entries: Vec::new(),
            conflicts: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// The variant's identifier, which is also its stream's.
    pub const fn id(&self) -> Id<Stock> {
        self.id
    }

    /// Where its first event was recorded; `None` until one is.
    pub const fn location(&self) -> Option<Id<Location>> {
        self.location
    }

    /// The unit it is kept in, its first event's; `None` until one applies.
    pub const fn unit(&self) -> Option<Unit> {
        self.unit
    }

    /// Whether it is tracked: it has a movement or a count.
    pub const fn is_tracked(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Its movements and counts that applied, in the order they did.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// What is reserved: reserved, less released.
    ///
    /// # Errors
    /// [`QuantityError`] if the sum overflows.
    pub fn reserved(&self) -> Result<Quantity, QuantityError> {
        reserved_of(self.unit.unwrap_or(Unit::Each), &self.entries)
    }

    /// Everything in its history that a person should look at.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// The events it couldn't read.
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    fn conflict(&mut self, meta: &EventMeta, kind: ConflictKind) {
        self.conflicts.push(Conflict { event: meta.event_id, kind });
    }
}

/// Reserved, less released, of `entries`, in `unit`.
fn reserved_of(unit: Unit, entries: &[Entry]) -> Result<Quantity, QuantityError> {
    let mut reserved = Quantity::zero(unit);
    for entry in entries {
        match entry.kind {
            EntryKind::Moved(Movement::Reserved, quantity) => {
                reserved = reserved.checked_add(quantity)?;
            }
            EntryKind::Moved(Movement::Released, quantity) => {
                reserved = reserved.checked_sub(quantity)?;
            }
            _ => {}
        }
    }
    Ok(reserved)
}

/// The order counts are taken in: by as-of time, then by HLC, then by event, the latest last.
fn count_order(a: &Count, a_hlc: Hlc, b: &Count, b_hlc: Hlc) -> Ordering {
    a.as_of
        .cmp(&b.as_of)
        .then(a_hlc.cmp(&b_hlc))
        .then_with(|| a.event.to_bytes().cmp(&b.event.to_bytes()))
}

/// The levels of an item kept in `unit`, from its stock's `entries` and the `flows` its orders
/// made (ADR-0036, decision 3). On hand starts from the latest count, by as-of time, and adds
/// what moved, sold and was returned after it; with no count, from the first entry, whose time
/// the flows count from. Entries and flows in another unit are left out.
///
/// # Errors
/// [`QuantityError`] if a sum overflows.
pub fn levels(unit: Unit, entries: &[Entry], flows: &[Flow]) -> Result<Levels, QuantityError> {
    let entries: Vec<&Entry> =
        entries.iter().filter(|entry| unit_of(&entry.kind) == unit).collect();
    let mut latest: Option<(Count, Hlc)> = None;
    for entry in &entries {
        if let EntryKind::Counted { quantity, as_of } = entry.kind {
            let count = Count { event: entry.event, quantity, as_of };
            let later = latest.as_ref().is_none_or(|(known, hlc)| {
                count_order(&count, entry.hlc, known, *hlc) == Ordering::Greater
            });
            if later {
                latest = Some((count, entry.hlc));
            }
        }
    }
    let counted = latest.map(|(count, _)| count);
    // What counts after the start: after a count's as-of time, or from the first entry's time.
    let first = entries.iter().map(|entry| entry.at).min();
    let after = |at: Timestamp| match (&counted, first) {
        (Some(count), _) => at > count.as_of,
        (None, Some(first)) => at >= first,
        (None, None) => false,
    };
    let mut on_hand = counted.map_or(Quantity::zero(unit), |count| count.quantity);
    for entry in &entries {
        let EntryKind::Moved(movement, quantity) = entry.kind else { continue };
        if !after(entry.at) && counted.is_some() {
            continue;
        }
        on_hand = match movement {
            Movement::Received | Movement::Adjusted => on_hand.checked_add(quantity)?,
            Movement::Wasted => on_hand.checked_sub(quantity)?,
            Movement::Reserved | Movement::Released => on_hand,
        };
    }
    for flow in flows.iter().filter(|flow| flow.delta.unit() == unit && after(flow.at)) {
        on_hand = on_hand.checked_add(flow.delta)?;
    }
    let owned: Vec<Entry> = entries.into_iter().cloned().collect();
    let reserved = reserved_of(unit, &owned)?;
    Ok(Levels { on_hand, reserved, available: on_hand.checked_sub(reserved)?, counted })
}

const fn unit_of(kind: &EntryKind) -> Unit {
    match kind {
        EntryKind::Moved(_, quantity) | EntryKind::Counted { quantity, .. } => quantity.unit(),
    }
}

impl Aggregate for Stock {
    type Event = StockEvent;

    fn stream_id(&self) -> Id<envelope::Aggregate> {
        self.id.cast()
    }

    fn apply(&mut self, meta: &EventMeta, event: &StockEvent) {
        match self.location {
            Some(location) if location != meta.location => {
                return self.conflict(meta, ConflictKind::WrongLocation);
            }
            _ => self.location = Some(meta.location),
        }
        let unit = event.quantity().unit();
        if self.unit.is_some_and(|known| known != unit) {
            return self.conflict(meta, ConflictKind::WrongUnit);
        }
        let Some(at) = time_of(meta.hlc) else {
            return self.conflict(meta, ConflictKind::OutOfRange);
        };
        self.unit = Some(unit);
        let kind = match event {
            StockEvent::Moved(moved) => EntryKind::Moved(moved.movement, moved.quantity),
            StockEvent::Counted(counted) => {
                EntryKind::Counted { quantity: counted.quantity, as_of: counted.as_of }
            }
        };
        self.entries.push(Entry { event: meta.event_id, at, hlc: meta.hlc, kind });
        if matches!(kind, EntryKind::Moved(Movement::Released, _))
            && self.reserved().is_ok_and(Quantity::is_negative)
        {
            self.conflict(meta, ConflictKind::OverReleased);
        }
    }

    fn skip(&mut self, meta: &EventMeta, schema: &SchemaRef, reason: &DecodeError) {
        self.skipped.push(Skipped {
            event: meta.event_id,
            schema: schema.clone(),
            reason: reason.clone(),
        });
    }
}
