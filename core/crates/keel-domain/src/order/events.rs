//! Order events, version 1, and their payloads.
//!
//! Every schema below is version 1. Keys and encodings follow [`crate::codec`]; "optional" fields
//! are omitted when absent, and in a change, `null` clears a field.
//!
//! | Schema | Keys |
//! |---|---|
//! | `order.created` | 1 channel, 2 mode, 3 currency, 4 revenue center (optional), 5 table (optional), 6 guest count (optional), 7 customer (optional), 8 owner (optional, a team member) |
//! | `order.attributes_changed` | 1 mode, 2 revenue center, 3 table, 4 guest count, 5 customer, 6 owner: each optional, at least one; all but the mode can be cleared |
//! | `order.line_added` | 1 line, 2 variant, 3 catalog version, 4 name, 5 tax category, 6 unit price, 7 quantity, 8 modifiers (an array, possibly empty), 9 seat (optional), 10 course (optional), 11 notes (optional) |
//! | `order.line_changed` | 1 line; 2 quantity, 3 modifiers, 4 seat, 5 course, 6 notes: each optional, at least one; seat, course and notes can be cleared |
//! | `order.line_removed` | 1 line |
//! | `order.lines_fired` | 1 lines (a set) |
//! | `order.line_voided` | 1 line, 2 reason code, 3 note (optional) |
//! | `order.line_comped` | 1 line, 2 reason code, 3 note (optional) |
//! | `order.voided` | 1 reason code, 2 note (optional) |
//! | `order.abandoned` | no keys: an empty map |
//! | `order.check_opened` | 1 check |
//! | `order.lines_allocated` | 1 allocations: an array, not empty |
//! | `order.check_closed` | 1 check, 2 rules version, 3 lines (an array of line charges, not empty), 4 taxes (an array of tax charges, possibly empty), 5 total, 6 payments (optional, a set) |
//! | `order.closed` | no keys: an empty map |
//! | `order.reopened` | 1 reason code, 2 note (optional) |
//! | `order.ownership_requested` | 1 lease: the one the device saw, which the request would replace |
//! | `order.ownership_granted` | 1 request (an event identifier), 2 device, 3 lease (from 1), 4 epoch (from 1) |
//! | `order.ownership_refused` | 1 request (an event identifier), 2 refusal |
//! | `order.ownership_overridden` | 1 lease: the one it overrides, 2 reason code, 3 note (optional) |
//!
//! A chosen modifier is a map: 1 modifier, 2 name, 3 prefix, 4 quantity, 5 placement, 6 unit
//! price, 7 modifiers (an array, possibly empty). An allocation is a map: 1 line, 2 check, 3
//! shares (a count up to 65,535). A line charge is a map: 1 line, 2 gross, 3 net, 4 tax. A tax
//! charge is a map: 1 tax, 2 taxable amount, 3 tax. A lease and an epoch are unsigned integers
//! of at most 2^63 − 1 (ADR-0021).
//!
//! Beyond the types, payloads must satisfy these rules:
//! - quantities are positive, and prices are zero or more;
//! - all the prices and amounts in one payload are in the same currency;
//! - a change changes at least one field;
//! - allocations are in strictly ascending order of line, then check, and each line's shares
//!   are in lowest terms;
//! - a closed check's line charges are in strictly ascending order of line, each with a net
//!   between zero and its gross and a tax of zero or more; its tax charges are in strictly
//!   ascending order of tax, each taxing more than zero, with a tax of zero or more; the taxes
//!   add up to the lines' tax; the total is the lines' net plus their tax; and payments are
//!   listed when the total is more than zero.

use core::num::{NonZeroU8, NonZeroU16};

use keel_events::cbor::Value;
use keel_events::envelope::{Customer, Event, SchemaRef, TeamMember};
use keel_types::{Currency, Id, Quantity};

use super::checks::{Check, LinesAllocated};
use super::closing::CheckClosed;
use super::ownership::{Lease, OwnershipGranted, Refusal};
use super::state::Line;
use super::types::{Channel, ChosenModifier, ItemSnapshot, Mode, Reason, modifier_currency};
use crate::codec::{Change, Fields, IdSet, PayloadError, Record};
use crate::refs::{RevenueCenter, Table};
use crate::schema::{DecodeError, DomainEvent, SchemaId};

