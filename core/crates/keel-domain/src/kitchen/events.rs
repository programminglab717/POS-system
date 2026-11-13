//! Kitchen ticket events, version 1, and their payload (ADR-0034).
//!
//! Keys and encodings follow [`crate::codec`]; "optional" fields are omitted when absent.
//!
//! | Schema | Keys |
//! |---|---|
//! | `kitchen.ticket_sent` | 1 order, 2 station, 3 order mode, 4 items (at least one) |
//! | `kitchen.ticket_shown` | none |
//! | `kitchen.ticket_bumped` | none |
//! | `kitchen.ticket_recalled` | none |
//! | `kitchen.ticket_rerouted` | 1 to: 0 the station's printer, 1 the fallback printer, 2 nowhere |
//!
//! An item is `{1 line, 2 name, 3 quantity, 4 modifiers, 5 notes (optional), 6 allergens
//! (optional), 7 component of (optional)}`, and a modifier `{1 name, 2 prefix, 3 quantity, 4
//! depth}`.
//!
//! Beyond the types, the payload must satisfy these rules: a ticket has at least one item and at
//! most [`MAX_ITEMS`]; an item's quantity is more than nothing; its modifiers, at most
//! [`MAX_MODIFIERS`], are listed each before those under it, the first at depth 1 and each at
//! most one deeper than the one before, none deeper than [`MAX_DEPTH`].

use core::num::NonZeroU8;
use std::collections::BTreeSet;

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_types::{Id, Quantity};

use crate::codec::{CodeSet, Field, Fields, Name, Note, PayloadError, Record, code_enum};
use crate::order::{Line, Mode, Order, Prefix};
use crate::profile::Allergen;
use crate::refs::Station;
use crate::schema::{DecodeError, DomainEvent, SchemaId};

/// The most items a kitchen ticket holds.
pub const MAX_ITEMS: usize = 500;
/// The most modifiers an item lists.
pub const MAX_MODIFIERS: usize = 255;
/// The deepest a modifier is listed.
pub const MAX_DEPTH: u8 = 5;

code_enum! {
    /// Where a ticket no screen showed was sent instead.
    pub enum RerouteTo {
        /// Its station's printer.
        StationPrinter = 0,
        /// The kitchen's fallback printer.
        FallbackPrinter = 1,
        /// Nowhere: there was no printer to send it to.
        Nowhere = 2,
    }
}

/// A modifier of a kitchen ticket's item, as the kitchen reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KitchenModifier {
    /// Its name, such as "Oat milk".
    pub name: Name,
    /// How it changes the item: "No" and "Extra" are shown apart.
    pub prefix: Prefix,
    /// How many times it is applied.
    pub quantity: NonZeroU8,
    /// How deep it is: 1 for one chosen on the item, 2 for one chosen under that, and so on.
    pub depth: NonZeroU8,
}

/// An item of a kitchen ticket: a line, or one of its modifiers as a component of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KitchenItem {
    /// The order's line it makes.
    pub line: Id<Line>,
    /// What to make, such as "Latte (Large)", or "Hash brown" for a component.
    pub name: Name,
    /// How many: more than nothing.
    pub quantity: Quantity,
    /// Its modifiers, each before those under it.
    pub modifiers: Vec<KitchenModifier>,
    /// The line's notes, if it has some.
    pub notes: Option<Note>,
    /// The allergens it contains, from the catalog.
    pub allergens: BTreeSet<Allergen>,
    /// For a component, the line's name, such as "Breakfast sandwich".
    pub component_of: Option<Name>,
}

/// A ticket was sent to a station: what it is to make, for which order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketSent {
    /// The order.
    pub order: Id<Order>,
    /// The station.
    pub station: Id<Station>,
    /// How the order reaches its customer: eaten in, taken out, ...
    pub mode: Mode,
    /// What to make: at least one item.
    pub items: Vec<KitchenItem>,
}

/// Something that happened to a kitchen ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KitchenEvent {
    /// It was sent to its station.
    Sent(TicketSent),
    /// A screen showed it.
    Shown,
    /// Its station is done with it.
    Bumped,
    /// It is back on its station's screen.
    Recalled,
    /// No screen showed it in time: it was sent elsewhere.
    Rerouted(RerouteTo),
}

const SENT: SchemaId = SchemaId { name: "kitchen.ticket_sent", version: 1 };
const SHOWN: SchemaId = SchemaId { name: "kitchen.ticket_shown", version: 1 };
const BUMPED: SchemaId = SchemaId { name: "kitchen.ticket_bumped", version: 1 };
const RECALLED: SchemaId = SchemaId { name: "kitchen.ticket_recalled", version: 1 };
const REROUTED: SchemaId = SchemaId { name: "kitchen.ticket_rerouted", version: 1 };

