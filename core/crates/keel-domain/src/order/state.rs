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
//! - For each line, the later allocation to checks wins. An allocation naming a check that
//!   doesn't exist yet doesn't apply to that line; one of a removed or voided line changes
//!   nothing.
//! - A closed check's snapshot stands: the money moved. A line with a part on a closed check
//!   still follows a change, removal, void or comp (the kitchen must know), but not a new
//!   allocation, and the order reports it. A line on a check that closed without it, because a
//!   device added or moved it there concurrently, moves to an open check, unpaid.
//! - A new line goes to the first open check. When every check is closed, it goes to a new
//!   check, opened for it: a post-close check.
//!
//! Events recorded before the order was created, or by a device at another location, aren't
//! applied either. Nothing is lost: every event stays in the log, and every one that isn't
//! applied, or applies with surprising effect, leaves a conflict.
//!
//! Who owns the order follows its ownership events (ADR-0021, [`super::ownership`]):
//! - The device that created the order owns it under lease 0.
//! - A grant, or a manager's override, from the order's current lease gives the order to its
//!   device under the next lease. One from a lease that has moved on doesn't apply, and is
//!   flagged; an override that applies is flagged too, for reconciliation.
//! - A request waits until an answer names it, wherever the answer folds: a hub whose clock is
//!   behind can answer before the request in canonical order, or even before the order's
//!   creation, where the answer itself doesn't apply.
//! - An event only the owner may record, recorded by another device, still applies, and is
//!   flagged.

use core::num::{NonZeroU8, NonZeroU16, NonZeroU32};

use std::collections::BTreeSet;

use keel_events::envelope::{self, Customer, Device, Event, Location, SchemaRef, TeamMember};
use keel_types::{Currency, Id, Quantity};

use super::checks::{Check, CheckShare, LinesAllocated};
use super::closing::CheckClosed;
use super::events::{AttributesChanged, LineAdded, LineChanged, OrderCreated, OrderEvent};
use super::ownership::{Lease, Ownership, OwnershipGranted, Request};
use super::types::{Channel, ChosenModifier, ItemSnapshot, Mode, Reason, modifier_currency};
use crate::aggregate::{Aggregate, EventMeta, Skipped};
use crate::codec::{Change, Note};
use crate::refs::{RevenueCenter, Table};
use crate::schema::DecodeError;

/// An order: the universal transaction (domain model §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    id: Id<Order>,
    info: Option<OrderInfo>,
    lines: Vec<Line>,
    checks: Vec<Check>,
    status: OrderStatus,
    ownership: Option<Ownership>,
    requests: Vec<Request>,
    answered: BTreeSet<Id<Event>>,
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
    /// Closed: every check holding a live line was closed. A manager can reopen it.
    Closed,
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
    allocation: Vec<CheckShare>,
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

    /// The checks the line belongs to, in ascending order of identifier, with each one's shares
    /// of it: never empty.
    pub fn allocation(&self) -> &[CheckShare] {
        &self.allocation
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
    /// A line was added to an order that was already closed, voided or abandoned: it is unpaid.
    AddedToClosedOrder(Id<Line>),
    /// A line was fired on an order that was already closed, voided or abandoned.
    FiredOnClosedOrder(Id<Line>),
    /// The order was abandoned although it had live lines, or lines that were fired, which may
    /// have been made.
    AbandonedWithLines,
    /// A check was opened with an identifier already in use; it wasn't opened again.
    DuplicateCheck(Id<Check>),
    /// An event refers to a check the order doesn't have; it didn't apply to the check or the
    /// lines it named.
    UnknownCheck(Id<Check>),
    /// A check was closed again; the first close stands.
    DuplicateClose(Id<Check>),
    /// A check was closed with amounts in another currency than the order's; it wasn't closed.
    CheckCurrencyMismatch(Id<Check>),
    /// A check was closed charging for a line that isn't on it: moved elsewhere, removed or
    /// voided concurrently. The check's snapshot stands.
    ChargedOffCheck(Id<Line>),
    /// A check was closed without a line on it, which a device added or moved there
    /// concurrently: the line's part moved to an open check, unpaid.
    LeftOffCheck(Id<Line>),
    /// A line with a part on a closed check was changed in quantity or modifiers, removed,
    /// voided or comped: the line follows the event, and the check's snapshot stands.
    ChangedOnClosedCheck(Id<Line>),
    /// An allocation would have moved a line onto or off a closed check; it didn't apply to the
    /// line.
    AllocatedOnClosedCheck(Id<Line>),
    /// The order was closed while a check holding a live line was still open: its lines are
    /// unpaid.
    ClosedWithOpenCheck(Id<Check>),
    /// An event that only the order's owner may record was recorded by another device: it
    /// applied, and a person should look.
    NotOwner {
        /// The device that recorded it.
        by: Id<Device>,
        /// The device that owned the order.
        owner: Id<Device>,
    },
    /// The hub gave the order away from a lease that had already moved on, by an override it
    /// hadn't heard of; the grant didn't apply.
    StaleGrant,
    /// A device took the order on a manager's word from the device named, without the hub: for
    /// reconciliation.
    Overridden(Id<Device>),
    /// A device overrode a lease that had already moved on; the override didn't apply.
    StaleOverride,
}

