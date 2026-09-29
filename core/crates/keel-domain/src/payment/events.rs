//! Payment events, version 1, and their payloads.
//!
//! Every schema below is version 1. Keys and encodings follow [`crate::codec`]; "optional" fields
//! are omitted when absent.
//!
//! | Schema | Keys |
//! |---|---|
//! | `payment.initiated` | 1 order, 2 check, 3 tender, 4 amount |
//! | `payment.authorized` | 1 amount, 2 processor reference (optional) |
//! | `payment.captured` | 1 amount, 2 tip (optional), 3 processor reference (optional), 4 amount tendered (optional), 5 cash rounding (optional) |
//! | `payment.failed` | 1 reason code, 2 note (optional), 3 processor reference (optional) |
//! | `payment.voided` | 1 reason code, 2 note (optional), 3 processor reference (optional) |
//!
//! Beyond the types, payloads must satisfy these rules:
//! - amounts are more than zero, and so is a tip; a cash rounding isn't zero;
//! - all the amounts in one payload are in the same currency;
//! - a cash rounding comes with the amount tendered; the amount, the tip and the rounding add up
//!   to zero or more, and the amount tendered covers them.

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_types::{Id, Money};

use crate::codec::{Fields, PayloadError, ProcessorRef, Record, code_enum};
use crate::order::{Check, Order, Reason};
use crate::schema::{DecodeError, DomainEvent, SchemaId};

code_enum! {
    /// How a payment is paid. Version 1 has cash and card; other tenders need a new version of
    /// `payment.initiated`.
    pub enum Tender {
        /// Cash, counted into the drawer.
        Cash = 0,
        /// A card, through a terminal or a processor.
        Card = 1,
    }
}

/// A payment was started: the terminal or the drawer is about to be asked for money.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentInitiated {
    /// The order it pays for.
    pub order: Id<Order>,
    /// The check it pays.
    pub check: Id<Check>,
    /// How it is paid.
    pub tender: Tender,
    /// How much it asks for: more than zero, in the order's currency.
    pub amount: Money,
}

/// A card payment was authorized: the card is held for an amount, not yet charged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentAuthorized {
    /// The amount held: more than zero, and no more than was asked for.
    pub amount: Money,
    /// The processor's reference for the authorization.
    pub reference: Option<ProcessorRef>,
}

/// A payment was captured: the money moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentCaptured {
    /// The amount applied to the check: more than zero.
    pub amount: Money,
    /// A tip on top of the amount, which the check doesn't owe.
    pub tip: Option<Money>,
    /// The processor's reference for the capture.
    pub reference: Option<ProcessorRef>,
    /// For cash: what the customer handed over, and any cash rounding.
    pub cash: Option<CashTendered>,
}

/// What a customer paid in cash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CashTendered {
    /// The cash handed over: at least what the customer pays.
    pub tendered: Money,
    /// The cash rounding, where the smallest coins aren't used: what the customer pays less the
    /// amount and the tip. Negative when rounded down, and never zero.
    pub rounding: Option<Money>,
}

/// A payment ended without money moving: it failed, or it was voided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentEnded {
    /// Why, such as `declined` or `cancelled`.
    pub reason: Reason,
    /// The processor's reference, when it gave one.
    pub reference: Option<ProcessorRef>,
}

impl PaymentCaptured {
    /// What the customer paid: the amount and the tip, with any cash rounding. `None` if the
    /// amounts don't add up in one currency, which a decoded payload never has.
    pub fn received(&self) -> Option<Money> {
        let mut received = self.amount;
        let rounding = self.cash.and_then(|cash| cash.rounding);
        for part in [self.tip, rounding].into_iter().flatten() {
            received = received.checked_add(part).ok()?;
        }
        Some(received)
    }

    /// The change given in cash: the amount tendered less what the customer paid. `None` for a
    /// payment that isn't in cash.
    pub fn change(&self) -> Option<Money> {
        self.cash?.tendered.checked_sub(self.received()?).ok()
    }

    fn validate(&self) -> Result<(), PayloadError> {
        let currency = self.amount.currency();
        if !self.amount.is_positive() {
            return Err(PayloadError::Invalid("amount"));
        }
        if self.tip.is_some_and(|tip| tip.currency() != currency || !tip.is_positive()) {
            return Err(PayloadError::Invalid("tip"));
        }
        let Some(cash) = self.cash else { return Ok(()) };
        if cash
            .rounding
            .is_some_and(|rounding| rounding.currency() != currency || rounding.is_zero())
        {
            return Err(PayloadError::Invalid("rounding"));
        }
        let covered = self.received().zip(self.change()).is_some_and(|(received, change)| {
            cash.tendered.currency() == currency && !received.is_negative() && !change.is_negative()
        });
        if !covered {
            return Err(PayloadError::Invalid("tendered"));
        }
        Ok(())
    }
}

