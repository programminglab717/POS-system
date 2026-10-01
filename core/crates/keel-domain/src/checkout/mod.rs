//! Checkout: the rules that span an order and its payments (ADR-0015).
//!
//! An order doesn't know its payments, and a payment knows only its order and check, so neither
//! aggregate alone can say what a check still owes. [`Checkout`] looks at an order together with
//! its payments and the location's pricing rules:
//!
//! - **Balances.** A check's balance is its total, the snapshot's once it is closed and the
//!   current pricing's before, less its captured payments. Tips are on top, and owed to no check.
//! - **Starting a payment** needs an active order, an open check with no *unresolved* payment
//!   (one initiated or authorized, whose money may still move), and an amount of no more than
//!   the balance. An unknown outcome is resolved before a new attempt, so no check is charged
//!   twice.
//! - **Closing a check** needs no unresolved payment on it, and captured payments that cover
//!   its total. The check's snapshot records what pricing charged it, and those payments.
//! - **Ownership.** Only the order's owning device may start a payment or close a check
//!   (ADR-0021). A payment's outcome is recorded by the device that started it, whoever owns
//!   the order by then: money that moved is a fact.
//! - **Issues.** Money that doesn't fit the order is reported, never hidden: the payments stand,
//!   and a manager decides.
//!
//! A payment counts toward its check once captured, and only in the order's currency.

#[cfg(test)]
mod tests;

use core::cmp::Ordering;

use keel_events::envelope::{Device, Location};
use keel_pricing::{PricingError, Rules, Totals};
use keel_types::{Id, Money, MoneyError};

use crate::codec::{IdSet, PayloadError, RulesVersion};
use crate::order::{
    Check, CheckClosed, CommandError, LineCharge, Order, OrderEvent, OrderStatus, TaxCharge,
};
use crate::payment::{Payment, PaymentCaptured, PaymentEvent, PaymentInitiated, Tender};
use crate::schema::{SchemaError, Unrecordable, check_recordable};

/// An order, its payments and the location's pricing rules, seen together.
#[derive(Clone, Debug)]
pub struct Checkout<'a> {
    order: &'a Order,
    /// The order's initiated payments, in ascending order of identifier, each once.
    payments: Vec<&'a Payment>,
    rules: &'a Rules,
    rules_version: RulesVersion,
}

/// What a check owes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Balance {
    /// The check.
    pub check: Id<Check>,
    /// What the check costs: the total it was closed with, or else its current pricing.
    pub total: Money,
    /// The amounts its captured payments applied to it.
    pub captured: Money,
    /// The tips its captured payments added, on top.
    pub tips: Money,
    /// What it still owes: the total less the captured amounts. Negative when overpaid.
    pub due: Money,
    /// Its payments that are initiated or authorized, with no final outcome, in ascending order.
    pub unresolved: Vec<Id<Payment>>,
}

/// Money that doesn't fit the order: the payments stand, and a manager decides.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Issue {
    /// A payment names a check the order doesn't have.
    UnknownCheck(Id<Payment>),
    /// A payment is in another currency than the order's; it doesn't count toward its check.
    WrongCurrency(Id<Payment>),
    /// A payment was captured, or is unresolved, on an order that was voided or abandoned.
    OnVoidedOrder(Id<Payment>),
    /// A payment was captured, or is unresolved, on a check that was closed without it.
    AfterClose(Id<Payment>),
    /// A check's captured payments exceed its total.
    Overpaid {
        /// The check.
        check: Id<Check>,
        /// By how much.
        by: Money,
    },
    /// A closed check's captured payments no longer cover the total it was closed with: one of
    /// them failed or was voided concurrently, or hasn't reached this device yet.
    Underpaid {
        /// The check.
        check: Id<Check>,
        /// By how much.
        by: Money,
    },
}

