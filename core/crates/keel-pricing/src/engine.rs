//! The pricing pipeline: extension, comps and line discounts, order discounts, then tax.

use core::cmp::Ordering;
use core::fmt;

use keel_types::{Currency, Decimal, Id, Money, MoneyError, RoundingMode};

use crate::basket::{Basket, Discount, Line, Modifier, TaxCategory};
use crate::rules::{Rules, TaxRounding, TaxScope};
use crate::totals::{LineTax, LineTotals, TaxTotal, Totals};
use crate::trace::Step;

/// The deepest modifiers may nest: a line's own modifiers are the first level.
pub const MAX_MODIFIER_DEPTH: usize = 32;

/// Why a basket couldn't be priced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PricingError {
    /// An amount is in another currency than the basket's.
    #[error("an amount is in {found}, not the basket's currency, {expected}")]
    CurrencyMismatch {
        /// The basket's currency.
        expected: Currency,
        /// The amount's.
        found: Currency,
    },
    /// A price, of an item or a modifier, is negative.
    #[error("line {}: a price is negative", Nth(*.line))]
    NegativePrice {
        /// The line, counted from zero.
        line: usize,
    },
    /// A line's quantity is zero or negative.
    #[error("line {}: the quantity isn't positive", Nth(*.line))]
    QuantityNotPositive {
        /// The line, counted from zero.
        line: usize,
    },
    /// Modifiers nest more deeply than [`MAX_MODIFIER_DEPTH`].
    #[error("line {}: modifiers nest more than {MAX_MODIFIER_DEPTH} deep", Nth(*.line))]
    ModifiersTooDeep {
        /// The line, counted from zero.
        line: usize,
    },
    /// A discount is negative, or a percentage is above 100%.
    #[error("{0} is out of range")]
    InvalidDiscount(DiscountRef),
    /// A tax's rate is negative.
    #[error("tax {}: the rate is negative", Nth(*.tax))]
    NegativeTaxRate {
        /// The tax, counted from zero.
        tax: usize,
    },
    /// Two taxes in the rules have the same identifier.
    #[error("tax {}: an earlier tax has the same identifier", Nth(*.tax))]
    DuplicateTax {
        /// The later tax, counted from zero.
        tax: usize,
    },
    /// An amount is too large to represent.
    #[error("an amount is out of range")]
    Overflow,
}

impl From<MoneyError> for PricingError {
    fn from(error: MoneyError) -> PricingError {
        match error {
            MoneyError::CurrencyMismatch { left, right } => {
                PricingError::CurrencyMismatch { expected: left, found: right }
            }
            _ => PricingError::Overflow,
        }
    }
}

/// Which discount: one of a line's, or one of the order's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscountRef {
    /// One of a line's discounts.
    Line {
        /// The line, counted from zero.
        line: usize,
        /// The discount among the line's, counted from zero.
        discount: usize,
    },
    /// One of the order's discounts.
    Order {
        /// The discount among the order's, counted from zero.
        discount: usize,
    },
}

impl fmt::Display for DiscountRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscountRef::Line { line, discount } => {
                write!(f, "discount {} of line {}", Nth(*discount), Nth(*line))
            }
            DiscountRef::Order { discount } => write!(f, "order discount {}", Nth(*discount)),
        }
    }
}

/// A position counted from one, as people count.
pub(crate) struct Nth(pub(crate) usize);

impl fmt::Display for Nth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.checked_add(1) {
            Some(nth) => write!(f, "{nth}"),
            None => write!(f, "{}+1", self.0),
        }
    }
}

/// Prices `basket` by `rules`: every line's gross, comps and discounts, net and taxes, and the
/// order's totals, with the trace of how each was reached.
///
/// The pipeline and its rounding points are fixed: extension, then comps and line discounts,
/// then order discounts, then tax (ADR-0014).
///
/// # Errors
/// [`PricingError`] if the basket or rules are invalid (another currency, a negative price, a
/// quantity that isn't positive, a discount out of range, a negative or repeated tax), or an
/// amount overflows.
pub fn price(basket: &Basket, rules: &Rules) -> Result<Totals, PricingError> {
    check(basket, rules)?;
    let mut trace = Vec::new();
    let mut lines = Vec::with_capacity(basket.lines.len());
    for (index, line) in basket.lines.iter().enumerate() {
        lines.push(price_line(index, line, rules, &mut trace)?);
    }
    let discounts = discount_order(basket, rules, &mut lines, &mut trace)?;
    let taxes = tax(basket, rules, &mut lines, &mut trace)?;
    assemble(basket.currency, lines, discounts, taxes, trace)
}