impl Order {
    /// The order with identifier `id`, before any event: not yet created.
    pub const fn new(id: Id<Order>) -> Order {
        Order {
            id,
            info: None,
            lines: Vec::new(),
            checks: Vec::new(),
            status: OrderStatus::Active,
            ownership: None,
            requests: Vec::new(),
            answered: BTreeSet::new(),
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

    /// Who owns the order, and under which lease; `None` until it is created.
    pub const fn ownership(&self) -> Option<Ownership> {
        self.ownership
    }

    /// The requests for the order that no answer names yet, in canonical order.
    pub fn requests(&self) -> &[Request] {
        &self.requests
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

    /// The order's checks, in the order they were opened: the main check first, once the order
    /// is created.
    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// The check with identifier `id`.
    pub fn check(&self, id: Id<Check>) -> Option<&Check> {
        self.checks.iter().find(|check| check.id == id)
    }

    /// The identifier of the main check: the order's own.
    pub const fn main_check(&self) -> Id<Check> {
        self.id.cast()
    }

    /// Whether `line` is live with a part on a closed check: what it costs, and how it is split,
    /// are then frozen until the order is reopened.
    pub fn is_frozen(&self, line: &Line) -> bool {
        line.is_live()
            && line
                .allocation
                .iter()
                .any(|share| self.check(share.check).is_some_and(|check| !check.is_open()))
    }

    /// Whether a live line has a part on `check`.
    pub(super) fn holds_live_line(&self, check: Id<Check>) -> bool {
        self.live_lines().any(|line| line.allocation.iter().any(|share| share.check == check))
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

    fn has_ended(&self) -> bool {
        matches!(self.status, OrderStatus::Voided(_) | OrderStatus::Abandoned)
    }

    /// The number the next check gets.
    fn next_number(&self) -> NonZeroU32 {
        let opened = u32::try_from(self.checks.len()).ok().and_then(|n| n.checked_add(1));
        opened.and_then(NonZeroU32::new).unwrap_or(NonZeroU32::MAX)
    }

    /// The check a line, or a part of one, goes to when it can't go where it would: the first
    /// open check that doesn't already hold a part of the line (`holding`), or else a new check
    /// opened for it, whose identifier is that of the event at `meta`. `None` if that identifier
    /// is already a check's, which only a forged event could cause.
    fn open_check_for(&mut self, meta: &EventMeta, holding: &[CheckShare]) -> Option<Id<Check>> {
        let holds = |check: Id<Check>| holding.iter().any(|share| share.check == check);
        if let Some(open) = self.checks.iter().find(|check| check.is_open() && !holds(check.id)) {
            return Some(open.id);
        }
        let id = meta.event_id.cast();
        if self.check(id).is_some() {
            return None;
        }
        self.checks.push(Check { id, number: self.next_number(), closed: None });
        Some(id)
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
        self.ownership = Some(Ownership { device: meta.origin_device, lease: Lease::FIRST });
        self.checks.push(Check { id: self.main_check(), number: NonZeroU32::MIN, closed: None });
    }

    fn apply_requested(&mut self, meta: &EventMeta, lease: Lease) {
        if !self.answered.contains(&meta.event_id) {
            let request = Request { event: meta.event_id, device: meta.origin_device, lease };
            self.requests.push(request);
        }
    }

    /// Notes that an answer names `request`: it no longer waits, even if it folds later.
    fn answer(&mut self, request: Id<Event>) {
        self.answered.insert(request);
        self.requests.retain(|waiting| waiting.event != request);
    }

    fn apply_granted(&mut self, meta: &EventMeta, granted: &OwnershipGranted) {
        let current = self.ownership.and_then(|ownership| ownership.lease.next());
        if current == Some(granted.lease) {
            self.ownership = Some(Ownership { device: granted.device, lease: granted.lease });
        } else {
            self.conflict(meta, ConflictKind::StaleGrant);
        }
    }

    fn apply_overridden(&mut self, meta: &EventMeta, lease: Lease) {
        let current = self.ownership.filter(|ownership| ownership.lease == lease);
        match current.zip(lease.next()) {
            Some((previous, next)) => {
                self.ownership = Some(Ownership { device: meta.origin_device, lease: next });
                self.conflict(meta, ConflictKind::Overridden(previous.device));
            }
            None => self.conflict(meta, ConflictKind::StaleOverride),
        }
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
        let check = self.open_check_for(meta, &[]).unwrap_or(self.main_check());
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
            allocation: vec![CheckShare { check, shares: NonZeroU16::MIN }],
        });
        if self.is_closed() {
            self.conflict(meta, ConflictKind::AddedToClosedOrder(added.line));
        }
    }

    /// Whether the line with identifier `id` is frozen, before an event applies to it.
    fn frozen(&self, id: Id<Line>) -> bool {
        self.line(id).is_some_and(|line| self.is_frozen(line))
    }

    fn apply_line_changed(&mut self, meta: &EventMeta, currency: Currency, changed: &LineChanged) {
        let id = changed.line;
        let frozen = self.frozen(id);
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        let refused = if !line.is_live() {
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
            None
        };
        if let Some(kind) = refused {
            return self.conflict(meta, kind);
        }
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
        if line.status == LineStatus::Fired {
            self.conflict(meta, ConflictKind::ChangedAfterFire(id));
        }
        // Seats, courses and notes don't change what a line costs.
        if frozen && (changed.quantity.is_some() || changed.modifiers.is_some()) {
            self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
        }
    }

    fn apply_line_removed(&mut self, meta: &EventMeta, id: Id<Line>) {
        let frozen = self.frozen(id);
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        let fired = match line.status {
            LineStatus::Pending => false,
            LineStatus::Fired => true,
            LineStatus::Removed | LineStatus::Voided(_) => return,
        };
        line.status = LineStatus::Removed;
        if fired {
            self.conflict(meta, ConflictKind::RemovedAfterFire(id));
        }
        if frozen {
            self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
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
        let frozen = self.frozen(id);
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        if line.is_live() {
            line.status = LineStatus::Voided(reason.clone());
            if frozen {
                self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
            }
        }
    }

    fn apply_line_comped(&mut self, meta: &EventMeta, id: Id<Line>, reason: &Reason) {
        let frozen = self.frozen(id);
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        if !line.is_live() {
            return self.conflict(meta, ConflictKind::CompedAfterRemoval(id));
        }
        if line.comp.is_none() {
            line.comp = Some(reason.clone());
            if frozen {
                self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
            }
        }
    }

    fn apply_check_opened(&mut self, meta: &EventMeta, id: Id<Check>) {
        if self.check(id).is_some() {
            return self.conflict(meta, ConflictKind::DuplicateCheck(id));
        }
        self.checks.push(Check { id, number: self.next_number(), closed: None });
    }

    fn apply_lines_allocated(&mut self, meta: &EventMeta, allocated: &LinesAllocated) {
        let mut unknown_checks: Vec<Id<Check>> = Vec::new();
        for (id, shares) in allocated.lines() {
            let unknown =
                shares.iter().map(|share| share.check).find(|&check| self.check(check).is_none());
            let onto_closed = shares
                .iter()
                .any(|share| self.check(share.check).is_some_and(|check| !check.is_open()));
            let frozen = self.frozen(id);
            let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
                self.conflict(meta, ConflictKind::UnknownLine(id));
                continue;
            };
            if !line.is_live() {
                continue;
            }
            match unknown {
                Some(check) => {
                    if !unknown_checks.contains(&check) {
                        unknown_checks.push(check);
                    }
                }
                None if frozen || onto_closed => {
                    self.conflict(meta, ConflictKind::AllocatedOnClosedCheck(id));
                }
                None => line.allocation = shares,
            }
        }
        unknown_checks.sort_by_key(|check| check.to_bytes());
        for check in unknown_checks {
            self.conflict(meta, ConflictKind::UnknownCheck(check));
        }
    }

    fn apply_check_closed(&mut self, meta: &EventMeta, currency: Currency, closed: &CheckClosed) {
        let id = closed.check;
        let Some(check) = self.checks.iter_mut().find(|check| check.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownCheck(id));
        };
        if check.closed.is_some() {
            return self.conflict(meta, ConflictKind::DuplicateClose(id));
        }
        // The payload's rules put every amount in the total's currency.
        if closed.total.currency() != currency {
            return self.conflict(meta, ConflictKind::CheckCurrencyMismatch(id));
        }
        check.closed = Some(closed.clone());
        let on_check =
            |line: &Line| line.is_live() && line.allocation.iter().any(|share| share.check == id);
        // Lines the check was charged for that aren't on it.
        for charge in &closed.lines {
            let kind = match self.line(charge.line) {
                None => ConflictKind::UnknownLine(charge.line),
                Some(line) if on_check(line) => continue,
                Some(_) => ConflictKind::ChargedOffCheck(charge.line),
            };
            self.conflict(meta, kind);
        }
        // Lines on the check that it wasn't charged for: their parts move to an open check.
        let left: Vec<usize> = (0..self.lines.len())
            .filter(|&index| {
                self.lines
                    .get(index)
                    .is_some_and(|line| on_check(line) && closed.line(line.id).is_none())
            })
            .collect();
        for index in left {
            let Some(line) = self.lines.get(index) else { continue };
            let (line_id, holding) = (line.id, line.allocation.clone());
            if let Some(target) = self.open_check_for(meta, &holding)
                && let Some(line) = self.lines.get_mut(index)
            {
                for share in line.allocation.iter_mut().filter(|share| share.check == id) {
                    share.check = target;
                }
                line.allocation.sort_by_key(|share| share.check.to_bytes());
            }
            self.conflict(meta, ConflictKind::LeftOffCheck(line_id));
        }
    }

    fn apply_order_closed(&mut self, meta: &EventMeta) {
        if self.status != OrderStatus::Active {
            return;
        }
        self.status = OrderStatus::Closed;
        let unpaid: Vec<Id<Check>> = self
            .checks
            .iter()
            .filter(|check| check.is_open() && self.holds_live_line(check.id))
            .map(Check::id)
            .collect();
        for check in unpaid {
            self.conflict(meta, ConflictKind::ClosedWithOpenCheck(check));
        }
    }

    fn apply_reopened(&mut self) {
        if self.has_ended() {
            return;
        }
        self.status = OrderStatus::Active;
        for check in &mut self.checks {
            check.closed = None;
        }
    }

    fn apply_ended(&mut self, meta: &EventMeta, status: OrderStatus) {
        if self.has_ended() {
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
        // An answer names its request wherever it folds, so that the request never waits for
        // another, even when the answer folds before the order's creation and doesn't apply.
        if let OrderEvent::OwnershipGranted(OwnershipGranted { request, .. })
        | OrderEvent::OwnershipRefused { request, .. } = event
        {
            self.answer(*request);
        }
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
        if event.needs_ownership()
            && let Some(owner) = self.ownership
            && owner.device != meta.origin_device
        {
            let by = meta.origin_device;
            self.conflict(meta, ConflictKind::NotOwner { by, owner: owner.device });
        }
        match event {
            // Applied above: the creation, and a refusal's answering its request, which is all a
            // refusal does.
            OrderEvent::Created(_) | OrderEvent::OwnershipRefused { .. } => {}
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
                self.apply_ended(meta, OrderStatus::Voided(reason.clone()));
            }
            OrderEvent::Abandoned => self.apply_ended(meta, OrderStatus::Abandoned),
            OrderEvent::CheckOpened { check } => self.apply_check_opened(meta, *check),
            OrderEvent::LinesAllocated(allocated) => self.apply_lines_allocated(meta, allocated),
            OrderEvent::CheckClosed(closed) => self.apply_check_closed(meta, currency, closed),
            OrderEvent::Closed => self.apply_order_closed(meta),
            OrderEvent::Reopened { .. } => self.apply_reopened(),
            OrderEvent::OwnershipRequested { lease } => self.apply_requested(meta, *lease),
            OrderEvent::OwnershipGranted(granted) => self.apply_granted(meta, granted),
            OrderEvent::OwnershipOverridden { lease, .. } => self.apply_overridden(meta, *lease),
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