/// An order was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderCreated {
    /// Where it was placed.
    pub channel: Channel,
    /// How it reaches the customer.
    pub mode: Mode,
    /// The currency of every price in the order.
    pub currency: Currency,
    /// The revenue center it belongs to.
    pub revenue_center: Option<Id<RevenueCenter>>,
    /// The table it is served at.
    pub table: Option<Id<Table>>,
    /// How many guests it is for.
    pub guest_count: Option<NonZeroU16>,
    /// The customer, as a reference into the PII vault.
    pub customer: Option<Id<Customer>>,
    /// The team member responsible for it, such as the server.
    pub owner: Option<Id<TeamMember>>,
}

/// Some of an order's attributes changed. Absent fields are unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttributesChanged {
    /// A new mode.
    pub mode: Option<Mode>,
    /// A new revenue center, or none.
    pub revenue_center: Option<Change<Id<RevenueCenter>>>,
    /// A new table, or none.
    pub table: Option<Change<Id<Table>>>,
    /// A new guest count, or none.
    pub guest_count: Option<Change<NonZeroU16>>,
    /// A new customer, or none.
    pub customer: Option<Change<Id<Customer>>>,
    /// A new owner, or none.
    pub owner: Option<Change<Id<TeamMember>>>,
}

impl AttributesChanged {
    fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.revenue_center.is_none()
            && self.table.is_none()
            && self.guest_count.is_none()
            && self.customer.is_none()
            && self.owner.is_none()
    }
}

/// A line was added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineAdded {
    /// The new line's identifier, minted by the device that added it.
    pub line: Id<Line>,
    /// What it sells.
    pub item: ItemSnapshot,
    /// How much: positive.
    pub quantity: Quantity,
    /// The modifiers chosen for it.
    pub modifiers: Vec<ChosenModifier>,
    /// The seat it is for.
    pub seat: Option<NonZeroU16>,
    /// The course it belongs to.
    pub course: Option<NonZeroU8>,
    /// Instructions for whoever prepares it.
    pub notes: Option<crate::codec::Note>,
}

/// Some of a line's details changed. Absent fields are unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineChanged {
    /// The line.
    pub line: Id<Line>,
    /// A new quantity: positive, in the line's unit.
    pub quantity: Option<Quantity>,
    /// New modifiers, replacing all the line's modifiers.
    pub modifiers: Option<Vec<ChosenModifier>>,
    /// A new seat, or none.
    pub seat: Option<Change<NonZeroU16>>,
    /// A new course, or none.
    pub course: Option<Change<NonZeroU8>>,
    /// New notes, or none.
    pub notes: Option<Change<crate::codec::Note>>,
}

impl LineChanged {
    /// A change to `line` that changes nothing yet: set the fields to change.
    pub const fn to(line: Id<Line>) -> LineChanged {
        LineChanged { line, quantity: None, modifiers: None, seat: None, course: None, notes: None }
    }

    fn is_empty(&self) -> bool {
        self.quantity.is_none()
            && self.modifiers.is_none()
            && self.seat.is_none()
            && self.course.is_none()
            && self.notes.is_none()
    }
}

