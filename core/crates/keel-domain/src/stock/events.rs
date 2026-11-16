//! Stock events, version 1, and their payloads (ADR-0036).
//!
//! Keys and encodings follow [`crate::codec`]; "optional" fields are omitted when absent. A time
//! is microseconds since the Unix epoch.
//!
//! | Schema | Keys |
//! |---|---|
//! | `stock.moved` | 1 movement (0 received, 1 adjusted, 2 wasted, 3 reserved, 4 released), 2 quantity, 3 reason code (optional), 4 note (optional) |
//! | `stock.counted` | 1 quantity, 2 as of, 3 note (optional) |
//!
//! Beyond the types, payloads must satisfy these rules: a movement's quantity is more than zero,
//! but an adjustment's, which is anything but zero; a count's is zero or more; a movement's note
//! comes with a reason.

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_types::{Quantity, Timestamp};

use crate::codec::{Fields, Note, PayloadError, Record, code_enum};
use crate::order::Reason;
use crate::schema::{DecodeError, DomainEvent, SchemaId};

code_enum! {
    /// What a movement does to an item's stock.
    pub enum Movement {
        /// Received, as from a delivery: on hand rises.
        Received = 0,
        /// Adjusted, up or down, as a correction: on hand moves by the quantity, which is signed.
        Adjusted = 1,
        /// Wasted, as spoiled, broken or returned and not put back: on hand falls.
        Wasted = 2,
        /// Set aside, as for a customer: reserved rises.
        Reserved = 3,
        /// No longer set aside: reserved falls.
        Released = 4,
    }
}

/// Stock moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockMoved {
    /// How.
    pub movement: Movement,
    /// How much: more than zero, but an adjustment's, which is signed and never zero.
    pub quantity: Quantity,
    /// Why, if the person said: a reason code, with a note.
    pub reason: Option<Reason>,
}

/// Stock was counted, as of a time: what was on hand then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockCounted {
    /// What was counted: zero or more.
    pub quantity: Quantity,
    /// When it was on hand: what moved after it still counts.
    pub as_of: Timestamp,
    /// A note.
    pub note: Option<Note>,
}

/// Something that happened to an item's stock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StockEvent {
    /// It moved.
    Moved(StockMoved),
    /// It was counted.
    Counted(StockCounted),
}

const MOVED: SchemaId = SchemaId { name: "stock.moved", version: 1 };
const COUNTED: SchemaId = SchemaId { name: "stock.counted", version: 1 };

impl StockEvent {
    /// Checks the payload's rules: see the module's.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        match self {
            StockEvent::Moved(moved) => {
                let valid = if moved.movement == Movement::Adjusted {
                    !moved.quantity.is_zero()
                } else {
                    moved.quantity.is_positive()
                };
                if valid { Ok(()) } else { Err(PayloadError::Invalid("quantity")) }
            }
            StockEvent::Counted(counted) if counted.quantity.is_negative() => {
                Err(PayloadError::Invalid("quantity"))
            }
            StockEvent::Counted(_) => Ok(()),
        }
    }

    /// The quantity it names.
    pub const fn quantity(&self) -> Quantity {
        match self {
            StockEvent::Moved(moved) => moved.quantity,
            StockEvent::Counted(counted) => counted.quantity,
        }
    }
}

impl DomainEvent for StockEvent {
    const STREAM: &'static str = "stock";

    const SCHEMAS: &'static [SchemaId] = &[MOVED, COUNTED];

    fn schema(&self) -> SchemaId {
        match self {
            StockEvent::Moved(_) => MOVED,
            StockEvent::Counted(_) => COUNTED,
        }
    }

    fn to_value(&self) -> Value {
        let record = Record::default();
        match self {
            StockEvent::Moved(moved) => record
                .field(1, &moved.movement)
                .field(2, &moved.quantity)
                .optional(3, moved.reason.as_ref().map(|reason| &reason.code))
                .optional(4, moved.reason.as_ref().and_then(|reason| reason.note.as_ref())),
            StockEvent::Counted(counted) => record
                .field(1, &counted.quantity)
                .field(2, &counted.as_of)
                .optional(3, counted.note.as_ref()),
        }
        .build()
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<StockEvent, DecodeError> {
        if !Self::SCHEMAS.iter().any(|known| known.matches(schema)) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let event = if MOVED.matches(schema) {
            let movement = fields.required(1, "movement")?;
            let quantity = fields.required(2, "quantity")?;
            let code = fields.optional(3, "reason")?;
            let note = fields.optional(4, "note")?;
            let reason = match (code, note) {
                (Some(code), note) => Some(Reason { code, note }),
                (None, None) => None,
                (None, Some(_)) => return Err(PayloadError::Invalid("note").into()),
            };
            StockEvent::Moved(StockMoved { movement, quantity, reason })
        } else if COUNTED.matches(schema) {
            StockEvent::Counted(StockCounted {
                quantity: fields.required(1, "quantity")?,
                as_of: fields.required(2, "as of")?,
                note: fields.optional(3, "note")?,
            })
        } else {
            return Err(DecodeError::UnknownSchema);
        };
        fields.finish()?;
        event.validate()?;
        Ok(event)
    }
}
