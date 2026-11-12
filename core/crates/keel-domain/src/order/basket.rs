//! Pricing an order: the baskets its live lines make, for `keel-pricing`.

use core::num::NonZeroU32;

use keel_pricing::{Basket, Dining, Discount, Modifier, PricingError, Rules, Share};
use keel_types::{Id, Money, Rate};

use super::checks::Check;
use super::state::{Line, Order, OrderInfo};
use super::types::{ChosenModifier, DiscountApplied, DiscountValue, MAX_DISCOUNT_PERCENT, Mode};

impl Order {
    /// The basket that prices the whole order as one sale: its live lines, each with the prices
    /// it was rung up with, and comped if it was. `None` until the order is created.
    ///
    /// The basket's lines are the order's [live lines](Order::live_lines), in the same order, so
    /// `order.live_lines().zip(&totals.lines)` pairs each line with its amounts. Each line
    /// carries its discounts, and the basket the whole order's, in the order they were applied
    /// (ADR-0030); orders don't record exemptions yet. An order is eaten on the premises when its
    /// mode is dine-in; every other mode takes it away.
    ///
    /// An order split among several checks is paid as several sales: see
    /// [`Order::check_basket`].
    pub fn basket(&self) -> Option<Basket> {
        let info = self.info()?;
        let lines = self.live_lines().map(|line| priced(line, None)).collect();
        Some(sale(info, lines, self.discounts()))
    }

    /// The basket that prices `check` as its own sale: the live lines allocated to it, in the
    /// order's order, each whole if the check has all of it, or else with the check's share of it
    /// (ADR-0015). `None` until the order is created, or if it has no such check.
    ///
    /// A shared line's share lists every check's shares of it, in ascending order of the checks'
    /// identifiers, so the baskets of all the checks sharing it split it the same way, and their
    /// parts add up to the whole line. Its percentage discounts apply to each part as they are,
    /// and an amount off is split among the parts the same way, so each check takes its part of
    /// it. The whole order's discounts, all percentages, apply to each check's basket.
    pub fn check_basket(&self, check: Id<Check>) -> Option<Basket> {
        let info = self.info()?;
        self.check(check)?;
        let lines = self
            .lines_on(check)
            .filter_map(|line| {
                let allocation = line.allocation();
                let index = allocation.iter().position(|share| share.check == check)?;
                let share = (allocation.len() > 1).then(|| Share {
                    weights: allocation.iter().map(|share| u64::from(share.shares.get())).collect(),
                    index,
                });
                Some(priced(line, share))
            })
            .collect();
        Some(sale(info, lines, self.discounts()))
    }

    /// The live lines with a part on `check`, in the order's order: the lines its basket holds,
    /// so `order.lines_on(check).zip(&totals.lines)` pairs each with its amounts.
    pub fn lines_on(&self, check: Id<Check>) -> impl Iterator<Item = &Line> {
        self.live_lines()
            .filter(move |line| line.allocation().iter().any(|share| share.check == check))
    }
}

/// What the order's discounts would take, with a discount applied or a line's quantity changed,
/// for the approval it may need (ADR-0030).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscountEffect {
    /// What the discount itself would take: nothing, for a quantity changed.
    pub taken: Money,
    /// What all the order's discounts would take together, before tax.
    pub total: Money,
    /// The share its scope's discounts would take together, in basis points, rounded up: on a
    /// line, of the line's gross less its comp; on the whole order, of what the lines' own
    /// discounts leave. Percentages compound, as they apply: two of 10% are 19%; an amount
    /// counts as its share of what is left when it applies. So a role's limit holds for the
    /// discounts together, however many: stacked, they count as one. Found in review: five of
    /// 10% each took 41% with no approval.
    pub percent: u32,
}

