//! A kitchen ticket, folded from its events (ADR-0034).
//!
//! - Its sending stands: a second one doesn't apply, and is reported.
//! - Its first showing stands; a screen showing it again changes nothing.
//! - The latest bump or recall in canonical order stands: it is bumped exactly when that is a
//!   bump.
//! - Its first reroute stands. One made although a screen showed it, or a showing after a
//!   reroute, which happen when the device that rerouted it hadn't heard of the showing, is
//!   reported: the ticket was printed and shown.
//! - An event before it was sent, or at another location than its first, doesn't apply, and is
//!   reported.

use keel_events::envelope::{self, Device, Event, Location, SchemaRef};
use keel_types::{Hlc, Id, Timestamp};

use super::events::{KitchenEvent, RerouteTo, TicketSent};
use crate::aggregate::{Aggregate, EventMeta, Skipped};
use crate::schema::DecodeError;

/// A kitchen ticket: what a station is to make for an order, and what it did with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KitchenTicket {
    id: Id<KitchenTicket>,
    location: Option<Id<Location>>,
    sent: Option<(TicketSent, Moment)>,
    shown: Option<Moment>,
    bump: Option<(bool, Moment)>,
    rerouted: Option<(RerouteTo, Moment)>,
    conflicts: Vec<Conflict>,
    skipped: Vec<Skipped>,
}

/// When something happened to a ticket, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moment {
    /// The event.
    pub event: Id<Event>,
    /// The device that recorded it.
    pub device: Id<Device>,
    /// Its HLC.
    pub hlc: Hlc,
}

impl Moment {
    /// When it happened, by the clock of the device that recorded it, to the millisecond;
    /// `None` for an HLC beyond a timestamp's range, which no clock reads.
    pub fn at(&self) -> Option<Timestamp> {
        let millis = i64::try_from(self.hlc.wall_ms()).ok()?;
        Timestamp::from_millis(millis).ok()
    }
}

/// Something in a ticket's history that a person should look at.
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
    /// An event at another location than the ticket's first; it wasn't applied.
    WrongLocation,
    /// The ticket was sent again; the first sending stands.
    SentTwice,
    /// An event before the ticket was sent; it wasn't applied.
    BeforeSent,
    /// The ticket was both shown and rerouted: printed elsewhere, and on a screen.
    ShownAndRerouted,
}

impl KitchenTicket {
    /// Ticket `id`, before anything happened to it.
    pub const fn new(id: Id<KitchenTicket>) -> KitchenTicket {
        KitchenTicket {
            id,
            location: None,
            sent: None,
            shown: None,
            bump: None,
            rerouted: None,
            conflicts: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// Its identifier, which is also its stream's.
    pub const fn id(&self) -> Id<KitchenTicket> {
        self.id
    }

    /// Where it was sent; `None` until it was.
    pub const fn location(&self) -> Option<Id<Location>> {
        self.location
    }

    /// What it is to make, and when it was sent; `None` until it was.
    pub fn sent(&self) -> Option<(&TicketSent, Moment)> {
        self.sent.as_ref().map(|(sent, moment)| (sent, *moment))
    }

    /// When a screen first showed it.
    pub const fn shown(&self) -> Option<Moment> {
        self.shown
    }

    /// Whether its station is done with it.
    pub fn is_bumped(&self) -> bool {
        self.bump.is_some_and(|(bumped, _)| bumped)
    }

    /// When it was bumped, if it is.
    pub fn bumped(&self) -> Option<Moment> {
        self.bump.and_then(|(bumped, moment)| bumped.then_some(moment))
    }

    /// The latest bump or recall, and whether it was a bump.
    pub const fn last_bump_or_recall(&self) -> Option<(bool, Moment)> {
        self.bump
    }

    /// Where it was rerouted, and when, if it was.
    pub const fn rerouted(&self) -> Option<(RerouteTo, Moment)> {
        self.rerouted
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

impl Aggregate for KitchenTicket {
    type Event = KitchenEvent;

    fn stream_id(&self) -> Id<envelope::Aggregate> {
        self.id.cast()
    }

    fn apply(&mut self, meta: &EventMeta, event: &KitchenEvent) {
        match self.location {
            Some(location) if location != meta.location => {
                return self.conflict(meta, ConflictKind::WrongLocation);
            }
            _ => {}
        }
        let moment = Moment { event: meta.event_id, device: meta.origin_device, hlc: meta.hlc };
        if let KitchenEvent::Sent(sent) = event {
            if self.sent.is_some() {
                return self.conflict(meta, ConflictKind::SentTwice);
            }
            self.location = Some(meta.location);
            self.sent = Some((sent.clone(), moment));
            return;
        }
        if self.sent.is_none() {
            return self.conflict(meta, ConflictKind::BeforeSent);
        }
        match event {
            KitchenEvent::Sent(_) => {}
            KitchenEvent::Shown => {
                if self.shown.is_none() {
                    self.shown = Some(moment);
                    if self.rerouted.is_some() {
                        self.conflict(meta, ConflictKind::ShownAndRerouted);
                    }
                }
            }
            KitchenEvent::Bumped => self.bump = Some((true, moment)),
            KitchenEvent::Recalled => self.bump = Some((false, moment)),
            KitchenEvent::Rerouted(to) => {
                if self.rerouted.is_none() {
                    self.rerouted = Some((*to, moment));
                    if self.shown.is_some() {
                        self.conflict(meta, ConflictKind::ShownAndRerouted);
                    }
                }
            }
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
