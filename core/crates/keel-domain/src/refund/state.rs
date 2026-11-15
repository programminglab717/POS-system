//! A refund, folded from its events (ADR-0032).
//!
//! The fold applies what it can and reports the rest: a completion by another device than the
//! one that started the refund, or for more than it was started for, applies (the money has
//! moved) and is reported; a second start, a second completion, a completion before the start,
//! one at another location or in another currency, don't apply and are reported.
//!
//! A failed attempt leaves the refund owed (ADR-0035): a completion after it applies; a failure
//! after the completion doesn't, and is reported, as is one before the start or at another
//! location. One by another device applies, and is reported.

use core::cmp::Ordering;

use keel_events::envelope::{self, Device, Event, Location, SchemaRef};
use keel_types::{Id, Money};

use super::events::{RefundCompleted, RefundEvent, RefundFailed, RefundInitiated};
use crate::aggregate::{Aggregate, EventMeta, Skipped};
use crate::order::Order;
use crate::payment::{Payment, Tender};
use crate::schema::DecodeError;

/// A refund: what it is for, and whether it was paid back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refund {
    id: Id<Refund>,
    info: Option<RefundInfo>,
    completed: Option<RefundCompleted>,
    failed: Option<RefundFailed>,
    failures: u32,
    conflicts: Vec<Conflict>,
    skipped: Vec<Skipped>,
}

/// What a refund was started with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefundInfo {
    /// The order whose lines were returned.
    pub order: Id<Order>,
    /// How it is paid back.
    pub tender: Tender,
    /// How much is owed.
    pub amount: Money,
    /// For a card refund, the card payment it goes back to.
    pub payment: Option<Id<Payment>>,
    /// Where it was started.
    pub location: Id<Location>,
    /// The device that started it, which alone writes it.
    pub device: Id<Device>,
    /// The event that started it.
    pub initiated_by: Id<Event>,
}

/// Something in a refund's history that a person should look at.
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
    /// The refund was started again; the second start wasn't applied.
    DuplicateInitiation,
    /// An event came before the refund was started; it wasn't applied.
    BeforeInitiation,
    /// An event was recorded at another location; it wasn't applied.
    WrongLocation,
    /// An event was recorded by another device than the one that started the refund; it applied.
    NotInitiator {
        /// The device that recorded it.
        by: Id<Device>,
    },
    /// An amount in another currency than the refund's; it wasn't applied.
    CurrencyMismatch,
    /// The refund was completed again; the first completion stands.
    DuplicateCompletion,
    /// The refund was completed for more than it was started for; it applied.
    BeyondInitiated,
    /// An attempt failed after the refund was completed; it wasn't applied.
    FailedAfterCompletion,
}

impl Refund {
    /// The refund with identifier `id`, before any event: not yet started.
    pub const fn new(id: Id<Refund>) -> Refund {
        Refund {
            id,
            info: None,
            completed: None,
            failed: None,
            failures: 0,
            conflicts: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// The refund's identifier, which is also its stream's, and the one its return names.
    pub const fn id(&self) -> Id<Refund> {
        self.id
    }

    /// What it was started with; `None` until it is started.
    pub const fn info(&self) -> Option<&RefundInfo> {
        self.info.as_ref()
    }

    /// Its completion, once the money was paid back.
    pub const fn completed(&self) -> Option<&RefundCompleted> {
        self.completed.as_ref()
    }

    /// The latest attempt that failed, while the refund is owed; `None` once it is completed.
    pub fn failed(&self) -> Option<&RefundFailed> {
        if self.completed.is_some() { None } else { self.failed.as_ref() }
    }

    /// How many attempts to pay it back failed: the next attempt is this one more.
    pub const fn failures(&self) -> u32 {
        self.failures
    }

    /// Whether it is owed: started, and not yet paid back.
    pub const fn is_owed(&self) -> bool {
        self.info.is_some() && self.completed.is_none()
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

    fn apply_initiated(&mut self, meta: &EventMeta, initiated: &RefundInitiated) {
        if self.info.is_some() {
            return self.conflict(meta, ConflictKind::DuplicateInitiation);
        }
        self.info = Some(RefundInfo {
            order: initiated.order,
            tender: initiated.tender,
            amount: initiated.amount,
            payment: initiated.payment,
            location: meta.location,
            device: meta.origin_device,
            initiated_by: meta.event_id,
        });
    }

    fn apply_completed(&mut self, meta: &EventMeta, info: RefundInfo, completed: &RefundCompleted) {
        if self.completed.is_some() {
            return self.conflict(meta, ConflictKind::DuplicateCompletion);
        }
        match completed.amount.compare(info.amount) {
            Err(_) => return self.conflict(meta, ConflictKind::CurrencyMismatch),
            Ok(Ordering::Greater) => self.conflict(meta, ConflictKind::BeyondInitiated),
            Ok(_) => {}
        }
        self.completed = Some(*completed);
    }
}

impl Aggregate for Refund {
    type Event = RefundEvent;

    fn stream_id(&self) -> Id<envelope::Aggregate> {
        self.id.cast()
    }

    fn apply(&mut self, meta: &EventMeta, event: &RefundEvent) {
        if let RefundEvent::Initiated(initiated) = event {
            return self.apply_initiated(meta, initiated);
        }
        let Some(info) = self.info else {
            return self.conflict(meta, ConflictKind::BeforeInitiation);
        };
        if meta.location != info.location {
            return self.conflict(meta, ConflictKind::WrongLocation);
        }
        if meta.origin_device != info.device {
            let by = meta.origin_device;
            self.conflict(meta, ConflictKind::NotInitiator { by });
        }
        match event {
            RefundEvent::Initiated(_) => {}
            RefundEvent::Completed(completed) => self.apply_completed(meta, info, completed),
            RefundEvent::Failed(_) if self.completed.is_some() => {
                self.conflict(meta, ConflictKind::FailedAfterCompletion);
            }
            RefundEvent::Failed(failed) => {
                self.failed = Some(failed.clone());
                self.failures = self.failures.saturating_add(1);
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