/// Something that happened to an order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderEvent {
    /// The order was opened.
    Created(OrderCreated),
    /// Some of its attributes changed.
    AttributesChanged(AttributesChanged),
    /// A line was added.
    LineAdded(LineAdded),
    /// Some of a line's details changed.
    LineChanged(LineChanged),
    /// A line that wasn't fired was removed.
    LineRemoved {
        /// The line.
        line: Id<Line>,
    },
    /// Lines were sent to be prepared.
    LinesFired {
        /// The lines.
        lines: IdSet<Line>,
    },
    /// A fired line was voided: it won't be served or charged.
    LineVoided {
        /// The line.
        line: Id<Line>,
        /// Why.
        reason: Reason,
    },
    /// A line was given away: it is served, at no charge.
    LineComped {
        /// The line.
        line: Id<Line>,
        /// Why.
        reason: Reason,
    },
    /// The whole order was voided.
    Voided {
        /// Why.
        reason: Reason,
    },
    /// The order was dropped before anything in it was fired.
    Abandoned,
    /// A check was opened.
    CheckOpened {
        /// The new check's identifier, minted by the device that opened it.
        check: Id<Check>,
    },
    /// Lines were allocated to checks.
    LinesAllocated(LinesAllocated),
    /// A check was closed: its payments covered it.
    CheckClosed(CheckClosed),
    /// The order was closed: every check holding a live line was closed.
    Closed,
    /// A closed order, or an order with closed checks, was reopened: the order and every check
    /// are open again.
    Reopened {
        /// Why.
        reason: Reason,
    },
    /// A device asked the hub for the order (ADR-0021).
    OwnershipRequested {
        /// The lease the device saw, which the request would replace.
        lease: Lease,
    },
    /// The hub gave the order to the device that asked for it.
    OwnershipGranted(OwnershipGranted),
    /// The hub didn't give the order to the device that asked for it.
    OwnershipRefused {
        /// The request it answers.
        request: Id<Event>,
        /// Why.
        refusal: Refusal,
    },
    /// A device took the order on a manager's word, without the hub.
    OwnershipOverridden {
        /// The lease it overrode, which it replaces.
        lease: Lease,
        /// Why.
        reason: Reason,
    },
}

impl OrderEvent {
    /// Whether only the order's owning device may record the event: its structural and money
    /// changes (ADR-0021). The fold flags one recorded by any other device.
    pub const fn needs_ownership(&self) -> bool {
        matches!(
            self,
            OrderEvent::LineChanged(_)
                | OrderEvent::LineRemoved { .. }
                | OrderEvent::LineVoided { .. }
                | OrderEvent::LineComped { .. }
                | OrderEvent::Voided { .. }
                | OrderEvent::Abandoned
                | OrderEvent::CheckOpened { .. }
                | OrderEvent::LinesAllocated(_)
                | OrderEvent::CheckClosed(_)
                | OrderEvent::Closed
                | OrderEvent::Reopened { .. }
        )
    }
}

/// The schemas, in the order of `OrderEvent`'s variants.
const CREATED: SchemaId = SchemaId { name: "order.created", version: 1 };
const ATTRIBUTES_CHANGED: SchemaId = SchemaId { name: "order.attributes_changed", version: 1 };
const LINE_ADDED: SchemaId = SchemaId { name: "order.line_added", version: 1 };
const LINE_CHANGED: SchemaId = SchemaId { name: "order.line_changed", version: 1 };
const LINE_REMOVED: SchemaId = SchemaId { name: "order.line_removed", version: 1 };
const LINES_FIRED: SchemaId = SchemaId { name: "order.lines_fired", version: 1 };
const LINE_VOIDED: SchemaId = SchemaId { name: "order.line_voided", version: 1 };
const LINE_COMPED: SchemaId = SchemaId { name: "order.line_comped", version: 1 };
const VOIDED: SchemaId = SchemaId { name: "order.voided", version: 1 };
const ABANDONED: SchemaId = SchemaId { name: "order.abandoned", version: 1 };
const CHECK_OPENED: SchemaId = SchemaId { name: "order.check_opened", version: 1 };
const LINES_ALLOCATED: SchemaId = SchemaId { name: "order.lines_allocated", version: 1 };
const CHECK_CLOSED: SchemaId = SchemaId { name: "order.check_closed", version: 1 };
const CLOSED: SchemaId = SchemaId { name: "order.closed", version: 1 };
const REOPENED: SchemaId = SchemaId { name: "order.reopened", version: 1 };
const OWNERSHIP_REQUESTED: SchemaId = SchemaId { name: "order.ownership_requested", version: 1 };
const OWNERSHIP_GRANTED: SchemaId = SchemaId { name: "order.ownership_granted", version: 1 };
const OWNERSHIP_REFUSED: SchemaId = SchemaId { name: "order.ownership_refused", version: 1 };
const OWNERSHIP_OVERRIDDEN: SchemaId = SchemaId { name: "order.ownership_overridden", version: 1 };