impl KitchenEvent {
    /// Checks the payload's rules: see the module's.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        let KitchenEvent::Sent(sent) = self else { return Ok(()) };
        if sent.items.is_empty() || sent.items.len() > MAX_ITEMS {
            return Err(PayloadError::Invalid("items"));
        }
        for item in &sent.items {
            if item.quantity.micros() <= 0 {
                return Err(PayloadError::Invalid("quantity"));
            }
            if item.modifiers.len() > MAX_MODIFIERS {
                return Err(PayloadError::Invalid("modifiers"));
            }
            let mut deepest = 0_u8;
            for modifier in &item.modifiers {
                let depth = modifier.depth.get();
                if depth > deepest.saturating_add(1) || depth > MAX_DEPTH {
                    return Err(PayloadError::Invalid("depth"));
                }
                deepest = depth;
            }
        }
        Ok(())
    }
}

impl DomainEvent for KitchenEvent {
    const STREAM: &'static str = "kitchen_ticket";

    const SCHEMAS: &'static [SchemaId] = &[SENT, SHOWN, BUMPED, RECALLED, REROUTED];

    fn schema(&self) -> SchemaId {
        match self {
            KitchenEvent::Sent(_) => SENT,
            KitchenEvent::Shown => SHOWN,
            KitchenEvent::Bumped => BUMPED,
            KitchenEvent::Recalled => RECALLED,
            KitchenEvent::Rerouted(_) => REROUTED,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            KitchenEvent::Sent(sent) => Record::default()
                .field(1, &sent.order)
                .field(2, &sent.station)
                .field(3, &sent.mode)
                .field(4, &sent.items)
                .build(),
            KitchenEvent::Shown | KitchenEvent::Bumped | KitchenEvent::Recalled => {
                Record::default().build()
            }
            KitchenEvent::Rerouted(to) => Record::default().field(1, to).build(),
        }
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<KitchenEvent, DecodeError> {
        let mut fields = Fields::read(payload)?;
        let event = if SENT.matches(schema) {
            KitchenEvent::Sent(TicketSent {
                order: fields.required(1, "order")?,
                station: fields.required(2, "station")?,
                mode: fields.required(3, "order mode")?,
                items: fields.required(4, "items")?,
            })
        } else if SHOWN.matches(schema) {
            KitchenEvent::Shown
        } else if BUMPED.matches(schema) {
            KitchenEvent::Bumped
        } else if RECALLED.matches(schema) {
            KitchenEvent::Recalled
        } else if REROUTED.matches(schema) {
            KitchenEvent::Rerouted(fields.required(1, "to")?)
        } else {
            return Err(DecodeError::UnknownSchema);
        };
        fields.finish()?;
        event.validate()?;
        Ok(event)
    }
}

impl Field for KitchenModifier {
    fn to_value(&self) -> Value {
        Record::default()
            .field(1, &self.name)
            .field(2, &self.prefix)
            .field(3, &self.quantity)
            .field(4, &self.depth)
            .build()
    }

    fn from_value(value: &Value) -> Option<KitchenModifier> {
        let mut fields = Fields::read(value).ok()?;
        let modifier = KitchenModifier {
            name: fields.required(1, "name").ok()?,
            prefix: fields.required(2, "prefix").ok()?,
            quantity: fields.required(3, "quantity").ok()?,
            depth: fields.required(4, "depth").ok()?,
        };
        fields.finish().ok()?;
        Some(modifier)
    }
}

impl Field for KitchenItem {
    fn to_value(&self) -> Value {
        let allergens = (!self.allergens.is_empty()).then(|| CodeSet(self.allergens.clone()));
        Record::default()
            .field(1, &self.line)
            .field(2, &self.name)
            .field(3, &self.quantity)
            .field(4, &self.modifiers)
            .optional(5, self.notes.as_ref())
            .optional(6, allergens.as_ref())
            .optional(7, self.component_of.as_ref())
            .build()
    }

    fn from_value(value: &Value) -> Option<KitchenItem> {
        let mut fields = Fields::read(value).ok()?;
        let allergens: Option<CodeSet<Allergen>> = fields.optional(6, "allergens").ok()?;
        let item = KitchenItem {
            line: fields.required(1, "line").ok()?,
            name: fields.required(2, "name").ok()?,
            quantity: fields.required(3, "quantity").ok()?,
            modifiers: fields.required(4, "modifiers").ok()?,
            notes: fields.optional(5, "notes").ok()?,
            allergens: allergens.map(|codes| codes.0).unwrap_or_default(),
            component_of: fields.optional(7, "component of").ok()?,
        };
        fields.finish().ok()?;
        Some(item)
    }
}
