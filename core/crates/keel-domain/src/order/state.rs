//! The order's state, and the fold that builds it from events.
//!
//! The fold is total. Events that don't fit the order's state, because their device validated
//! them against a different, concurrent view, still have a defined outcome, following the
//! conflict rules of the offline and sync design (§5.2), and leave a [`Conflict`] for a person
//! to review:
//!
//! - Lines added on different devices are all kept.
//! - When two devices change the same attribute, the later change in canonical order wins.
//! - A removal or void wins over a change made concurrently; the change is reported.
//! - A void wins over a concurrent fire; the kitchen is told the fired line is void.
//! - The first removal, void or comp of a line wins; later ones change nothing.
//! - An event that would put money in another currency, or a quantity in another unit, into the
//!   order isn't applied, so every price in an order can be added up.
//!
//! Events recorded before the order was created, or by a device at another location, aren't
//! applied either. Nothing is lost: every event stays in the log, and every one that isn't
//! applied, or applies with surprising effect, leaves a conflict.

use core::num::{NonZeroU8, NonZeroU16};

use keel_events::envelope::{self, Customer, Event, Location, SchemaRef, TeamMember};
use keel_types::{Currency, Id, Quantity};

use super::events::{AttributesChanged, LineAdded, LineChanged, OrderCreated, OrderEvent};
use super::types::{Channel, ChosenModifier, ItemSnapshot, Mode, Reason, modifier_currency};
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::{Change, Note};
use crate::refs::{RevenueCenter, Table};
use crate::schema::DecodeError;

/// An order: the universal transaction (domain model §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    id: Id<Order>,
    info: Option<OrderInfo>,
    lines: Vec<Line>,
    status: OrderStatus,
    conflicts: Vec<Conflict>,
    skipped: Vec<Skipped>,
}

/// What an order was created with, and its attributes since.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderInfo {
    /// The order's location: where it was created.
    pub location: Id<Location>,
    /// Where it was placed.
    pub channel: Channel,
    /// The currency of every price in it.
    pub currency: Currency,
    /// The event that created it.
    pub created_by: Id<Event>,
    /// How it reaches the customer.
    pub mode: Mode,
    /// Its revenue center.
    pub revenue_center: Option<Id<RevenueCenter>>,
    /// Its table.
    pub table: Option<Id<Table>>,
    /// How many guests it is for.
    pub guest_count: Option<NonZeroU16>,
    /// Its customer.
    pub customer: Option<Id<Customer>>,
    /// The team member responsible for it.
    pub owner: Option<Id<TeamMember>>,
}

/// Whether an order is still being worked on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderStatus {
    /// Open for changes.
    Active,
    /// Voided as a whole.
    Voided(Reason),
    /// Dropped before anything in it was fired.
    Abandoned,
}

/// Where an active order is in its life, derived from its lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Nothing ordered yet: no live lines.
    Draft,
    /// Some live lines haven't been fired.
    Open,
    /// Every live line has been fired.
    Submitted,
}

/// A line of an order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    id: Id<Line>,
    item: ItemSnapshot,
    quantity: Quantity,
    modifiers: Vec<ChosenModifier>,
    seat: Option<NonZeroU16>,
    course: Option<NonZeroU8>,
    notes: Option<Note>,
    status: LineStatus,
    comp: Option<Reason>,
    fired: bool,
}

/// Where a line is in its life.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineStatus {
    /// Rung up, not yet sent to be prepared.
    Pending,
    /// Sent to be prepared.
    Fired,
    /// Taken off before it was fired.
    Removed,
    /// Taken off after it was fired: it won't be served or charged.
    Voided(Reason),
}

impl Line {
    /// The line's identifier.
    pub const fn id(&self) -> Id<Line> {
        self.id
    }

    /// What it sells.
    pub const fn item(&self) -> &ItemSnapshot {
        &self.item
    }

    /// How much.
    pub const fn quantity(&self) -> Quantity {
        self.quantity
    }

