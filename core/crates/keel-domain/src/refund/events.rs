//! Refund events and their payloads (ADR-0032, ADR-0035).
//!
//! Every schema below is version 1 but `refund.initiated`, which is version 2. Keys and encodings
//! follow [`crate::codec`]; "optional" fields are omitted when absent.
//!
//! | Schema | Keys |
//! |---|---|
//! | `refund.initiated` | 1 order, 2 tender, 3 amount, 4 payment (version 2) |
//! | `refund.completed` | 1 amount, 2 rounding (optional) |
//! | `refund.failed` | 1 reason code, 2 note (optional), 3 processor reference (optional) |
//!
//! Version 1 of `refund.initiated` is version 2 without the payment. A card refund names the card
//! payment it goes back to, as version 2; a cash refund names none, and is written as version 1,
//! as before.
//!
//! Beyond the types, payloads must satisfy these rules:
//! - what a refund is initiated for, and what it refunded, are more than zero;
//! - a completion's rounding isn't zero, and is in its amount's currency;
//! - a refund that names a payment is by card.

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_types::{Id, Money};

use crate::codec::{Fields, PayloadError, ProcessorRef, Record};
use crate::order::{Order, Reason};
use crate::payment::{Payment, Tender};
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
    /// For a card refund, the card payment it goes back to (version 2).
    pub payment: Option<Id<Payment>>,
}

/// An attempt to pay a refund back failed: nothing moved, and the refund is still owed
/// (ADR-0035). It can be attempted again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundFailed {
    /// Why, such as `declined`.
    pub reason: Reason,
    /// The processor's reference, when it gave one.
    pub reference: Option<ProcessorRef>,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefundEvent {
    /// The refund was started.
    Initiated(RefundInitiated),
    /// The money was paid back.
    Completed(RefundCompleted),
    /// An attempt to pay it back failed.
    Failed(RefundFailed),
}

const INITIATED_V1: SchemaId = SchemaId { name: "refund.initiated", version: 1 };
const INITIATED: SchemaId = SchemaId { name: "refund.initiated", version: 2 };
const COMPLETED: SchemaId = SchemaId { name: "refund.completed", version: 1 };
const FAILED: SchemaId = SchemaId { name: "refund.failed", version: 1 };

impl RefundEvent {
    /// Checks the payload's rules: see the module's.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        match self {
            RefundEvent::Initiated(initiated) if !initiated.amount.is_positive() => {
                Err(PayloadError::Invalid("amount"))
            }
            RefundEvent::Initiated(initiated)
                if initiated.payment.is_some() && initiated.tender != Tender::Card =>
            {
                Err(PayloadError::Invalid("payment"))
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

    const SCHEMAS: &'static [SchemaId] = &[INITIATED_V1, INITIATED, COMPLETED, FAILED];

    fn schema(&self) -> SchemaId {
        match self {
            // Written as the oldest version that holds it: without a payment, as version 1.
            RefundEvent::Initiated(initiated) if initiated.payment.is_none() => INITIATED_V1,
            RefundEvent::Initiated(_) => INITIATED,
            RefundEvent::Completed(_) => COMPLETED,
            RefundEvent::Failed(_) => FAILED,
        }
    }

    fn to_value(&self) -> Value {
        let record = Record::default();
        match self {
            RefundEvent::Initiated(initiated) => record
                .field(1, &initiated.order)
                .field(2, &initiated.tender)
                .field(3, &initiated.amount)
                .optional(4, initiated.payment.as_ref()),
            RefundEvent::Completed(completed) => {
                record.field(1, &completed.amount).optional(2, completed.rounding.as_ref())
            }
            RefundEvent::Failed(failed) => record
                .field(1, &failed.reason.code)
                .optional(2, failed.reason.note.as_ref())
                .optional(3, failed.reference.as_ref()),
        }
        .build()
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<RefundEvent, DecodeError> {
        if !Self::SCHEMAS.iter().any(|known| known.matches(schema)) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let event = if INITIATED_V1.matches(schema) || INITIATED.matches(schema) {
            RefundEvent::Initiated(RefundInitiated {
                order: fields.required(1, "order")?,
                tender: fields.required(2, "tender")?,
                amount: fields.required(3, "amount")?,
                // Version 2 has the payment, always: one without is written as version 1.
                payment: if INITIATED.matches(schema) {
                    Some(fields.required(4, "payment")?)
                } else {
                    None
                },
            })
        } else if COMPLETED.matches(schema) {
            RefundEvent::Completed(RefundCompleted {
                amount: fields.required(1, "amount")?,
                rounding: fields.optional(2, "rounding")?,
            })
        } else if FAILED.matches(schema) {
            RefundEvent::Failed(RefundFailed {
                reason: Reason {
                    code: fields.required(1, "reason")?,
                    note: fields.optional(2, "note")?,
                },
                reference: fields.optional(3, "reference")?,
            })
        } else {
            return Err(DecodeError::UnknownSchema);
        };
        fields.finish()?;
        event.validate()?;
        Ok(event)
    }
}
