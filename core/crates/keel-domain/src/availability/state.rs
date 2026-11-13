//! An item's availability, folded from its events (ADR-0033).
//!
//! The latest change in canonical order stands. A change at another location than the first
//! doesn't apply, and is reported; one naming the other kind of item applies, the kind following
//! it, and is reported.

use keel_events::envelope::{self, Device, Event, Location, SchemaRef};
use keel_types::{Hlc, Id, Timestamp};

use super::events::{AvailabilityChanged, AvailabilityEvent, ItemKind};
use crate::aggregate::{Aggregate, EventMeta, Skipped};
use crate::schema::DecodeError;

/// An item's availability at a location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Availability {
    id: Id<Availability>,
    location: Option<Id<Location>>,
    latest: Option<Change>,
    conflicts: Vec<Conflict>,
    skipped: Vec<Skipped>,
}

/// The change that stands, and who made it, when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// What it changed to.
    pub changed: AvailabilityChanged,
    /// The event.
    pub event: Id<Event>,
    /// The device that made it.
    pub device: Id<Device>,
    /// Its HLC.
    pub hlc: Hlc,
}

/// Something in an item's availability that a person should look at.
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
    /// A change was recorded at another location than the item's first; it wasn't applied.
    WrongLocation,
    /// A change named the other kind of item than the one before; it applied.
    KindChanged,
}

impl Availability {
    /// The availability of item `id`, before any change: on sale.
    pub const fn new(id: Id<Availability>) -> Availability {
        Availability {
            id,
            location: None,
            latest: None,
            conflicts: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// The item's identifier, which is also its stream's.
    pub const fn id(&self) -> Id<Availability> {
        self.id
    }

    /// Where its availability was first changed; `None` until it is.
    pub const fn location(&self) -> Option<Id<Location>> {
        self.location
    }

    /// The change that stands; `None` if it was never changed: on sale.
    pub const fn latest(&self) -> Option<&Change> {
        self.latest.as_ref()
    }

    /// Whether the item is sold out at `now`: 86'd, and not yet back on sale.
    pub fn sold_out_at(&self, now: Timestamp) -> bool {
        self.latest.as_ref().is_some_and(|latest| {
            !latest.changed.available && latest.changed.until.is_none_or(|until| now < until)
        })
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

impl Aggregate for Availability {
    type Event = AvailabilityEvent;

    fn stream_id(&self) -> Id<envelope::Aggregate> {
        self.id.cast()
    }

    fn apply(&mut self, meta: &EventMeta, event: &AvailabilityEvent) {
        let AvailabilityEvent::Changed(changed) = event;
        match self.location {
            Some(location) if location != meta.location => {
                return self.conflict(meta, ConflictKind::WrongLocation);
            }
            _ => self.location = Some(meta.location),
        }
        let kind: Option<ItemKind> = self.latest.as_ref().map(|latest| latest.changed.kind);
        if kind.is_some_and(|kind| kind != changed.kind) {
            self.conflict(meta, ConflictKind::KindChanged);
        }
        self.latest = Some(Change {
            changed: changed.clone(),
            event: meta.event_id,
            device: meta.origin_device,
            hlc: meta.hlc,
        });
    }

    fn skip(&mut self, meta: &EventMeta, schema: &SchemaRef, reason: &DecodeError) {
        self.skipped.push(Skipped {
            event: meta.event_id,
            schema: schema.clone(),
            reason: reason.clone(),
        });
    }
}