impl Order {
    /// What the order's discounts would take with `applied`, priced by `rules`, after the
    /// discounts it has. `None` until the order is created, or for a line that isn't live.
    ///
    /// # Errors
    /// [`PricingError`] if the order, with the discount, can't be priced.
    pub fn discount_effect(
        &self,
        applied: &DiscountApplied,
        rules: &Rules,
    ) -> Result<Option<DiscountEffect>, PricingError> {
        let Some(mut basket) = self.basket() else { return Ok(None) };
        let priced = discount(applied, None);
        let Some(at) = applied.line else {
            basket.discounts.push(priced);
            let totals = keel_pricing::price(&basket, rules)?;
            let taken = totals.discounts.last().copied().unwrap_or(Money::zero(basket.currency));
            let mut values: Vec<DiscountValue> =
                self.discounts().iter().map(|discount| discount.value).collect();
            values.push(applied.value);
            return effect(&totals, None, &values, taken).map(Some);
        };
        let Some(index) = self.live_lines().position(|line| line.id() == at) else {
            return Ok(None);
        };
        let Some(line) = basket.lines.get_mut(index) else { return Ok(None) };
        line.discounts.push(priced);
        let totals = keel_pricing::price(&basket, rules)?;
        let Some(priced_line) = totals.lines.get(index) else { return Ok(None) };
        let taken = priced_line.discounts.last().copied().unwrap_or(Money::zero(basket.currency));
        let mut values = self.line_values(at);
        values.push(applied.value);
        effect(&totals, Some(index), &values, taken).map(Some)
    }

    /// What the order's discounts would take were `line` `quantity` of it, priced by `rules`:
    /// for a line's quantity raised, which raises what its percentages and the order's take.
    /// `None` until the order is created, or for a line that isn't live.
    ///
    /// # Errors
    /// [`PricingError`] if the order, so changed, can't be priced.
    pub fn quantity_effect(
        &self,
        line: Id<Line>,
        quantity: keel_types::Quantity,
        rules: &Rules,
    ) -> Result<Option<DiscountEffect>, PricingError> {
        let Some(mut basket) = self.basket() else { return Ok(None) };
        let Some(index) = self.live_lines().position(|live| live.id() == line) else {
            return Ok(None);
        };
        let Some(priced) = basket.lines.get_mut(index) else { return Ok(None) };
        priced.quantity = quantity;
        let totals = keel_pricing::price(&basket, rules)?;
        let values = self.line_values(line);
        effect(&totals, Some(index), &values, Money::zero(basket.currency)).map(Some)
    }

    /// What `line`'s discounts take, in the order they apply.
    fn line_values(&self, line: Id<Line>) -> Vec<DiscountValue> {
        self.line(line)
            .map(|line| line.discounts().iter().map(|discount| discount.value).collect())
            .unwrap_or_default()
    }
}

/// The effect `totals` price: all the discounts, and the share those of the scope take, the line
/// at `scope` or the whole order, whose values are `values`, in the order they apply.
fn effect(
    totals: &keel_pricing::Totals,
    scope: Option<usize>,
    values: &[DiscountValue],
    taken: Money,
) -> Result<DiscountEffect, PricingError> {
    let currency = totals.currency;
    let lines = totals.lines.iter().flat_map(|line| &line.discounts);
    let total = Money::sum(currency, lines.chain(&totals.discounts).copied())?;
    let (scoped, base) = if let Some(line) = scope.and_then(|at| totals.lines.get(at)) {
        (line.discounts.clone(), line.gross.checked_sub(line.comp)?)
    } else {
        let mut base = Money::zero(currency);
        for line in &totals.lines {
            let own = Money::sum(currency, line.discounts.iter().copied())?;
            base = base.checked_add(line.gross.checked_sub(line.comp)?.checked_sub(own)?)?;
        }
        (totals.discounts.clone(), base)
    };
    Ok(DiscountEffect { taken, total, percent: share(values, &scoped, base) })
}