    /// The modifiers chosen for it.
    pub fn modifiers(&self) -> &[ChosenModifier] {
        &self.modifiers
    }

    /// The seat it is for.
    pub const fn seat(&self) -> Option<NonZeroU16> {
        self.seat
    }

    /// The course it belongs to.
    pub const fn course(&self) -> Option<NonZeroU8> {
        self.course
    }

    /// Instructions for whoever prepares it.
    pub const fn notes(&self) -> Option<&Note> {
        self.notes.as_ref()
    }

    /// Where it is in its life.
    pub const fn status(&self) -> &LineStatus {
        &self.status
    }

    /// Why it was comped, if it was: it is then served at no charge.
    pub const fn comp(&self) -> Option<&Reason> {
        self.comp.as_ref()
    }

    /// Whether the line still counts: it hasn't been removed or voided.
    pub const fn is_live(&self) -> bool {
        matches!(self.status, LineStatus::Pending | LineStatus::Fired)
    }

    /// Whether the line was ever sent to be prepared, even if it was removed or voided later, or
    /// the fire arrived after it was taken off: whoever prepares it may have made it.
    pub const fn was_fired(&self) -> bool {
        self.fired
    }
}

/// Something in an order's history that a person should look at.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// The order was created again; the second creation wasn't applied.
    DuplicateCreation,
    /// An event came before the order was created; it wasn't applied.
    BeforeCreation,
    /// An event was recorded by a device at another location; it wasn't applied.
    WrongLocation,
    /// A line was added with an identifier already in use; it wasn't added again.
    DuplicateLine(Id<Line>),
    /// An event refers to a line the order doesn't have.
    UnknownLine(Id<Line>),
    /// An event would have put money in another currency into the order; it wasn't applied.
    CurrencyMismatch(Id<Line>),
    /// A change would have given a line a quantity in another unit; it wasn't applied.
    UnitMismatch(Id<Line>),
    /// A line was changed after it was fired: the kitchen may need to know.
    ChangedAfterFire(Id<Line>),
    /// A line was changed after it was removed or voided; the removal wins.
    ChangedAfterRemoval(Id<Line>),
    /// A fired line was removed rather than voided.
    RemovedAfterFire(Id<Line>),
    /// A line was fired after it was removed or voided; the removal wins, and the kitchen
    /// should be told.
    FiredAfterRemoval(Id<Line>),
    /// A line was comped after it was removed or voided; the comp has no effect.
    CompedAfterRemoval(Id<Line>),
    /// A line was added to an order that was already voided or abandoned.
    AddedToClosedOrder(Id<Line>),
    /// A line was fired on an order that was already voided or abandoned.
    FiredOnClosedOrder(Id<Line>),
    /// The order was abandoned although it had live lines, or lines that were fired, which may
    /// have been made.
    AbandonedWithLines,
}

/// An event that wasn't applied because it couldn't be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The event.
    pub event: Id<Event>,
    /// Its schema.
    pub schema: SchemaRef,
    /// Why it couldn't be decoded.
    pub reason: DecodeError,
}

impl Order {
    /// The order with identifier `id`, before any event: not yet created.
    pub const fn new(id: Id<Order>) -> Order {
        Order {
            id,
            info: None,
            lines: Vec::new(),
            status: OrderStatus::Active,
            conflicts: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// The order's identifier, which is also its event stream's.
    pub const fn id(&self) -> Id<Order> {
        self.id
    }

    /// What the order was created with, and its attributes since; `None` until it is created.
    pub const fn info(&self) -> Option<&OrderInfo> {
        self.info.as_ref()
    }

    /// Whether the order is still being worked on.
    pub const fn status(&self) -> &OrderStatus {
        &self.status
    }

    /// Where an active order is in its life.
    pub fn stage(&self) -> Stage {
        let mut live = self.lines.iter().filter(|line| line.is_live()).peekable();
        if live.peek().is_none() {
            Stage::Draft
        } else if live.any(|line| line.status == LineStatus::Pending) {
            Stage::Open
        } else {
            Stage::Submitted
        }
    }

    /// Every line, including removed and voided ones, in the order they were added.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The line with identifier `id`.
    pub fn line(&self, id: Id<Line>) -> Option<&Line> {
        self.lines.iter().find(|line| line.id == id)
    }

    /// The lines that still count, in the order they were added.
    pub fn live_lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|line| line.is_live())
    }

