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
//!
//! A chosen modifier is a map: 1 modifier, 2 name, 3 prefix, 4 quantity, 5 placement, 6 unit
//! price, 7 modifiers (an array, possibly empty).
//!
//! Beyond the types, payloads must satisfy these rules:
//! - quantities are positive, and prices are zero or more;
//! - all the prices in one payload are in the same currency;
//! - a change changes at least one field.

use core::num::{NonZeroU8, NonZeroU16};

use keel_events::cbor::Value;
use keel_events::envelope::{Customer, SchemaRef, TeamMember};
use keel_types::{Currency, Id, Quantity};

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
        }
    }

    fn to_value(&self) -> Value {
        let record = Record::default();
        match self {
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
            OrderEvent::Voided { reason } => {
                record.field(1, &reason.code).optional(2, reason.note.as_ref())
            }
            OrderEvent::Abandoned => record,
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
        } else if VOIDED.matches(schema) {
            let reason =
                Reason { code: fields.required(1, "reason")?, note: fields.optional(2, "note")? };
            OrderEvent::Voided { reason }
        } else if ABANDONED.matches(schema) {
            OrderEvent::Abandoned
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