/// A line while it is being priced.
struct Work {
    category: Id<TaxCategory>,
    unit_price: Money,
    gross: Money,
    comp: Money,
    discounts: Vec<Money>,
    order_discounts: Vec<Money>,
    /// What is left of the line: its net, once every discount is taken.
    net: Money,
    taxes: Vec<Option<LineTax>>,
}

/// What a discount took from what was left.
struct Taken {
    amount: Money,
    limited: bool,
}

/// Takes `discount` from `rest`: a percentage of it, rounded, or an amount, at most all of it.
fn take(rest: Money, discount: Discount, mode: RoundingMode) -> Result<Taken, PricingError> {
    match discount {
        Discount::Percent(rate) => {
            Ok(Taken { amount: rest.mul_decimal(rate.as_fraction(), mode)?, limited: false })
        }
        Discount::Amount(amount) => {
            let limited = amount.compare(rest)? == Ordering::Greater;
            Ok(Taken { amount: if limited { rest } else { amount }, limited })
        }
    }
}

/// The price of `modifiers` for one of what they modify.
fn modifiers_price(modifiers: &[Modifier], currency: Currency) -> Result<Money, PricingError> {
    let mut total = Money::zero(currency);
    for modifier in modifiers {
        let each =
            modifier.unit_price.checked_add(modifiers_price(&modifier.modifiers, currency)?)?;
        let price = each.checked_mul(i64::from(modifier.quantity.get()))?;
        total = total.checked_add(price)?;
    }
    Ok(total)
}

/// Extends a line, then applies its comp and its discounts.
fn price_line(
    index: usize,
    line: &Line,
    rules: &Rules,
    trace: &mut Vec<Step>,
) -> Result<Work, PricingError> {
    let currency = line.unit_price.currency();
    let modifiers = modifiers_price(&line.modifiers, currency)?;
    let unit_price = line.unit_price.checked_add(modifiers)?;
    let gross = unit_price.mul_decimal(line.quantity.to_decimal(), rules.extension)?;
    trace.push(Step::Extended {
        line: index,
        item: line.unit_price,
        modifiers,
        quantity: line.quantity,
        mode: rules.extension,
        gross,
    });

    let zero = Money::zero(currency);
    let (comp, mut net) = if line.comped { (gross, zero) } else { (zero, gross) };
    if line.comped {
        trace.push(Step::Comped { line: index, amount: comp });
    }
    let mut discounts = Vec::with_capacity(line.discounts.len());
    for (position, &discount) in line.discounts.iter().enumerate() {
        let taken = take(net, discount, rules.discounts)?;
        trace.push(Step::LineDiscounted {
            line: index,
            discount: position,
            value: discount,
            base: net,
            mode: rules.discounts,
            amount: taken.amount,
            limited: taken.limited,
        });
        net = net.checked_sub(taken.amount)?;
        discounts.push(taken.amount);
    }
    Ok(Work {
        category: line.tax_category,
        unit_price,
        gross,
        comp,
        discounts,
        order_discounts: Vec::new(),
        net,
        taxes: Vec::new(),
    })
}

/// Splits `amount` among parts in proportion to `weights`, by largest remainder. The weights are
/// never negative, and when they add up to zero, so does `amount`. `amount` may exceed their sum:
/// a tax above 100% does.
fn allocate(amount: Money, weights: &[Money]) -> Result<Vec<Money>, PricingError> {
    if amount.is_zero() {
        return Ok(vec![Money::zero(amount.currency()); weights.len()]);
    }
    let weights = weights
        .iter()
        .map(|weight| u64::try_from(weight.minor()).map_err(|_| PricingError::Overflow))
        .collect::<Result<Vec<u64>, PricingError>>()?;
    Ok(amount.allocate(&weights)?)
}

