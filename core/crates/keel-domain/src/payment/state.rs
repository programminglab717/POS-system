//! The payment's state, and the fold that builds it from events.
//!
//! A payment is a state machine: initiated, then authorized or captured, or failed or voided;
//! an authorized payment is then captured, failed or voided. The fold is total, and money that
//! may have moved is never lost to an event that arrives out of turn, from a device acting
//! concurrently:
//!
//! - A capture always applies: the money moved. Once captured, a payment stays captured, and any
//!   outcome recorded after it leaves a conflict.
//! - An authorization or a capture after the payment failed or was voided applies, and leaves a
//!   conflict: the card may be held or charged. Authorized, the payment is unresolved again, and
//!   must be captured or voided.
//! - A failure or a void after the payment failed or was voided changes nothing: no money moved
//!   either way. A second authorization changes nothing either.
//!
//! Events recorded before the payment was initiated, by a device at another location, or with
//! money in another currency aren't applied, and leave a conflict.

use keel_events::envelope::{self, Event, Location, SchemaRef};
use keel_types::{Id, Money};

use super::events::{PaymentAuthorized, PaymentCaptured, PaymentEnded, PaymentEvent, Tender};
use crate::aggregate::{Aggregate, EventMeta, Skipped};
use crate::order::{Check, Order};
use crate::schema::DecodeError;

/// A payment: money moving against a check (domain model §7). Its identifier is the idempotency
/// key the terminal or processor sees, so a retry can never charge twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payment {
    id: Id<Payment>,
    info: Option<PaymentInfo>,
    status: PaymentStatus,
    conflicts: Vec<Conflict>,
    skipped: Vec<Skipped>,
}

/// What a payment was initiated with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentInfo {
    /// Where it was initiated.
    pub location: Id<Location>,
    /// The order it pays for.
    pub order: Id<Order>,
    /// The check it pays.
    pub check: Id<Check>,
    /// How it is paid.
    pub tender: Tender,
    /// How much it asked for.
    pub amount: Money,
    /// The event that initiated it.
    pub initiated_by: Id<Event>,
}

/// Where a payment is in its life.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaymentStatus {
    /// Started, with no outcome yet.
    Initiated,
    /// The card is held for an amount, not yet charged.
    Authorized(PaymentAuthorized),
    /// The money moved.
    Captured(PaymentCaptured),
    /// The attempt failed: no money moved.
    Failed(PaymentEnded),
    /// The payment was voided before it was captured: no money moved.
    Voided(PaymentEnded),
}

/// A payment's outcome, without its details.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Authorized.
    Authorized,
    /// Captured.
    Captured,
    /// Failed.
    Failed,
    /// Voided.
    Voided,
}

impl PaymentStatus {
    /// The outcome recorded so far: `None` while the payment is only initiated.
    pub const fn outcome(&self) -> Option<Outcome> {
        match self {
            PaymentStatus::Initiated => None,
            PaymentStatus::Authorized(_) => Some(Outcome::Authorized),
            PaymentStatus::Captured(_) => Some(Outcome::Captured),
            PaymentStatus::Failed(_) => Some(Outcome::Failed),
            PaymentStatus::Voided(_) => Some(Outcome::Voided),
        }
    }

    /// Whether the payment is unresolved: initiated or authorized, with no final outcome. Money
    /// may still move, so no other payment can start on its check.
    pub const fn is_unresolved(&self) -> bool {
        matches!(self, PaymentStatus::Initiated | PaymentStatus::Authorized(_))
    }
}

/// Something in a payment's history that a person should look at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The event that caused it.
    pub event: Id<Event>,
    /// What happened.
    pub kind: ConflictKind,
}

/// What a payment conflict is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictKind {
    /// The payment was initiated again; the second initiation wasn't applied.
    DuplicateInitiation,
    /// An event came before the payment was initiated; it wasn't applied.
    BeforeInitiation,
    /// An event was recorded by a device at another location; it wasn't applied.
    WrongLocation,
    /// An event has money in another currency than the payment's; it wasn't applied.
    CurrencyMismatch,
    /// An outcome was recorded after another it doesn't follow: `outcome` after `after`, such as
    /// a void after a capture. A capture applies even so; see the module's rules for the rest.
    OutOfTurn {
        /// The outcome the payment had.
        after: Outcome,
        /// The outcome recorded.
        outcome: Outcome,
    },
}

