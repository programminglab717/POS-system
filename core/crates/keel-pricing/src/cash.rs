//! Cash rounding: the amount due when paying in cash, where the smallest coins aren't used.

use keel_types::{Money, RoundingRule};

use crate::engine::PricingError;

/// What a customer pays in cash, and the rounding that gets there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CashDue {
    /// The amount to pay in cash.
    pub due: Money,
    /// The rounding: `due` less the amount before rounding. Negative when rounded down.
    pub rounding: Money,
}

/// Rounds an amount paid in cash with the jurisdiction's `rule`, such as Canada's nearest 5 cents.
///
/// Cash rounding applies only to the amount paid in cash, when it is paid: never to an order's
/// totals, and never to other tenders. The rounding is recorded as its own amount, so it can be
/// reported and posted separately.
///
/// # Errors
/// [`PricingError::Overflow`] if the rounded amount is out of range.
pub fn round_cash(amount: Money, rule: RoundingRule) -> Result<CashDue, PricingError> {
    let due = amount.round_to(rule)?;
    let rounding = due.checked_sub(amount)?;
    Ok(CashDue { due, rounding })
}
