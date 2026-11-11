//! Pricing an order: the baskets its live lines make, for `keel-pricing`.

use core::num::NonZeroU32;

use keel_pricing::{Basket, Dining, Discount, Modifier, Share};
use keel_types::{Id, Rate};

use super::checks::Check;
use super::state::{Line, Order, OrderInfo};
use super::types::{ChosenModifier, DiscountApplied, DiscountValue, Mode};

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
        unit_price: line.item().unit_price,
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