    /// Everything in the order's history that a person should look at.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Events that weren't applied because they couldn't be decoded.
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// Whether events from a newer kernel were skipped: this kernel needs updating to show the
    /// order fully.
    pub fn needs_update(&self) -> bool {
        self.skipped.iter().any(|skipped| skipped.reason == DecodeError::UnknownSchema)
    }

    fn conflict(&mut self, meta: &EventMeta, kind: ConflictKind) {
        self.conflicts.push(Conflict { event: meta.event_id, kind });
    }

    fn is_closed(&self) -> bool {
        self.status != OrderStatus::Active
    }

    fn apply_created(&mut self, meta: &EventMeta, created: &OrderCreated) {
        if self.info.is_some() {
            return self.conflict(meta, ConflictKind::DuplicateCreation);
        }
        self.info = Some(OrderInfo {
            location: meta.location,
            channel: created.channel,
            currency: created.currency,
            created_by: meta.event_id,
            mode: created.mode,
            revenue_center: created.revenue_center,
            table: created.table,
            guest_count: created.guest_count,
            customer: created.customer,
            owner: created.owner,
        });
    }

    fn apply_attributes(info: &mut OrderInfo, changed: &AttributesChanged) {
        fn set<T: Copy>(field: &mut Option<T>, change: Option<&Change<T>>) {
            if let Some(change) = change {
                *field = change.into_option();
            }
        }
        if let Some(mode) = changed.mode {
            info.mode = mode;
        }
        set(&mut info.revenue_center, changed.revenue_center.as_ref());
        set(&mut info.table, changed.table.as_ref());
        set(&mut info.guest_count, changed.guest_count.as_ref());
        set(&mut info.customer, changed.customer.as_ref());
        set(&mut info.owner, changed.owner.as_ref());
    }

    fn apply_line_added(&mut self, meta: &EventMeta, currency: Currency, added: &LineAdded) {
        if self.line(added.line).is_some() {
            return self.conflict(meta, ConflictKind::DuplicateLine(added.line));
        }
        // The payload's rules put the modifiers' prices in the item's currency.
        if added.item.unit_price.currency() != currency {
            return self.conflict(meta, ConflictKind::CurrencyMismatch(added.line));
        }
        self.lines.push(Line {
            id: added.line,
            item: added.item.clone(),
            quantity: added.quantity,
            modifiers: added.modifiers.clone(),
            seat: added.seat,
            course: added.course,
            notes: added.notes.clone(),
            status: LineStatus::Pending,
            comp: None,
            fired: false,
        });
        if self.is_closed() {
            self.conflict(meta, ConflictKind::AddedToClosedOrder(added.line));
        }
    }

    fn apply_line_changed(&mut self, meta: &EventMeta, currency: Currency, changed: &LineChanged) {
        let id = changed.line;
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        let conflict = if !line.is_live() {
            Some(ConflictKind::ChangedAfterRemoval(id))
        } else if changed.quantity.is_some_and(|quantity| quantity.unit() != line.quantity.unit()) {
            Some(ConflictKind::UnitMismatch(id))
        } else if changed
            .modifiers
            .as_deref()
            .and_then(modifier_currency)
            .is_some_and(|modifiers| modifiers != currency)
        {
            Some(ConflictKind::CurrencyMismatch(id))
        } else {
            if let Some(quantity) = changed.quantity {
                line.quantity = quantity;
            }
            if let Some(modifiers) = &changed.modifiers {
                line.modifiers.clone_from(modifiers);
            }
            if let Some(seat) = changed.seat {
                line.seat = seat.into_option();
            }
            if let Some(course) = changed.course {
                line.course = course.into_option();
            }
            if let Some(notes) = &changed.notes {
                line.notes = notes.clone().into_option();
            }
            (line.status == LineStatus::Fired).then_some(ConflictKind::ChangedAfterFire(id))
        };
        if let Some(kind) = conflict {
            self.conflict(meta, kind);
        }
    }