/// Applies the order's discounts in turn, each allocated to the lines by what is left of them.
fn discount_order(
    basket: &Basket,
    rules: &Rules,
    lines: &mut [Work],
    trace: &mut Vec<Step>,
) -> Result<Vec<Money>, PricingError> {
    let mut taken = Vec::with_capacity(basket.discounts.len());
    for (position, &discount) in basket.discounts.iter().enumerate() {
        let rests: Vec<Money> = lines.iter().map(|line| line.net).collect();
        let base = Money::sum(basket.currency, rests.iter().copied())?;
        let taking = take(base, discount, rules.discounts)?;
        let shares = allocate(taking.amount, &rests)?;
        for (line, &share) in lines.iter_mut().zip(&shares) {
            line.net = line.net.checked_sub(share)?;
            line.order_discounts.push(share);
        }
        trace.push(Step::OrderDiscounted {
            discount: position,
            value: discount,
            base,
            mode: rules.discounts,
            amount: taking.amount,
            limited: taking.limited,
            shares,
        });
        taken.push(taking.amount);
    }
    Ok(taken)
}

/// Applies each tax to the lines it covers, rounded per line or per document.
fn tax(
    basket: &Basket,
    rules: &Rules,
    lines: &mut [Work],
    trace: &mut Vec<Step>,
) -> Result<Vec<TaxTotal>, PricingError> {
    let zero = Money::zero(basket.currency);
    let TaxRounding { scope, mode } = rules.tax_rounding;
    let mut totals = Vec::with_capacity(rules.taxes.len());
    for (position, tax) in rules.taxes.iter().enumerate() {
        if basket.exemptions.contains(&tax.id) {
            trace.push(Step::Exempted { tax: position });
            for line in lines.iter_mut() {
                line.taxes.push(None);
            }
            totals.push(TaxTotal { id: tax.id, exempt: true, taxable: zero, tax: zero });
            continue;
        }
        let rate = tax.rate;
        let taxable: Vec<Option<Money>> = lines
            .iter()
            .map(|line| tax.applies(line.category, basket.dining).then_some(line.net))
            .collect();
        let base = Money::sum(basket.currency, taxable.iter().flatten().copied())?;
        let amounts: Vec<Option<Money>> = match scope {
            TaxScope::Line => {
                let mut amounts = Vec::with_capacity(taxable.len());
                for (line, part) in taxable.iter().enumerate() {
                    let amount = match part {
                        Some(part) => {
                            let amount = part.mul_decimal(rate.as_fraction(), mode)?;
                            trace.push(Step::TaxedLine {
                                tax: position,
                                line,
                                rate,
                                taxable: *part,
                                mode,
                                amount,
                            });
                            Some(amount)
                        }
                        None => None,
                    };
                    amounts.push(amount);
                }
                amounts
            }
            TaxScope::Document => {
                let amount = base.mul_decimal(rate.as_fraction(), mode)?;
                let weights: Vec<Money> = taxable.iter().map(|part| part.unwrap_or(zero)).collect();
                let shares: Vec<Option<Money>> = allocate(amount, &weights)?
                    .into_iter()
                    .zip(&taxable)
                    .map(|(share, part)| part.map(|_| share))
                    .collect();
                trace.push(Step::TaxedDocument {
                    tax: position,
                    rate,
                    taxable: base,
                    mode,
                    amount,
                    shares: shares.clone(),
                });
                shares
            }
        };
        let total = Money::sum(basket.currency, amounts.iter().flatten().copied())?;
        for ((line, part), amount) in lines.iter_mut().zip(&taxable).zip(&amounts) {
            line.taxes.push(match (part, amount) {
                (Some(taxable), Some(tax)) => Some(LineTax { taxable: *taxable, tax: *tax }),
                _ => None,
            });
        }
        totals.push(TaxTotal { id: tax.id, exempt: false, taxable: base, tax: total });
    }
    Ok(totals)
}