impl DomainEvent for OrderEvent {
    const STREAM: &'static str = "order";

    const SCHEMAS: &'static [SchemaId] = &[
        CREATED,
        ATTRIBUTES_CHANGED,
        LINE_ADDED,
        LINE_CHANGED,
        LINE_REMOVED,
        LINES_FIRED,
        LINE_VOIDED,
        LINE_COMPED,
        VOIDED,
        ABANDONED,
        CHECK_OPENED,
        LINES_ALLOCATED,
        CHECK_CLOSED,
        CLOSED,
        REOPENED,
        OWNERSHIP_REQUESTED,
        OWNERSHIP_GRANTED,
        OWNERSHIP_REFUSED,
        OWNERSHIP_OVERRIDDEN,
    ];

    fn schema(&self) -> SchemaId {
        match self {
            OrderEvent::Created(_) => CREATED,
            OrderEvent::AttributesChanged(_) => ATTRIBUTES_CHANGED,
            OrderEvent::LineAdded(_) => LINE_ADDED,
            OrderEvent::LineChanged(_) => LINE_CHANGED,
            OrderEvent::LineRemoved { .. } => LINE_REMOVED,
            OrderEvent::LinesFired { .. } => LINES_FIRED,
            OrderEvent::LineVoided { .. } => LINE_VOIDED,
            OrderEvent::LineComped { .. } => LINE_COMPED,
            OrderEvent::Voided { .. } => VOIDED,
            OrderEvent::Abandoned => ABANDONED,
            OrderEvent::CheckOpened { .. } => CHECK_OPENED,
            OrderEvent::LinesAllocated(_) => LINES_ALLOCATED,
            OrderEvent::CheckClosed(_) => CHECK_CLOSED,
            OrderEvent::Closed => CLOSED,
            OrderEvent::Reopened { .. } => REOPENED,
            OrderEvent::OwnershipRequested { .. } => OWNERSHIP_REQUESTED,
            OrderEvent::OwnershipGranted(_) => OWNERSHIP_GRANTED,
            OrderEvent::OwnershipRefused { .. } => OWNERSHIP_REFUSED,
            OrderEvent::OwnershipOverridden { .. } => OWNERSHIP_OVERRIDDEN,
        }
    }

    fn to_value(&self) -> Value {
        let record = Record::default();
        match self {
            OrderEvent::CheckClosed(closed) => closed.record(record),
            OrderEvent::Created(created) => record
                .field(1, &created.channel)
                .field(2, &created.mode)
                .field(3, &created.currency)
                .optional(4, created.revenue_center.as_ref())
                .optional(5, created.table.as_ref())
                .optional(6, created.guest_count.as_ref())
                .optional(7, created.customer.as_ref())
                .optional(8, created.owner.as_ref()),
            OrderEvent::AttributesChanged(changed) => record
                .optional(1, changed.mode.as_ref())
                .change(2, changed.revenue_center.as_ref())
                .change(3, changed.table.as_ref())
                .change(4, changed.guest_count.as_ref())
                .change(5, changed.customer.as_ref())
                .change(6, changed.owner.as_ref()),
            OrderEvent::LineAdded(added) => record
                .field(1, &added.line)
                .field(2, &added.item.variant)
                .field(3, &added.item.catalog_version)
                .field(4, &added.item.name)
                .field(5, &added.item.tax_category)
                .field(6, &added.item.unit_price)
                .field(7, &added.quantity)
                .field(8, &added.modifiers)
                .optional(9, added.seat.as_ref())
                .optional(10, added.course.as_ref())
                .optional(11, added.notes.as_ref()),
            OrderEvent::LineChanged(changed) => record
                .field(1, &changed.line)
                .optional(2, changed.quantity.as_ref())
                .optional(3, changed.modifiers.as_ref())
                .change(4, changed.seat.as_ref())
                .change(5, changed.course.as_ref())
                .change(6, changed.notes.as_ref()),
            OrderEvent::LineRemoved { line } => record.field(1, line),
            OrderEvent::LinesFired { lines } => record.field(1, lines),
            OrderEvent::LineVoided { line, reason } | OrderEvent::LineComped { line, reason } => {
                record.field(1, line).field(2, &reason.code).optional(3, reason.note.as_ref())
            }
            OrderEvent::Voided { reason } | OrderEvent::Reopened { reason } => {
                record.field(1, &reason.code).optional(2, reason.note.as_ref())
            }
            OrderEvent::Abandoned | OrderEvent::Closed => record,
            OrderEvent::CheckOpened { check } => record.field(1, check),
            OrderEvent::LinesAllocated(allocated) => record.field(1, allocated),
            OrderEvent::OwnershipRequested { lease } => record.field(1, lease),
            OrderEvent::OwnershipGranted(granted) => record
                .field(1, &granted.request)
                .field(2, &granted.device)
                .field(3, &granted.lease)
                .field(4, &granted.epoch),
            OrderEvent::OwnershipRefused { request, refusal } => {
                record.field(1, request).field(2, refusal)
            }
            OrderEvent::OwnershipOverridden { lease, reason } => {
                record.field(1, lease).field(2, &reason.code).optional(3, reason.note.as_ref())
            }
        }
        .build()
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<OrderEvent, DecodeError> {
        if !Self::SCHEMAS.iter().any(|known| known.matches(schema)) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let event = if CREATED.matches(schema) {
            OrderEvent::Created(read_created(&mut fields)?)
        } else if ATTRIBUTES_CHANGED.matches(schema) {
            OrderEvent::AttributesChanged(read_attributes_changed(&mut fields)?)
        } else if LINE_ADDED.matches(schema) {
            OrderEvent::LineAdded(read_line_added(&mut fields)?)
        } else if LINE_CHANGED.matches(schema) {
            OrderEvent::LineChanged(read_line_changed(&mut fields)?)
        } else if LINE_REMOVED.matches(schema) {
            OrderEvent::LineRemoved { line: fields.required(1, "line")? }
        } else if LINES_FIRED.matches(schema) {
            OrderEvent::LinesFired { lines: fields.required(1, "lines")? }
        } else if LINE_VOIDED.matches(schema) || LINE_COMPED.matches(schema) {
            let line = fields.required(1, "line")?;
            let reason =
                Reason { code: fields.required(2, "reason")?, note: fields.optional(3, "note")? };
            if LINE_VOIDED.matches(schema) {
                OrderEvent::LineVoided { line, reason }
            } else {
                OrderEvent::LineComped { line, reason }
            }
        } else if VOIDED.matches(schema) || REOPENED.matches(schema) {
            let reason =
                Reason { code: fields.required(1, "reason")?, note: fields.optional(2, "note")? };
            if VOIDED.matches(schema) {
                OrderEvent::Voided { reason }
            } else {
                OrderEvent::Reopened { reason }
            }
        } else if ABANDONED.matches(schema) {
            OrderEvent::Abandoned
        } else if CLOSED.matches(schema) {
            OrderEvent::Closed
        } else if CHECK_CLOSED.matches(schema) {
            OrderEvent::CheckClosed(CheckClosed::read(&mut fields)?)
        } else if CHECK_OPENED.matches(schema) {
            OrderEvent::CheckOpened { check: fields.required(1, "check")? }
        } else if LINES_ALLOCATED.matches(schema) {
            OrderEvent::LinesAllocated(fields.required(1, "allocations")?)
        } else if OWNERSHIP_REQUESTED.matches(schema) {
            OrderEvent::OwnershipRequested { lease: fields.required(1, "lease")? }
        } else if OWNERSHIP_GRANTED.matches(schema) {
            OrderEvent::OwnershipGranted(read_granted(&mut fields)?)
        } else if OWNERSHIP_REFUSED.matches(schema) {
            OrderEvent::OwnershipRefused {
                request: fields.required(1, "request")?,
                refusal: fields.required(2, "refusal")?,
            }
        } else if OWNERSHIP_OVERRIDDEN.matches(schema) {
            OrderEvent::OwnershipOverridden {
                lease: fields.required(1, "lease")?,
                reason: Reason {
                    code: fields.required(2, "reason")?,
                    note: fields.optional(3, "note")?,
                },
            }
        } else {
            return Err(DecodeError::UnknownSchema);
        };
        fields.finish()?;
        Ok(event)
    }
}