    fn apply_line_removed(&mut self, meta: &EventMeta, id: Id<Line>) {
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        match line.status {
            LineStatus::Pending => line.status = LineStatus::Removed,
            LineStatus::Fired => {
                line.status = LineStatus::Removed;
                self.conflict(meta, ConflictKind::RemovedAfterFire(id));
            }
            LineStatus::Removed | LineStatus::Voided(_) => {}
        }
    }

    fn apply_line_fired(&mut self, meta: &EventMeta, id: Id<Line>) {
        let closed = self.is_closed();
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        line.fired = true;
        match line.status {
            LineStatus::Pending => {
                line.status = LineStatus::Fired;
                if closed {
                    self.conflict(meta, ConflictKind::FiredOnClosedOrder(id));
                }
            }
            LineStatus::Fired => {}
            LineStatus::Removed | LineStatus::Voided(_) => {
                self.conflict(meta, ConflictKind::FiredAfterRemoval(id));
            }
        }
    }

    fn apply_line_voided(&mut self, meta: &EventMeta, id: Id<Line>, reason: &Reason) {
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        if line.is_live() {
            line.status = LineStatus::Voided(reason.clone());
        }
    }

    fn apply_line_comped(&mut self, meta: &EventMeta, id: Id<Line>, reason: &Reason) {
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        if !line.is_live() {
            return self.conflict(meta, ConflictKind::CompedAfterRemoval(id));
        }
        if line.comp.is_none() {
            line.comp = Some(reason.clone());
        }
    }

    fn apply_closed(&mut self, meta: &EventMeta, status: OrderStatus) {
        if self.is_closed() {
            return;
        }
        let abandoned_with_lines = status == OrderStatus::Abandoned
            && self.lines.iter().any(|line| line.is_live() || line.fired);
        self.status = status;
        if abandoned_with_lines {
            self.conflict(meta, ConflictKind::AbandonedWithLines);
        }
    }
}

impl Aggregate for Order {
    type Event = OrderEvent;

    fn stream_id(&self) -> Id<envelope::Aggregate> {
        self.id.cast()
    }

    fn apply(&mut self, meta: &EventMeta, event: &OrderEvent) {
        if let OrderEvent::Created(created) = event {
            return self.apply_created(meta, created);
        }
        let Some(info) = &self.info else {
            return self.conflict(meta, ConflictKind::BeforeCreation);
        };
        if meta.location != info.location {
            return self.conflict(meta, ConflictKind::WrongLocation);
        }
        let currency = info.currency;
        match event {
            OrderEvent::Created(_) => {}
            OrderEvent::AttributesChanged(changed) => {
                if let Some(info) = &mut self.info {
                    Order::apply_attributes(info, changed);
                }
            }
            OrderEvent::LineAdded(added) => self.apply_line_added(meta, currency, added),
            OrderEvent::LineChanged(changed) => self.apply_line_changed(meta, currency, changed),
            OrderEvent::LineRemoved { line } => self.apply_line_removed(meta, *line),
            OrderEvent::LinesFired { lines } => {
                for line in lines.iter() {
                    self.apply_line_fired(meta, line);
                }
            }
            OrderEvent::LineVoided { line, reason } => self.apply_line_voided(meta, *line, reason),
            OrderEvent::LineComped { line, reason } => self.apply_line_comped(meta, *line, reason),
            OrderEvent::Voided { reason } => {
                self.apply_closed(meta, OrderStatus::Voided(reason.clone()));
            }
            OrderEvent::Abandoned => self.apply_closed(meta, OrderStatus::Abandoned),
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
