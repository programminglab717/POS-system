//! The drawer session's state, and the fold that builds it from events (ADR-0031).
//!
//! A drawer session is single-writer: the device that opened it alone appends to it. The fold is
//! total, and no cash goes uncounted:
//!
//! - An event from another device than the opener's, or after the close, applies and is
//!   reported: only a forged or misrouted event is one, and the cash it records is real.
//! - Cash taken twice for one payment, an amount in another currency than the float's, an
//!   event before the opening or from another location: these don't apply, and are reported.
//! - A second opening, or a second close, doesn't apply, and is reported.

use std::collections::BTreeSet;

use keel_events::envelope::{self, Device, Event, Location, SchemaRef};
use keel_types::{Id, Money, MoneyError};

use super::events::{CashMoved, CashRefunded, CashTaken, DrawerClosed, DrawerEvent, Movement};
use crate::aggregate::{Aggregate, EventMeta, Skipped};
use crate::payment::Payment;
use crate::refund::Refund;
use crate::schema::DecodeError;

/// A drawer session: the cash in one drawer, from its opening to its close (domain model §13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawer {
    id: Id<Drawer>,
    info: Option<DrawerInfo>,
    sums: Option<Sums>,
    payments: BTreeSet<Id<Payment>>,
    /// The refunds whose cash it handed back (ADR-0032).
    refunds: BTreeSet<Id<Refund>>,
    no_sales: u64,
    closed: Option<DrawerClosed>,
    conflicts: Vec<Conflict>,
    skipped: Vec<Skipped>,
}

/// What a session's cash came to, each in its float's currency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sums {
    /// What its cash payments left in it.
    pub taken: Money,
    /// Paid in.
    pub paid_in: Money,
    /// Paid out.
    pub paid_out: Money,
    /// Dropped to the safe.
    pub dropped: Money,
    /// Handed back for refunds (ADR-0032).
    pub refunded: Money,
}

/// What a drawer session was opened with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawerInfo {
    /// Where.
    pub location: Id<Location>,
    /// The device that opened it, which alone writes it.
    pub device: Id<Device>,
    /// The cash put in to begin.
    pub float: Money,
    /// The event that opened it.
    pub opened_by: Id<Event>,
}

/// Something in a drawer session's history that a person should look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The event that caused it.
    pub event: Id<Event>,
    /// What happened.
    pub kind: ConflictKind,
}

/// What a drawer conflict is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictKind {
    /// The session was opened again; the second opening wasn't applied.
    DuplicateOpening,
    /// An event came before the session was opened; it wasn't applied.
    BeforeOpening,
    /// An event was recorded at another location; it wasn't applied.
    WrongLocation,
    /// An event was recorded by another device than the one that opened the session; it applied.
    NotOpener {
        /// The device that recorded it.
        by: Id<Device>,
    },
    /// An amount in another currency than the float's; it wasn't applied.
    CurrencyMismatch,
    /// Cash was taken again for a payment already counted; it wasn't counted twice.
    DuplicatePayment(Id<Payment>),
    /// An event came after the session closed; it applied, but the close's count stands.
    AfterClose,
    /// The session was closed again; the second close wasn't applied.
    DuplicateClose,
    /// Cash was handed back again for a refund already counted; it wasn't counted twice
    /// (ADR-0032).
    DuplicateRefund(Id<Refund>),
    /// An amount would overflow its sum; it wasn't applied. Found in review: it was reported as
    /// in another currency.
    Overflow,
}