fn read_created(fields: &mut Fields<'_>) -> Result<OrderCreated, PayloadError> {
    Ok(OrderCreated {
        channel: fields.required(1, "channel")?,
        mode: fields.required(2, "mode")?,
        currency: fields.required(3, "currency")?,
        revenue_center: fields.optional(4, "revenue center")?,
        table: fields.optional(5, "table")?,
        guest_count: fields.optional(6, "guest count")?,
        customer: fields.optional(7, "customer")?,
        owner: fields.optional(8, "owner")?,
    })
}

fn read_granted(fields: &mut Fields<'_>) -> Result<OwnershipGranted, PayloadError> {
    let granted = OwnershipGranted {
        request: fields.required(1, "request")?,
        device: fields.required(2, "device")?,
        lease: fields.required(3, "lease")?,
        epoch: fields.required(4, "epoch")?,
    };
    // A grant replaces a lease, so it never gives the first.
    if granted.lease == Lease::FIRST {
        return Err(PayloadError::Invalid("lease"));
    }
    Ok(granted)
}

fn read_attributes_changed(fields: &mut Fields<'_>) -> Result<AttributesChanged, PayloadError> {
    let changed = AttributesChanged {
        mode: fields.optional(1, "mode")?,
        revenue_center: fields.change(2, "revenue center")?,
        table: fields.change(3, "table")?,
        guest_count: fields.change(4, "guest count")?,
        customer: fields.change(5, "customer")?,
        owner: fields.change(6, "owner")?,
    };
    if changed.is_empty() {
        return Err(PayloadError::EmptyChange);
    }
    Ok(changed)
}

