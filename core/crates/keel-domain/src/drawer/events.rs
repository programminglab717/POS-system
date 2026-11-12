//! Drawer events, version 1, and their payloads (ADR-0031).
//!
//! Every schema below is version 1. Keys and encodings follow [`crate::codec`]; "optional" fields
//! are omitted when absent.
//!
//! | Schema | Keys |
//! |---|---|
//! | `drawer.opened` | 1 float |
//! | `drawer.cash_taken` | 1 payment, 2 amount |
//! | `drawer.cash_moved` | 1 kind, 2 amount, 3 reason code, 4 note (optional) |
//! | `drawer.no_sale` | 1 reason code, 2 note (optional) |
//! | `drawer.closed` | 1 counted, 2 expected |
//!
//! Beyond the types, payloads must satisfy these rules:
//! - a float is zero or more; cash taken and moved are more than zero; a count is zero or more;
//! - a close's count and expected amount are in the same currency.

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_types::{Id, Money};

use crate::codec::{Fields, PayloadError, Record, code_enum};
use crate::order::Reason;
use crate::payment::Payment;
use crate::schema::{DecodeError, DomainEvent, SchemaId};

code_enum! {
    /// How cash moved in or out of a drawer other than by a sale.
    pub enum Movement {
        /// Cash put in: change brought from the safe, say.
        PaidIn = 0,
        /// Cash taken out for an expense: a delivery paid, say.
        PaidOut = 1,
        /// Cash taken to the safe, so the drawer holds less.
        Drop = 2,
    }
}

impl Movement {
    /// Whether the movement takes cash out of the drawer.
    pub const fn takes_out(self) -> bool {
        matches!(self, Movement::PaidOut | Movement::Drop)
    }
}

/// Cash a cash payment left in the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CashTaken {
    /// The payment.
    pub payment: Id<Payment>,
    /// What it left in the drawer: its amount, tip and cash rounding. More than zero.
    pub amount: Money,
}

/// Cash moved in or out of the drawer, for a reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CashMoved {
    /// How.
    pub kind: Movement,
    /// How much: more than zero.
    pub amount: Money,
    /// Why.
    pub reason: Reason,
}

/// The drawer was counted and closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawerClosed {
    /// What was counted in it: zero or more.
    pub counted: Money,
    /// What the device expected it to hold, when it was closed.
    pub expected: Money,
}

impl DrawerClosed {
    /// The variance: what was counted less what was expected. Over is more than zero, short
    /// less. `None` if the two can't be subtracted, which a decoded payload never has.
    pub fn variance(&self) -> Option<Money> {
        self.counted.checked_sub(self.expected).ok()
    }
}

/// Something that happened to a drawer session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawerEvent {
    /// The session opened, with its float.
    Opened {
        /// The cash put in to begin: zero or more.
        float: Money,
    },
    /// A cash payment left cash in the drawer.
    CashTaken(CashTaken),
    /// Cash was paid in or out, or dropped.
    CashMoved(CashMoved),
    /// The drawer was opened with no sale, for a reason.
    NoSale {
        /// Why.
        reason: Reason,
    },
    /// The drawer was counted and the session closed.
    Closed(DrawerClosed),
}

const OPENED: SchemaId = SchemaId { name: "drawer.opened", version: 1 };
const CASH_TAKEN: SchemaId = SchemaId { name: "drawer.cash_taken", version: 1 };
const CASH_MOVED: SchemaId = SchemaId { name: "drawer.cash_moved", version: 1 };
const NO_SALE: SchemaId = SchemaId { name: "drawer.no_sale", version: 1 };
const CLOSED: SchemaId = SchemaId { name: "drawer.closed", version: 1 };

impl DrawerEvent {
    /// Checks the payload's rules: see the module's.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        match self {
            DrawerEvent::Opened { float } if float.is_negative() => {
                Err(PayloadError::Invalid("float"))
            }
            DrawerEvent::CashTaken(taken) if !taken.amount.is_positive() => {
                Err(PayloadError::Invalid("amount"))
            }
            DrawerEvent::CashMoved(moved) if !moved.amount.is_positive() => {
                Err(PayloadError::Invalid("amount"))
            }
            DrawerEvent::Closed(closed) if closed.counted.is_negative() => {
                Err(PayloadError::Invalid("counted"))
            }
            DrawerEvent::Closed(closed)
                if closed.counted.currency() != closed.expected.currency() =>
            {
                Err(PayloadError::Invalid("expected"))
            }
            _ => Ok(()),
        }
    }
}

impl DomainEvent for DrawerEvent {
    const STREAM: &'static str = "drawer";

    const SCHEMAS: &'static [SchemaId] = &[OPENED, CASH_TAKEN, CASH_MOVED, NO_SALE, CLOSED];

    fn schema(&self) -> SchemaId {
        match self {
            DrawerEvent::Opened { .. } => OPENED,
            DrawerEvent::CashTaken(_) => CASH_TAKEN,
            DrawerEvent::CashMoved(_) => CASH_MOVED,
            DrawerEvent::NoSale { .. } => NO_SALE,
            DrawerEvent::Closed(_) => CLOSED,
        }
    }

    fn to_value(&self) -> Value {
        let record = Record::default();
        match self {
            DrawerEvent::Opened { float } => record.field(1, float),
            DrawerEvent::CashTaken(taken) => {
                record.field(1, &taken.payment).field(2, &taken.amount)
            }
            DrawerEvent::CashMoved(moved) => record
                .field(1, &moved.kind)
                .field(2, &moved.amount)
                .field(3, &moved.reason.code)
                .optional(4, moved.reason.note.as_ref()),
            DrawerEvent::NoSale { reason } => {
                record.field(1, &reason.code).optional(2, reason.note.as_ref())
            }
            DrawerEvent::Closed(closed) => {
                record.field(1, &closed.counted).field(2, &closed.expected)
            }
        }
        .build()
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<DrawerEvent, DecodeError> {
        if !Self::SCHEMAS.iter().any(|known| known.matches(schema)) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let event = if OPENED.matches(schema) {
            DrawerEvent::Opened { float: fields.required(1, "float")? }
        } else if CASH_TAKEN.matches(schema) {
            DrawerEvent::CashTaken(CashTaken {
                payment: fields.required(1, "payment")?,
                amount: fields.required(2, "amount")?,
            })
        } else if CASH_MOVED.matches(schema) {
            DrawerEvent::CashMoved(CashMoved {
                kind: fields.required(1, "kind")?,
                amount: fields.required(2, "amount")?,
                reason: Reason {
                    code: fields.required(3, "reason")?,
                    note: fields.optional(4, "note")?,
                },
            })
        } else if NO_SALE.matches(schema) {
            DrawerEvent::NoSale {
                reason: Reason {
                    code: fields.required(1, "reason")?,
                    note: fields.optional(2, "note")?,
                },
            }
        } else if CLOSED.matches(schema) {
            DrawerEvent::Closed(DrawerClosed {
                counted: fields.required(1, "counted")?,
                expected: fields.required(2, "expected")?,
            })
        } else {
            return Err(DecodeError::UnknownSchema);
        };
        fields.finish()?;
        event.validate()?;
        Ok(event)
    }
}
