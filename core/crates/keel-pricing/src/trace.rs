//! The trace: every step of a calculation, with its inputs and result, so every amount can be
//! explained.
//!
//! Lines, discounts and taxes are identified by their position, counted from zero, in the basket
//! and the rules. The `Display` form, for logs and tests, counts from one, as people do.

use core::fmt;

use keel_types::{Money, Quantity, Rate, RoundingMode};

use crate::basket::Discount;
use crate::engine::Nth;

/// One step of a calculation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// A line's gross: the item's price plus its modifiers', times the quantity.
    Extended {
        /// The line.
        line: usize,
        /// The item's price for one unit.
        item: Money,
        /// The modifiers' prices for one unit of the line.
        modifiers: Money,
        /// The line's quantity.
        quantity: Quantity,
        /// How a fractional quantity's product is rounded.
        mode: RoundingMode,
        /// The result.
        gross: Money,
    },
    /// A comped line: the comp took all of it.
    Comped {
        /// The line.
        line: usize,
        /// What the comp took.
        amount: Money,
    },
    /// One of a line's discounts took from what was left of the line.
    LineDiscounted {
        /// The line.
        line: usize,
        /// The discount, by its position among the line's discounts.
        discount: usize,
        /// The discount as given.
        value: Discount,
        /// What was left of the line before the discount.
        base: Money,
        /// How a percentage is rounded.
        mode: RoundingMode,
        /// What the discount took.
        amount: Money,
        /// Whether an amount discount was cut down to what was left.
        limited: bool,
    },
    /// One of the order's discounts took from what was left of the order, and was allocated to
    /// the lines.
    OrderDiscounted {
        /// The discount, by its position among the order's discounts.
        discount: usize,
        /// The discount as given.
        value: Discount,
        /// What was left of the order before the discount.
        base: Money,
        /// How a percentage is rounded.
        mode: RoundingMode,
        /// What the discount took.
        amount: Money,
        /// Whether an amount discount was cut down to what was left.
        limited: bool,
        /// Each line's share, in the basket's order.
        shares: Vec<Money>,
    },
    /// A tax on one line, rounded for the line.
    TaxedLine {
        /// The tax, by its position in the rules.
        tax: usize,
        /// The line.
        line: usize,
        /// The tax's rate.
        rate: Rate,
        /// The amount taxed.
        taxable: Money,
        /// How the tax is rounded.
        mode: RoundingMode,
        /// The tax.
        amount: Money,
    },
    /// A tax on the order, rounded once, and allocated to the lines it applies to.
    TaxedDocument {
        /// The tax, by its position in the rules.
        tax: usize,
        /// The tax's rate.
        rate: Rate,
        /// The amount taxed: the nets of the lines it applies to.
        taxable: Money,
        /// How the tax is rounded.
        mode: RoundingMode,
        /// The tax.
        amount: Money,
        /// Each line's share, in the basket's order: `None` if the tax doesn't apply to it.
        shares: Vec<Option<Money>>,
    },
    /// The customer is exempt from a tax.
    Exempted {
        /// The tax, by its position in the rules.
        tax: usize,
    },
}

/// A rounding mode in words.
fn describe(mode: RoundingMode) -> &'static str {
    match mode {
        RoundingMode::HalfAwayFromZero => "half away from zero",
        RoundingMode::HalfEven => "half to even",
        RoundingMode::HalfTowardZero => "half toward zero",
        RoundingMode::AwayFromZero => "away from zero",
        RoundingMode::TowardZero => "toward zero",
        RoundingMode::Ceiling => "up",
        RoundingMode::Floor => "down",
    }
}

impl fmt::Display for Discount {
    /// `10%`, or `USD 5.00`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Discount::Percent(rate) => write!(f, "{rate}"),
            Discount::Amount(amount) => write!(f, "{amount}"),
        }
    }
}

/// A discount's result: `10% of USD 9.99 = USD 1.00, rounded half away from zero`.
fn discounted(
    f: &mut fmt::Formatter<'_>,
    value: Discount,
    base: Money,
    mode: RoundingMode,
    amount: Money,
    limited: bool,
) -> fmt::Result {
    match value {
        Discount::Percent(rate) => {
            write!(f, "{rate} of {base} = {amount}, rounded {}", describe(mode))
        }
        Discount::Amount(given) if limited => {
            write!(f, "{given} off {base} = {amount}, limited to what was left")
        }
        Discount::Amount(given) => write!(f, "{given} off {base} = {amount}"),
    }
}

/// Shares in the basket's order, `-` where there is none.
fn shares<'a>(
    f: &mut fmt::Formatter<'_>,
    shares: impl Iterator<Item = Option<&'a Money>>,
) -> fmt::Result {
    write!(f, "; shares")?;
    for share in shares {
        match share {
            Some(share) => write!(f, " {share}")?,
            None => write!(f, " -")?,
        }
    }
    Ok(())
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::Extended { line, item, modifiers, quantity, mode, gross } => write!(
                f,
                "line {}: ({item} + modifiers {modifiers}) × {quantity} = {gross}, rounded {}",
                Nth(*line),
                describe(*mode)
            ),
            Step::Comped { line, amount } => write!(f, "line {}: comped, {amount}", Nth(*line)),
            Step::LineDiscounted { line, discount, value, base, mode, amount, limited } => {
                write!(f, "line {}, discount {}: ", Nth(*line), Nth(*discount))?;
                discounted(f, *value, *base, *mode, *amount, *limited)
            }
            Step::OrderDiscounted {
                discount,
                value,
                base,
                mode,
                amount,
                limited,
                shares: parts,
            } => {
                write!(f, "order discount {}: ", Nth(*discount))?;
                discounted(f, *value, *base, *mode, *amount, *limited)?;
                shares(f, parts.iter().map(Some))
            }
            Step::TaxedLine { tax, line, rate, taxable, mode, amount } => write!(
                f,
                "tax {} on line {}: {rate} of {taxable} = {amount}, rounded {}",
                Nth(*tax),
                Nth(*line),
                describe(*mode)
            ),
            Step::TaxedDocument { tax, rate, taxable, mode, amount, shares: parts } => {
                write!(
                    f,
                    "tax {}: {rate} of {taxable} = {amount}, rounded {} once",
                    Nth(*tax),
                    describe(*mode)
                )?;
                shares(f, parts.iter().map(Option::as_ref))
            }
            Step::Exempted { tax } => write!(f, "tax {}: exempt", Nth(*tax)),
        }
    }
}