fn read_line_added(fields: &mut Fields<'_>) -> Result<LineAdded, PayloadError> {
    let added = LineAdded {
        line: fields.required(1, "line")?,
        item: ItemSnapshot {
            variant: fields.required(2, "variant")?,
            catalog_version: fields.required(3, "catalog version")?,
            name: fields.required(4, "name")?,
            tax_category: fields.required(5, "tax category")?,
            unit_price: fields.required(6, "unit price")?,
        },
        quantity: fields.required(7, "quantity")?,
        modifiers: fields.required(8, "modifiers")?,
        seat: fields.optional(9, "seat")?,
        course: fields.optional(10, "course")?,
        notes: fields.optional(11, "notes")?,
    };
    if added.item.unit_price.is_negative() {
        return Err(PayloadError::Invalid("unit price"));
    }
    if !added.quantity.is_positive() {
        return Err(PayloadError::Invalid("quantity"));
    }
    let currency = added.item.unit_price.currency();
    if !added.modifiers.iter().all(|modifier| modifier.is_priced_in(currency)) {
        return Err(PayloadError::Invalid("modifiers"));
    }
    Ok(added)
}

fn read_line_changed(fields: &mut Fields<'_>) -> Result<LineChanged, PayloadError> {
    let changed = LineChanged {
        line: fields.required(1, "line")?,
        quantity: fields.optional(2, "quantity")?,
        modifiers: fields.optional(3, "modifiers")?,
        seat: fields.change(4, "seat")?,
        course: fields.change(5, "course")?,
        notes: fields.change(6, "notes")?,
    };
    if changed.is_empty() {
        return Err(PayloadError::EmptyChange);
    }
    if changed.quantity.is_some_and(|quantity| !quantity.is_positive()) {
        return Err(PayloadError::Invalid("quantity"));
    }
    if let Some(modifiers) = &changed.modifiers {
        let consistent = modifier_currency(modifiers).is_none_or(|currency| {
            modifiers.iter().all(|modifier| modifier.is_priced_in(currency))
        });
        if !consistent {
            return Err(PayloadError::Invalid("modifiers"));
        }
    }
    Ok(changed)
}