/// Why checkout refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckoutError {
    /// The order refused: it isn't created or active at the device's location, or the check is
    /// unknown, closed, or holds no live line.
    #[error(transparent)]
    Order(#[from] CommandError),
    /// A payment on the check has no outcome yet: resolve it first.
    #[error("payment {0} is unresolved")]
    Unresolved(Id<Payment>),
    /// A payment with that identifier already exists.
    #[error("payment {0} already exists")]
    PaymentExists(Id<Payment>),
    /// The amount isn't more than zero.
    #[error("the amount isn't more than zero")]
    InvalidAmount,
    /// The amount is in another currency than the order's.
    #[error("the amount is in another currency than the order's")]
    WrongCurrency,
    /// The amount is more than the check owes.
    #[error("the check owes {due}")]
    TooMuch {
        /// What the check owes.
        due: Money,
    },
    /// The check's captured payments don't cover its total.
    #[error("the check still owes {due}")]
    NotCovered {
        /// What the check still owes.
        due: Money,
    },
    /// The check couldn't be priced.
    #[error(transparent)]
    Pricing(#[from] PricingError),
    /// An amount is out of range.
    #[error("an amount is out of range")]
    Overflow,
    /// The event wouldn't satisfy its schema.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl From<MoneyError> for CheckoutError {
    fn from(_: MoneyError) -> CheckoutError {
        CheckoutError::Overflow
    }
}

impl From<Unrecordable> for CheckoutError {
    fn from(error: Unrecordable) -> CheckoutError {
        match error {
            Unrecordable::Schema(error) => CheckoutError::Schema(error),
            Unrecordable::Payload(error) => CheckoutError::Invalid(error),
        }
    }
}

impl<'a> Checkout<'a> {
    /// `order` with its `payments`, priced by `rules`, whose version is `rules_version`.
    /// Payments of other orders, and payments not yet initiated, are left out; a payment given
    /// twice counts once.
    pub fn new(
        order: &'a Order,
        payments: impl IntoIterator<Item = &'a Payment>,
        rules: &'a Rules,
        rules_version: RulesVersion,
    ) -> Checkout<'a> {
        let mut payments: Vec<&Payment> = payments
            .into_iter()
            .filter(|payment| payment.info().is_some_and(|info| info.order == order.id()))
            .collect();
        payments.sort_by_key(|payment| payment.id().to_bytes());
        payments.dedup_by_key(|payment| payment.id());
        Checkout { order, payments, rules, rules_version }
    }

    /// The order's payments on `check`.
    fn payments_on(&self, check: Id<Check>) -> impl Iterator<Item = &'a Payment> + '_ {
        self.payments
            .iter()
            .copied()
            .filter(move |payment| payment.info().is_some_and(|info| info.check == check))
    }

    /// The captures that count toward `check`: its captured payments in the order's currency.
    fn counted(
        &self,
        check: Id<Check>,
    ) -> Result<Vec<(Id<Payment>, &'a PaymentCaptured)>, CheckoutError> {
        let currency = self.currency()?;
        Ok(self
            .payments_on(check)
            .filter(|payment| payment.info().is_some_and(|info| info.amount.currency() == currency))
            .filter_map(|payment| Some((payment.id(), payment.captured()?)))
            .collect())
    }

    fn currency(&self) -> Result<keel_types::Currency, CheckoutError> {
        let info = self.order.info().ok_or(CommandError::NotCreated)?;
        Ok(info.currency)
    }

    /// Prices `check` as its own sale.
    fn price(&self, check: Id<Check>) -> Result<Totals, CheckoutError> {
        let basket = self.order.check_basket(check).ok_or(CommandError::UnknownCheck(check))?;
        Ok(keel_pricing::price(&basket, self.rules)?)
    }

    /// What `check` owes.
    ///
    /// # Errors
    /// [`CheckoutError::Order`] if the order isn't created or has no such check,
    /// [`CheckoutError::Pricing`] if an open check can't be priced, and
    /// [`CheckoutError::Overflow`] if an amount is out of range.
    pub fn balance(&self, check: Id<Check>) -> Result<Balance, CheckoutError> {
        let currency = self.currency()?;
        let found = self.order.check(check).ok_or(CommandError::UnknownCheck(check))?;
        let total = match found.closed() {
            Some(closed) => closed.total,
            None => self.price(check)?.total,
        };
        let counted = self.counted(check)?;
        let captured = Money::sum(currency, counted.iter().map(|(_, captured)| captured.amount))?;
        let tips = Money::sum(currency, counted.iter().filter_map(|(_, captured)| captured.tip))?;
        let unresolved = self
            .payments_on(check)
            .filter(|payment| payment.is_unresolved())
            .map(Payment::id)
            .collect();
        Ok(Balance { check, total, captured, tips, due: total.checked_sub(captured)?, unresolved })
    }

    /// Starts a payment of `amount` on `check`, by `tender`, from `device` at `location`: the
    /// event that initiates payment `id`.
    ///
    /// # Errors
    /// [`CheckoutError`] saying why the payment can't start: the order isn't active at
    /// `location`, another device owns it, the check is unknown or closed, a payment on it is
    /// unresolved, `id` is already a payment's, or the amount isn't more than zero, is in
    /// another currency, or is more than the check owes.
    pub fn start_payment(
        &self,
        location: Id<Location>,
        device: Id<Device>,
        id: Id<Payment>,
        check: Id<Check>,
        tender: Tender,
        amount: Money,
    ) -> Result<PaymentEvent, CheckoutError> {
        let info = self.order.check_active(location)?;
        self.order.check_owner(device)?;
        let found = self.order.check(check).ok_or(CommandError::UnknownCheck(check))?;
        if !found.is_open() {
            return Err(CommandError::CheckClosed(check).into());
        }
        if self.payments.iter().any(|payment| payment.id() == id) {
            return Err(CheckoutError::PaymentExists(id));
        }
        let balance = self.balance(check)?;
        if let Some(&unresolved) = balance.unresolved.first() {
            return Err(CheckoutError::Unresolved(unresolved));
        }
        if amount.currency() != info.currency {
            return Err(CheckoutError::WrongCurrency);
        }
        if !amount.is_positive() {
            return Err(CheckoutError::InvalidAmount);
        }
        if amount.compare(balance.due)? == Ordering::Greater {
            return Err(CheckoutError::TooMuch { due: balance.due });
        }
        let event = PaymentEvent::Initiated(PaymentInitiated {
            order: self.order.id(),
            check,
            tender,
            amount,
        });
        check_recordable(&event)?;
        Ok(event)
    }

    /// Closes `check`, from `device` at `location`: the event that records what the check was
    /// charged, as pricing charges it now, and the payments that settled it.
    ///
    /// # Errors
    /// [`CheckoutError`] saying why the check can't close: the order isn't active at `location`,
    /// another device owns it, the check is unknown, closed or holds no live line, a payment on
    /// it is unresolved, its captured payments don't cover it, or it can't be priced.
    pub fn close_check(
        &self,
        location: Id<Location>,
        device: Id<Device>,
        check: Id<Check>,
    ) -> Result<OrderEvent, CheckoutError> {
        let currency = self.order.check_closable(location, check)?.currency;
        self.order.check_owner(device)?;
        if let Some(unresolved) = self.payments_on(check).find(|payment| payment.is_unresolved()) {
            return Err(CheckoutError::Unresolved(unresolved.id()));
        }
        let totals = self.price(check)?;
        let counted = self.counted(check)?;
        let captured = Money::sum(currency, counted.iter().map(|(_, captured)| captured.amount))?;
        let due = totals.total.checked_sub(captured)?;
        if due.is_positive() {
            return Err(CheckoutError::NotCovered { due });
        }
        let mut lines: Vec<LineCharge> = self
            .order
            .lines_on(check)
            .zip(&totals.lines)
            .map(|(line, priced)| LineCharge {
                line: line.id(),
                gross: priced.gross,
                net: priced.net,
                tax: priced.tax,
            })
            .collect();
        lines.sort_by_key(|charge| charge.line.to_bytes());
        let mut taxes: Vec<TaxCharge> = totals
            .taxes
            .iter()
            .filter(|tax| tax.taxable.is_positive())
            .map(|tax| TaxCharge { tax: tax.id, taxable: tax.taxable, amount: tax.tax })
            .collect();
        taxes.sort_by_key(|charge| charge.tax.to_bytes());
        let payments = IdSet::new(counted.into_iter().map(|(id, _)| id)).ok();
        let event = OrderEvent::CheckClosed(CheckClosed {
            check,
            rules_version: self.rules_version,
            lines,
            taxes,
            total: totals.total,
            payments,
        });
        check_recordable(&event)?;
        Ok(event)
    }

    /// Money that doesn't fit the order: first each payment's issues, in ascending order of
    /// payment, then each check's, in the order of the checks.
    ///
    /// # Errors
    /// As [`Checkout::balance`], for any check.
    pub fn issues(&self) -> Result<Vec<Issue>, CheckoutError> {
        let currency = self.currency()?;
        let ended = matches!(self.order.status(), OrderStatus::Voided(_) | OrderStatus::Abandoned);
        let mut issues = Vec::new();
        for payment in &self.payments {
            let Some(info) = payment.info() else { continue };
            let id = payment.id();
            // Money moved, or may still move.
            let live = payment.captured().is_some() || payment.is_unresolved();
            let check = self.order.check(info.check);
            if check.is_none() {
                issues.push(Issue::UnknownCheck(id));
            }
            if info.amount.currency() != currency {
                issues.push(Issue::WrongCurrency(id));
            }
            if ended && live {
                issues.push(Issue::OnVoidedOrder(id));
            }
            let settled = |closed: &CheckClosed| {
                closed.payments.as_ref().is_some_and(|payments| payments.iter().any(|p| p == id))
            };
            if live && check.and_then(Check::closed).is_some_and(|closed| !settled(closed)) {
                issues.push(Issue::AfterClose(id));
            }
        }
        for check in self.order.checks() {
            let due = self.balance(check.id())?.due;
            if due.is_negative() {
                issues.push(Issue::Overpaid { check: check.id(), by: due.checked_neg()? });
            } else if due.is_positive() && !check.is_open() {
                issues.push(Issue::Underpaid { check: check.id(), by: due });
            }
        }
        Ok(issues)
    }
}
