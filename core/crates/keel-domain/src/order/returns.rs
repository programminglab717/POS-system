//! Returns (ADR-0032): lines a customer brought back, recorded on the order they were sold on,
//! each refunded its share of what the order's checks charged it.
//!
//! A line's share for a quantity is taken of the quantity returned so far, so partial returns
//! add up: the last return of a line refunds exactly what is left of its net and tax, whatever
//! was returned before.

use keel_events::cbor::Value;
use keel_types::{Id, Money, Quantity};

use super::state::Line;
use super::types::Reason;
use crate::codec::{Field, Fields, PayloadError, Record};
use crate::refund::Refund;

/// Lines were returned, refunded by `refund`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinesReturned {
    /// The refund that pays them back.
    pub refund: Id<Refund>,
    /// Each line returned, in strictly ascending order of line, never empty.
    pub lines: Vec<LineReturned>,
    /// Why.
    pub reason: Reason,
}

/// A line returned: how much of it, and what that refunds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineReturned {
    /// The line.
    pub line: Id<Line>,
    /// How much of it was returned: more than zero, in the line's unit.
    pub quantity: Quantity,
    /// What it refunds before tax: zero or more.
    pub net: Money,
    /// The tax it refunds: zero or more.
    pub tax: Money,
}

/// What a device asks to return: how much of each line, for `reason`, refunded by `refund`. The
/// order works out what each refunds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Return {
    /// The refund that pays them back.
    pub refund: Id<Refund>,
    /// Each line and how much of it, each line once.
    pub lines: Vec<(Id<Line>, Quantity)>,
    /// Why.
    pub reason: Reason,
}

/// What a line's returns came to: how much, and what they refunded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Returned {
    /// How much of the line was returned.
    pub quantity: Quantity,
    /// What its returns refunded before tax.
    pub net: Money,
    /// The tax its returns refunded.
    pub tax: Money,
}

impl LinesReturned {
    /// Checks the payload's rules: lines in strictly ascending order and not empty, each
    /// returning more than zero and refunding zero or more, every amount in one currency.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        let Some(first) = self.lines.first() else {
            return Err(PayloadError::Invalid("lines"));
        };
        let currency = first.net.currency();
        let ascending = self
            .lines
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a.line.to_bytes() < b.line.to_bytes()));
        let valid = ascending
            && self.lines.iter().all(|returned| {
                returned.quantity.is_positive()
                    && !returned.net.is_negative()
                    && !returned.tax.is_negative()
                    && returned.net.currency() == currency
                    && returned.tax.currency() == currency
            });
        if !valid {
            return Err(PayloadError::Invalid("lines"));
        }
        Ok(())
    }

    /// What the return refunds: every line's net and tax. `None` if it overflows, which a valid
    /// payload of real amounts never does.
    pub fn total(&self) -> Option<Money> {
        let currency = self.lines.first()?.net.currency();
        let parts = self.lines.iter().flat_map(|returned| [returned.net, returned.tax]);
        Money::sum(currency, parts).ok()
    }

    /// The payload's fields, added to a payload.
    pub(crate) fn record(&self, record: Record) -> Record {
        record
            .field(1, &self.refund)
            .field(2, &self.lines)
            .field(3, &self.reason.code)
            .optional(4, self.reason.note.as_ref())
    }

    /// The return a payload's fields hold, if they hold a valid one.
    pub(crate) fn read(fields: &mut Fields<'_>) -> Result<LinesReturned, PayloadError> {
        let returned = LinesReturned {
            refund: fields.required(1, "refund")?,
            lines: fields.required(2, "lines")?,
            reason: Reason {
                code: fields.required(3, "reason")?,
                note: fields.optional(4, "note")?,
            },
        };
        returned.validate()?;
        Ok(returned)
    }
}

impl Field for LineReturned {
    fn to_value(&self) -> Value {
        Record::default()
            .field(1, &self.line)
            .field(2, &self.quantity)
            .field(3, &self.net)
            .field(4, &self.tax)
            .build()
    }

    fn from_value(value: &Value) -> Option<LineReturned> {
        let mut fields = Fields::read(value).ok()?;
        let returned = LineReturned {
            line: fields.required(1, "line").ok()?,
            quantity: fields.required(2, "quantity").ok()?,
            net: fields.required(3, "net").ok()?,
            tax: fields.required(4, "tax").ok()?,
        };
        fields.finish().ok()?;
        Some(returned)
    }
}

/// `amount`'s share for `part` of `whole`: `amount × part / whole`, rounded half up. `None`
/// unless `amount` is zero or more, `whole` is more than zero, and `part` is from zero to
/// `whole` in its unit.
pub fn share(amount: Money, part: Quantity, whole: Quantity) -> Option<Money> {
    if amount.is_negative() || part.unit() != whole.unit() || part.is_negative() {
        return None;
    }
    let minor = u128::try_from(amount.minor()).ok()?;
    let (part, whole) = (u128::try_from(part.micros()).ok()?, u128::try_from(whole.micros()).ok()?);
    if whole == 0 || part > whole {
        return None;
    }
    // amount < 2^63 and part ≤ whole < 2^63: twice their product is below 2^127.
    let doubled = minor.checked_mul(part)?.checked_mul(2)?;
    let rounded = doubled.checked_add(whole)?.checked_div(whole.checked_mul(2)?)?;
    Some(Money::from_minor(i64::try_from(rounded).ok()?, amount.currency()))
}

/// What returning `quantity` more of a line refunds of `amount`, what its order charged it, once
/// `before` of its `whole` quantity was returned: its share of everything returned after, less
/// its share of what was returned before. `None` if that would take more than the line has.
pub fn refunded(
    amount: Money,
    before: Quantity,
    quantity: Quantity,
    whole: Quantity,
) -> Option<Money> {
    let after = before.checked_add(quantity).ok()?;
    share(amount, after, whole)?.checked_sub(share(amount, before, whole)?).ok()
}