impl Payment {
    /// The payment with identifier `id`, before any event: not yet initiated.
    pub const fn new(id: Id<Payment>) -> Payment {
        Payment {
            id,
            info: None,
            status: PaymentStatus::Initiated,
            conflicts: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// The payment's identifier: its idempotency key, and its event stream's.
    pub const fn id(&self) -> Id<Payment> {
        self.id
    }

    /// What the payment was initiated with; `None` until it is initiated.
    pub const fn info(&self) -> Option<&PaymentInfo> {
        self.info.as_ref()
    }

    /// Where the payment is in its life. Meaningless until it is initiated.
    pub const fn status(&self) -> &PaymentStatus {
        &self.status
    }

    /// The capture, if the money moved.
    pub const fn captured(&self) -> Option<&PaymentCaptured> {
        match &self.status {
            PaymentStatus::Captured(captured) => Some(captured),
            _ => None,
        }
    }

    /// Whether the payment was initiated and is unresolved: money may still move.
    pub const fn is_unresolved(&self) -> bool {
        self.info.is_some() && self.status.is_unresolved()
    }

    /// Everything in the payment's history that a person should look at.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Events that weren't applied because they couldn't be decoded.
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// Whether events from a newer kernel were skipped: this kernel needs updating to show the
    /// payment fully.
    pub fn needs_update(&self) -> bool {
        self.skipped.iter().any(|skipped| skipped.reason == DecodeError::UnknownSchema)
    }

    fn conflict(&mut self, meta: &EventMeta, kind: ConflictKind) {
        self.conflicts.push(Conflict { event: meta.event_id, kind });
    }

    /// Moves the payment to `next`, whose outcome is `outcome`, as the state machine allows.
    fn record(&mut self, meta: &EventMeta, outcome: Outcome, next: PaymentStatus) {
        let applies = match (self.status.outcome(), outcome) {
            // Every outcome follows an initiation, and every other one an authorization.
            (None, _)
            | (Some(Outcome::Authorized), Outcome::Captured | Outcome::Failed | Outcome::Voided) => {
                true
            }
            (Some(Outcome::Authorized), Outcome::Authorized)
            | (Some(Outcome::Failed | Outcome::Voided), Outcome::Failed | Outcome::Voided) => false,
            // The money moved: the capture stands.
            (Some(Outcome::Captured), _) => {
                self.conflict(meta, ConflictKind::OutOfTurn { after: Outcome::Captured, outcome });
                false
            }
            // The card may be held or charged after all.
            (
                Some(after @ (Outcome::Failed | Outcome::Voided)),
                Outcome::Authorized | Outcome::Captured,
            ) => {
                self.conflict(meta, ConflictKind::OutOfTurn { after, outcome });
                true
            }
        };
        if applies {
            self.status = next;
        }
    }
}

impl Aggregate for Payment {
    type Event = PaymentEvent;

    fn stream_id(&self) -> Id<envelope::Aggregate> {
        self.id.cast()
    }

    fn apply(&mut self, meta: &EventMeta, event: &PaymentEvent) {
        if let PaymentEvent::Initiated(initiated) = event {
            if self.info.is_some() {
                return self.conflict(meta, ConflictKind::DuplicateInitiation);
            }
            self.info = Some(PaymentInfo {
                location: meta.location,
                order: initiated.order,
                check: initiated.check,
                tender: initiated.tender,
                amount: initiated.amount,
                initiated_by: meta.event_id,
            });
            return;
        }
        let Some(info) = &self.info else {
            return self.conflict(meta, ConflictKind::BeforeInitiation);
        };
        if meta.location != info.location {
            return self.conflict(meta, ConflictKind::WrongLocation);
        }
        // The payload's rules put every amount in one event in one currency.
        let currency = info.amount.currency();
        let (outcome, next, amount) = match event {
            PaymentEvent::Initiated(_) => return,
            PaymentEvent::Authorized(authorized) => (
                Outcome::Authorized,
                PaymentStatus::Authorized(authorized.clone()),
                Some(authorized.amount),
            ),
            PaymentEvent::Captured(captured) => (
                Outcome::Captured,
                PaymentStatus::Captured(captured.clone()),
                Some(captured.amount),
            ),
            PaymentEvent::Failed(ended) => {
                (Outcome::Failed, PaymentStatus::Failed(ended.clone()), None)
            }
            PaymentEvent::Voided(ended) => {
                (Outcome::Voided, PaymentStatus::Voided(ended.clone()), None)
            }
        };
        if amount.is_some_and(|amount| amount.currency() != currency) {
            return self.conflict(meta, ConflictKind::CurrencyMismatch);
        }
        self.record(meta, outcome, next);
    }

    fn skip(&mut self, meta: &EventMeta, schema: &SchemaRef, reason: &DecodeError) {
        self.skipped.push(Skipped {
            event: meta.event_id,
            schema: schema.clone(),
            reason: reason.clone(),
        });
    }
}