/// Adds everything up.
fn assemble(
    currency: Currency,
    lines: Vec<Work>,
    discounts: Vec<Money>,
    taxes: Vec<TaxTotal>,
    trace: Vec<Step>,
) -> Result<Totals, PricingError> {
    let lines = lines
        .into_iter()
        .map(|work| {
            let tax = Money::sum(currency, work.taxes.iter().flatten().map(|part| part.tax))?;
            let total = work.net.checked_add(tax)?;
            Ok(LineTotals {
                unit_price: work.unit_price,
                gross: work.gross,
                comp: work.comp,
                discounts: work.discounts,
                order_discounts: work.order_discounts,
                net: work.net,
                taxes: work.taxes,
                tax,
                total,
            })
        })
        .collect::<Result<Vec<LineTotals>, PricingError>>()?;
    let gross = Money::sum(currency, lines.iter().map(|line| line.gross))?;
    let net = Money::sum(currency, lines.iter().map(|line| line.net))?;
    let discounted = gross.checked_sub(net)?;
    let tax = Money::sum(currency, taxes.iter().map(|total| total.tax))?;
    let total = net.checked_add(tax)?;
    Ok(Totals { currency, lines, discounts, taxes, gross, discounted, net, tax, total, trace })
}

/// Checks everything a calculation relies on before it starts.
fn check(basket: &Basket, rules: &Rules) -> Result<(), PricingError> {
    let currency = basket.currency;
    let same_currency = |amount: Money| {
        if amount.currency() == currency {
            Ok(())
        } else {
            Err(PricingError::CurrencyMismatch { expected: currency, found: amount.currency() })
        }
    };
    for (index, line) in basket.lines.iter().enumerate() {
        same_currency(line.unit_price)?;
        if line.unit_price.is_negative() {
            return Err(PricingError::NegativePrice { line: index });
        }
        check_modifiers(&line.modifiers, index, 1, &same_currency)?;
        if !line.quantity.is_positive() {
            return Err(PricingError::QuantityNotPositive { line: index });
        }
        for (position, &discount) in line.discounts.iter().enumerate() {
            let at = DiscountRef::Line { line: index, discount: position };
            check_discount(discount, at, &same_currency)?;
        }
    }
    for (position, &discount) in basket.discounts.iter().enumerate() {
        check_discount(discount, DiscountRef::Order { discount: position }, &same_currency)?;
    }
    for (position, tax) in rules.taxes.iter().enumerate() {
        if tax.rate.as_fraction() < Decimal::ZERO {
            return Err(PricingError::NegativeTaxRate { tax: position });
        }
        let earlier = rules.taxes.get(..position).unwrap_or_default();
        if earlier.iter().any(|other| other.id == tax.id) {
            return Err(PricingError::DuplicateTax { tax: position });
        }
    }
    Ok(())
}

fn check_modifiers(
    modifiers: &[Modifier],
    line: usize,
    depth: usize,
    same_currency: &impl Fn(Money) -> Result<(), PricingError>,
) -> Result<(), PricingError> {
    if modifiers.is_empty() {
        return Ok(());
    }
    if depth > MAX_MODIFIER_DEPTH {
        return Err(PricingError::ModifiersTooDeep { line });
    }
    let deeper = depth.checked_add(1).ok_or(PricingError::ModifiersTooDeep { line })?;
    for modifier in modifiers {
        same_currency(modifier.unit_price)?;
        if modifier.unit_price.is_negative() {
            return Err(PricingError::NegativePrice { line });
        }
        check_modifiers(&modifier.modifiers, line, deeper, same_currency)?;
    }
    Ok(())
}

fn check_discount(
    discount: Discount,
    at: DiscountRef,
    same_currency: &impl Fn(Money) -> Result<(), PricingError>,
) -> Result<(), PricingError> {
    let valid = match discount {
        Discount::Percent(rate) => (Decimal::ZERO..=Decimal::ONE).contains(&rate.as_fraction()),
        Discount::Amount(amount) => {
            same_currency(amount)?;
            !amount.is_negative()
        }
    };
    if valid { Ok(()) } else { Err(PricingError::InvalidDiscount(at)) }
}
