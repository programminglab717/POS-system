//! Closing a check: the snapshot of what it was charged (ADR-0015).
//!
//! A check closes once its payments cover its total. Its snapshot records what it was charged,
//! as pricing computed it then, and the payments that settled it. Receipts, the ledger and
//! returns read the snapshot, never a recomputation: later changes to the rules, or to the
//! order, never change what a closed check was charged.

use core::cmp::Ordering;

use keel_events::cbor::Value;
use keel_pricing::Tax;
use keel_types::{Id, Money};

use super::checks::Check;
use super::state::Line;
use crate::codec::{Field, Fields, IdSet, PayloadError, Record, RulesVersion};
use crate::payment::Payment;

/// A check was closed: what it was charged, and the payments that settled it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckClosed {
    /// The check.
    pub check: Id<Check>,
    /// The version of the pricing rules the check was priced with.
    pub rules_version: RulesVersion,
    /// What each line on the check was charged, for the check's part of it: in ascending order
    /// of line, and never empty.
    pub lines: Vec<LineCharge>,
    /// Each tax with something to tax on the check, in ascending order of tax.
    pub taxes: Vec<TaxCharge>,
    /// What the check was charged in all: its lines' net amounts and their tax.
    pub total: Money,
    /// The payments that settled it; none when its total is zero and nothing was paid.
    pub payments: Option<IdSet<Payment>>,
}

/// What a check was charged for a line, or for its part of a line it shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCharge {
    /// The line.
    pub line: Id<Line>,
    /// The check's part of the line's price before comps and discounts.
    pub gross: Money,
    /// What is left after comps and discounts: what the line was sold for, before tax.
    pub net: Money,
    /// The line's tax.
    pub tax: Money,
}

/// What a check was charged for one tax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaxCharge {
    /// The tax, as the pricing rules identify it.
    pub tax: Id<Tax>,
    /// The amount it taxed: more than zero.
    pub taxable: Money,
    /// The tax.
    pub amount: Money,
}

impl CheckClosed {
    /// Checks the snapshot's rules, which span its fields:
    /// - every amount is in the total's currency;
    /// - the lines are in strictly ascending order, and not empty; a line's net is between zero
    ///   and its gross, and its tax isn't negative;
    /// - the taxes are in strictly ascending order; each taxed more than zero, and its tax isn't
    ///   negative;
    /// - the taxes add up to the lines' tax, and the total is the lines' net plus their tax;
    /// - a check with something to pay was settled by payments.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        let currency = self.total.currency();
        let in_currency = |amount: Money| amount.currency() == currency;
        let at_most = |amount: Money, limit: Money| {
            matches!(amount.compare(limit), Ok(Ordering::Less | Ordering::Equal))
        };
        let lines_valid = !self.lines.is_empty()
            && ascending(self.lines.iter().map(|charge| charge.line.to_bytes()))
            && self.lines.iter().all(|charge| {
                [charge.gross, charge.net, charge.tax].into_iter().all(in_currency)
                    && !charge.net.is_negative()
                    && at_most(charge.net, charge.gross)
                    && !charge.tax.is_negative()
            });
        if !lines_valid {
            return Err(PayloadError::Invalid("lines"));
        }
        let taxes_valid = ascending(self.taxes.iter().map(|charge| charge.tax.to_bytes()))
            && self.taxes.iter().all(|charge| {
                in_currency(charge.taxable)
                    && in_currency(charge.amount)
                    && charge.taxable.is_positive()
                    && !charge.amount.is_negative()
            });
        let net = Money::sum(currency, self.lines.iter().map(|charge| charge.net)).ok();
        let tax = Money::sum(currency, self.lines.iter().map(|charge| charge.tax)).ok();
        let taxes = Money::sum(currency, self.taxes.iter().map(|charge| charge.amount)).ok();
        if !taxes_valid || tax.is_none() || tax != taxes {
            return Err(PayloadError::Invalid("taxes"));
        }
        let total = net.zip(tax).and_then(|(net, tax)| net.checked_add(tax).ok());
        if total != Some(self.total) {
            return Err(PayloadError::Invalid("total"));
        }
        if self.total.is_positive() && self.payments.is_none() {
            return Err(PayloadError::Missing("payments"));
        }
        Ok(())
    }

    /// The charge for `line`, if the check was charged for it.
    pub fn line(&self, line: Id<Line>) -> Option<&LineCharge> {
        self.lines.iter().find(|charge| charge.line == line)
    }
}

/// Whether identifiers come in strictly ascending byte order.
fn ascending(mut ids: impl Iterator<Item = [u8; 16]>) -> bool {
    let Some(mut previous) = ids.next() else { return true };
    ids.all(|id| {
        let rising = previous < id;
        previous = id;
        rising
    })
}

/// `CheckClosed` keys.
mod key {
    pub(super) const CHECK: u64 = 1;
    pub(super) const RULES_VERSION: u64 = 2;
    pub(super) const LINES: u64 = 3;
    pub(super) const TAXES: u64 = 4;
    pub(super) const TOTAL: u64 = 5;
    pub(super) const PAYMENTS: u64 = 6;
}

impl CheckClosed {
    /// The snapshot's fields, added to a payload.
    pub(crate) fn record(&self, record: Record) -> Record {
        record
            .field(key::CHECK, &self.check)
            .field(key::RULES_VERSION, &self.rules_version)
            .field(key::LINES, &self.lines)
            .field(key::TAXES, &self.taxes)
            .field(key::TOTAL, &self.total)
            .optional(key::PAYMENTS, self.payments.as_ref())
    }

    /// The snapshot a payload's fields hold, if they hold a valid one.
    pub(crate) fn read(fields: &mut Fields<'_>) -> Result<CheckClosed, PayloadError> {
        let closed = CheckClosed {
            check: fields.required(key::CHECK, "check")?,
            rules_version: fields.required(key::RULES_VERSION, "rules version")?,
            lines: fields.required(key::LINES, "lines")?,
            taxes: fields.required(key::TAXES, "taxes")?,
            total: fields.required(key::TOTAL, "total")?,
            payments: fields.optional(key::PAYMENTS, "payments")?,
        };
        closed.validate()?;
        Ok(closed)
    }
}

impl Field for LineCharge {
    fn to_value(&self) -> Value {
        Record::default()
            .field(1, &self.line)
            .field(2, &self.gross)
            .field(3, &self.net)
            .field(4, &self.tax)
            .build()
    }

    fn from_value(value: &Value) -> Option<LineCharge> {
        let mut fields = Fields::read(value).ok()?;
        let charge = LineCharge {
            line: fields.required(1, "line").ok()?,
            gross: fields.required(2, "gross").ok()?,
            net: fields.required(3, "net").ok()?,
            tax: fields.required(4, "tax").ok()?,
        };
        fields.finish().ok()?;
        Some(charge)
    }
}

impl Field for TaxCharge {
    fn to_value(&self) -> Value {
        Record::default().field(1, &self.tax).field(2, &self.taxable).field(3, &self.amount).build()
    }

    fn from_value(value: &Value) -> Option<TaxCharge> {
        let mut fields = Fields::read(value).ok()?;
        let charge = TaxCharge {
            tax: fields.required(1, "tax").ok()?,
            taxable: fields.required(2, "taxable").ok()?,
            amount: fields.required(3, "amount").ok()?,
        };
        fields.finish().ok()?;
        Some(charge)
    }
}