impl Drawer {
    /// The session with identifier `id`, before any event: not yet opened.
    pub fn new(id: Id<Drawer>) -> Drawer {
        Drawer {
            id,
            info: None,
            sums: None,
            payments: BTreeSet::new(),
            refunds: BTreeSet::new(),
            no_sales: 0,
            closed: None,
            conflicts: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// The session's identifier, which is also its stream's.
    pub const fn id(&self) -> Id<Drawer> {
        self.id
    }

    /// What it was opened with; `None` until it is opened.
    pub const fn info(&self) -> Option<&DrawerInfo> {
        self.info.as_ref()
    }

    /// What its cash came to; `None` until it is opened.
    pub const fn sums(&self) -> Option<&Sums> {
        self.sums.as_ref()
    }

    /// The payments whose cash it took.
    pub fn payments(&self) -> &BTreeSet<Id<Payment>> {
        &self.payments
    }

    /// The refunds whose cash it handed back.
    pub fn refunds(&self) -> &BTreeSet<Id<Refund>> {
        &self.refunds
    }

    /// How many times it was opened with no sale.
    pub const fn no_sales(&self) -> u64 {
        self.no_sales
    }

    /// Its close, once counted.
    pub const fn closed(&self) -> Option<&DrawerClosed> {
        self.closed.as_ref()
    }

    /// Whether it is open: opened, and not yet closed.
    pub const fn is_open(&self) -> bool {
        self.info.is_some() && self.closed.is_none()
    }

    /// What it should hold: its float, the cash taken and paid in, less what was paid out and
    /// dropped. `None` until it is opened.
    ///
    /// # Errors
    /// [`MoneyError`] if the amounts overflow.
    pub fn expected(&self) -> Result<Option<Money>, MoneyError> {
        let (Some(info), Some(sums)) = (&self.info, &self.sums) else { return Ok(None) };
        Ok(Some(held(info.float, sums)?))
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

    /// Adds `amount` to the sum `to` picks, in the float's currency; reports a mismatch instead.
    fn add(&mut self, meta: &EventMeta, amount: Money, to: fn(&mut Sums) -> &mut Money) -> bool {
        let added = self.sums.as_mut().map(|sums| {
            let sum = to(sums);
            sum.checked_add(amount).map(|added| *sum = added)
        });
        match added {
            Some(Ok(())) => return true,
            Some(Err(MoneyError::Overflow)) => self.conflict(meta, ConflictKind::Overflow),
            _ => self.conflict(meta, ConflictKind::CurrencyMismatch),
        }
        false
    }

    fn apply_opened(&mut self, meta: &EventMeta, float: Money) {
        if self.info.is_some() {
            return self.conflict(meta, ConflictKind::DuplicateOpening);
        }
        let zero = Money::zero(float.currency());
        self.info = Some(DrawerInfo {
            location: meta.location,
            device: meta.origin_device,
            float,
            opened_by: meta.event_id,
        });
        self.sums = Some(Sums {
            taken: zero,
            paid_in: zero,
            paid_out: zero,
            dropped: zero,
            refunded: zero,
        });
    }

    fn apply_taken(&mut self, meta: &EventMeta, taken: &CashTaken) {
        if self.payments.contains(&taken.payment) {
            return self.conflict(meta, ConflictKind::DuplicatePayment(taken.payment));
        }
        if self.add(meta, taken.amount, |sums| &mut sums.taken) {
            self.payments.insert(taken.payment);
        }
    }

    fn apply_refunded(&mut self, meta: &EventMeta, refunded: &CashRefunded) {
        if self.refunds.contains(&refunded.refund) {
            return self.conflict(meta, ConflictKind::DuplicateRefund(refunded.refund));
        }
        if self.add(meta, refunded.amount, |sums| &mut sums.refunded) {
            self.refunds.insert(refunded.refund);
        }
    }

    fn apply_moved(&mut self, meta: &EventMeta, moved: &CashMoved) {
        let to: fn(&mut Sums) -> &mut Money = match moved.kind {
            Movement::PaidIn => |sums| &mut sums.paid_in,
            Movement::PaidOut => |sums| &mut sums.paid_out,
            Movement::Drop => |sums| &mut sums.dropped,
        };
        self.add(meta, moved.amount, to);
    }

    fn apply_closed(&mut self, meta: &EventMeta, closed: &DrawerClosed) {
        if self.closed.is_some() {
            return self.conflict(meta, ConflictKind::DuplicateClose);
        }
        let float = self.info.as_ref().map(|info| info.float.currency());
        if float.is_some_and(|currency| closed.counted.currency() != currency) {
            return self.conflict(meta, ConflictKind::CurrencyMismatch);
        }
        self.closed = Some(*closed);
    }
}

/// What a session opened with `float` should hold once its cash came to `sums`.
pub(super) fn held(float: Money, sums: &Sums) -> Result<Money, MoneyError> {
    let held = float.checked_add(sums.taken)?.checked_add(sums.paid_in)?;
    held.checked_sub(sums.paid_out)?.checked_sub(sums.dropped)?.checked_sub(sums.refunded)
}

impl Aggregate for Drawer {
    type Event = DrawerEvent;

    fn stream_id(&self) -> Id<envelope::Aggregate> {
        self.id.cast()
    }

    fn apply(&mut self, meta: &EventMeta, event: &DrawerEvent) {
        if let DrawerEvent::Opened { float } = event {
            return self.apply_opened(meta, *float);
        }
        let Some(info) = &self.info else {
            return self.conflict(meta, ConflictKind::BeforeOpening);
        };
        if meta.location != info.location {
            return self.conflict(meta, ConflictKind::WrongLocation);
        }
        if meta.origin_device != info.device {
            let by = meta.origin_device;
            self.conflict(meta, ConflictKind::NotOpener { by });
        }
        // Cash taken or moved after the close is real cash: it is counted, and reported.
        let after = self.closed.is_some() && !matches!(event, DrawerEvent::Closed(_));
        let before = self.conflicts.len();
        match event {
            DrawerEvent::Opened { .. } => {}
            DrawerEvent::CashTaken(taken) => self.apply_taken(meta, taken),
            DrawerEvent::CashMoved(moved) => self.apply_moved(meta, moved),
            DrawerEvent::NoSale { .. } => self.no_sales = self.no_sales.saturating_add(1),
            DrawerEvent::Closed(closed) => self.apply_closed(meta, closed),
            DrawerEvent::CashRefunded(refunded) => self.apply_refunded(meta, refunded),
        }
        // Reported once: an event that didn't apply has already said why.
        if after && self.conflicts.len() == before {
            self.conflict(meta, ConflictKind::AfterClose);
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