/// The share discounts of `values`, which took `taken` each from `base` in turn, take together,
/// in basis points, rounded up: what is left is what each leaves of what the ones before it
/// left, exactly. Past what an `i128` holds, or with nothing to take from, all of it.
fn share(values: &[DiscountValue], taken: &[Money], base: Money) -> u32 {
    let all = MAX_DISCOUNT_PERCENT;
    let mut left = i128::from(base.minor());
    // What is left, as a fraction of the base: kept in lowest terms.
    let (mut kept, mut of) = (1_i128, 1_i128);
    for (value, taken) in values.iter().zip(taken) {
        let taken = i128::from(taken.minor());
        let (keep, from) = match *value {
            DiscountValue::Percent(bp) => {
                let bp = i128::from(bp);
                (10_000_i128.saturating_sub(bp), 10_000)
            }
            DiscountValue::Amount(_) if left > 0 => (left.saturating_sub(taken), left),
            DiscountValue::Amount(_) if taken > 0 => return all,
            DiscountValue::Amount(_) => (1, 1),
        };
        let (Some(next_kept), Some(next_of)) = (kept.checked_mul(keep), of.checked_mul(from))
        else {
            return all;
        };
        let divisor = gcd(next_kept, next_of).max(1);
        let (Some(reduced_kept), Some(reduced_of)) =
            (next_kept.checked_div(divisor), next_of.checked_div(divisor))
        else {
            return all;
        };
        (kept, of) = (reduced_kept, reduced_of);
        left = left.saturating_sub(taken);
    }
    // (of − kept) / of, in basis points, rounded up.
    let share = of
        .checked_sub(kept)
        .and_then(|gone| gone.checked_mul(10_000))
        .and_then(|scaled| scaled.checked_add(of)?.checked_sub(1)?.checked_div(of));
    share.and_then(|share| u32::try_from(share).ok()).map_or(all, |share| share.min(all))
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while let Some(rest) = a.checked_rem(b) {
        (a, b) = (b, rest);
    }
    i128::try_from(a).unwrap_or(1)
}

/// A basket of `lines` for the order described by `info`, with the whole order's `discounts`.
fn sale(info: &OrderInfo, lines: Vec<keel_pricing::Line>, discounts: &[DiscountApplied]) -> Basket {
    let dining = if info.mode == Mode::DineIn { Dining::OnPremises } else { Dining::ToGo };
    let discounts = discounts.iter().map(|applied| discount(applied, None)).collect();
    Basket { currency: info.currency, lines, discounts, dining, exemptions: Vec::new() }
}

/// A discount as pricing sees it, on a whole line or on `share` of it: an amount off a share
/// is the share's part of the amount, split as the line's gross is.
fn discount(applied: &DiscountApplied, share: Option<&Share>) -> Discount {
    match applied.value {
        DiscountValue::Percent(bp) => Discount::Percent(Rate::from_basis_points(i64::from(bp))),
        DiscountValue::Amount(amount) => {
            let part = share
                .and_then(|share| amount.allocate(&share.weights).ok()?.get(share.index).copied());
            Discount::Amount(part.unwrap_or(amount))
        }
    }
}

/// A line as pricing sees it, whole or shared.
fn priced(line: &Line, share: Option<Share>) -> keel_pricing::Line {
    keel_pricing::Line {
        unit_price: line.unit_price(),
        modifiers: line.modifiers().iter().map(modifier).collect(),
        quantity: line.quantity(),
        tax_category: line.item().tax_category.cast(),
        comped: line.comp().is_some(),
        discounts: line
            .discounts()
            .iter()
            .map(|applied| discount(applied, share.as_ref()))
            .collect(),
        share,
    }
}

/// A chosen modifier as pricing sees it: its price, quantity and nested modifiers.
fn modifier(chosen: &ChosenModifier) -> Modifier {
    Modifier {
        unit_price: chosen.unit_price,
        quantity: NonZeroU32::from(chosen.quantity),
        modifiers: chosen.modifiers.iter().map(modifier).collect(),
    }
}