/// Something that happened to a payment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaymentEvent {
    /// The payment was started.
    Initiated(PaymentInitiated),
    /// A card was authorized.
    Authorized(PaymentAuthorized),
    /// The money moved.
    Captured(PaymentCaptured),
    /// The attempt failed, such as a decline or a cancellation: no money moved.
    Failed(PaymentEnded),
    /// The payment was voided before it was captured: an authorization released, or an attempt
    /// called off. No money moved.
    Voided(PaymentEnded),
}

/// The schemas, in the order of `PaymentEvent`'s variants.
const INITIATED: SchemaId = SchemaId { name: "payment.initiated", version: 1 };
const AUTHORIZED: SchemaId = SchemaId { name: "payment.authorized", version: 1 };
const CAPTURED: SchemaId = SchemaId { name: "payment.captured", version: 1 };
const FAILED: SchemaId = SchemaId { name: "payment.failed", version: 1 };
const VOIDED: SchemaId = SchemaId { name: "payment.voided", version: 1 };

impl DomainEvent for PaymentEvent {
    const STREAM: &'static str = "payment";

    const SCHEMAS: &'static [SchemaId] = &[INITIATED, AUTHORIZED, CAPTURED, FAILED, VOIDED];

    fn schema(&self) -> SchemaId {
        match self {
            PaymentEvent::Initiated(_) => INITIATED,
            PaymentEvent::Authorized(_) => AUTHORIZED,
            PaymentEvent::Captured(_) => CAPTURED,
            PaymentEvent::Failed(_) => FAILED,
            PaymentEvent::Voided(_) => VOIDED,
        }
    }

    fn to_value(&self) -> Value {
        let record = Record::default();
        match self {
            PaymentEvent::Initiated(initiated) => record
                .field(1, &initiated.order)
                .field(2, &initiated.check)
                .field(3, &initiated.tender)
                .field(4, &initiated.amount),
            PaymentEvent::Authorized(authorized) => {
                record.field(1, &authorized.amount).optional(2, authorized.reference.as_ref())
            }
            PaymentEvent::Captured(captured) => record
                .field(1, &captured.amount)
                .optional(2, captured.tip.as_ref())
                .optional(3, captured.reference.as_ref())
                .optional(4, captured.cash.as_ref().map(|cash| &cash.tendered))
                .optional(5, captured.cash.as_ref().and_then(|cash| cash.rounding.as_ref())),
            PaymentEvent::Failed(ended) | PaymentEvent::Voided(ended) => record
                .field(1, &ended.reason.code)
                .optional(2, ended.reason.note.as_ref())
                .optional(3, ended.reference.as_ref()),
        }
        .build()
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<PaymentEvent, DecodeError> {
        if !Self::SCHEMAS.iter().any(|known| known.matches(schema)) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let event = if INITIATED.matches(schema) {
            let initiated = PaymentInitiated {
                order: fields.required(1, "order")?,
                check: fields.required(2, "check")?,
                tender: fields.required(3, "tender")?,
                amount: fields.required(4, "amount")?,
            };
            if !initiated.amount.is_positive() {
                return Err(PayloadError::Invalid("amount").into());
            }
            PaymentEvent::Initiated(initiated)
        } else if AUTHORIZED.matches(schema) {
            let authorized = PaymentAuthorized {
                amount: fields.required(1, "amount")?,
                reference: fields.optional(2, "reference")?,
            };
            if !authorized.amount.is_positive() {
                return Err(PayloadError::Invalid("amount").into());
            }
            PaymentEvent::Authorized(authorized)
        } else if CAPTURED.matches(schema) {
            PaymentEvent::Captured(read_captured(&mut fields)?)
        } else if FAILED.matches(schema) || VOIDED.matches(schema) {
            let ended = PaymentEnded {
                reason: Reason {
                    code: fields.required(1, "reason")?,
                    note: fields.optional(2, "note")?,
                },
                reference: fields.optional(3, "reference")?,
            };
            if FAILED.matches(schema) {
                PaymentEvent::Failed(ended)
            } else {
                PaymentEvent::Voided(ended)
            }
        } else {
            return Err(DecodeError::UnknownSchema);
        };
        fields.finish()?;
        Ok(event)
    }
}

fn read_captured(fields: &mut Fields<'_>) -> Result<PaymentCaptured, PayloadError> {
    let amount = fields.required(1, "amount")?;
    let tip = fields.optional(2, "tip")?;
    let reference = fields.optional(3, "reference")?;
    let tendered = fields.optional(4, "tendered")?;
    let rounding = fields.optional(5, "rounding")?;
    let cash = match (tendered, rounding) {
        (Some(tendered), rounding) => Some(CashTendered { tendered, rounding }),
        (None, None) => None,
        (None, Some(_)) => return Err(PayloadError::Invalid("rounding")),
    };
    let captured = PaymentCaptured { amount, tip, reference, cash };
    captured.validate()?;
    Ok(captured)
}
