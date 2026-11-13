//! Refund events, version 1, and their payloads (ADR-0032).
//!
//! Every schema below is version 1. Keys and encodings follow [`crate::codec`]; "optional" fields
//! are omitted when absent.
//!
//! | Schema | Keys |
//! |---|---|
//! | `refund.initiated` | 1 order, 2 tender, 3 amount |
//! | `refund.completed` | 1 amount, 2 rounding (optional) |
//!
//! Beyond the types, payloads must satisfy these rules:
//! - what a refund is initiated for, and what it refunded, are more than zero;
//! - a completion's rounding isn't zero, and is in its amount's currency.

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_types::{Id, Money};

use crate::codec::{Fields, PayloadError, Record};
use crate::order::Order;
use crate::payment::Tender;
use crate::schema::{DecodeError, DomainEvent, SchemaId};

/// A refund was started, for an order's return, by a tender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefundInitiated {
    /// The order whose lines were returned.
    pub order: Id<Order>,
    /// How it is paid back.
    pub tender: Tender,
    /// How much is owed: more than zero.
    pub amount: Money,
}

/// The money was paid back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefundCompleted {
    /// What was refunded: more than zero.
    pub amount: Money,
    /// What cash rounding added to what was handed back, if it did: never zero.
    pub rounding: Option<Money>,
}

impl RefundCompleted {
    /// What was handed back: the amount and its rounding. `None` if they don't add up in one
    /// currency, which a decoded payload never has.
    pub fn handed(&self) -> Option<Money> {
        match self.rounding {
            Some(rounding) => self.amount.checked_add(rounding).ok(),
            None => Some(self.amount),
        }
    }
}

/// Something that happened to a refund.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundEvent {
    /// The refund was started.
    Initiated(RefundInitiated),
    /// The money was paid back.
    Completed(RefundCompleted),
}

const INITIATED: SchemaId = SchemaId { name: "refund.initiated", version: 1 };
const COMPLETED: SchemaId = SchemaId { name: "refund.completed", version: 1 };

impl RefundEvent {
    /// Checks the payload's rules: see the module's.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        match self {
            RefundEvent::Initiated(initiated) if !initiated.amount.is_positive() => {
                Err(PayloadError::Invalid("amount"))
            }
            RefundEvent::Completed(completed) if !completed.amount.is_positive() => {
                Err(PayloadError::Invalid("amount"))
            }
            RefundEvent::Completed(RefundCompleted { amount, rounding: Some(rounding) })
                if rounding.is_zero() || rounding.currency() != amount.currency() =>
            {
                Err(PayloadError::Invalid("rounding"))
            }
            _ => Ok(()),
        }
    }
}

impl DomainEvent for RefundEvent {
    const STREAM: &'static str = "refund";

    const SCHEMAS: &'static [SchemaId] = &[INITIATED, COMPLETED];

    fn schema(&self) -> SchemaId {
        match self {
            RefundEvent::Initiated(_) => INITIATED,
            RefundEvent::Completed(_) => COMPLETED,
        }
    }

    fn to_value(&self) -> Value {
        let record = Record::default();
        match self {
            RefundEvent::Initiated(initiated) => record
                .field(1, &initiated.order)
                .field(2, &initiated.tender)
                .field(3, &initiated.amount),
            RefundEvent::Completed(completed) => {
                record.field(1, &completed.amount).optional(2, completed.rounding.as_ref())
            }
        }
        .build()
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<RefundEvent, DecodeError> {
        if !Self::SCHEMAS.iter().any(|known| known.matches(schema)) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let event = if INITIATED.matches(schema) {
            RefundEvent::Initiated(RefundInitiated {
                order: fields.required(1, "order")?,
                tender: fields.required(2, "tender")?,
                amount: fields.required(3, "amount")?,
            })
        } else if COMPLETED.matches(schema) {
            RefundEvent::Completed(RefundCompleted {
                amount: fields.required(1, "amount")?,
                rounding: fields.optional(2, "rounding")?,
            })
        } else {
            return Err(DecodeError::UnknownSchema);
        };
        fields.finish()?;
        event.validate()?;
        Ok(event)
    }
}
