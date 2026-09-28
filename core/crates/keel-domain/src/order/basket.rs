//! Pricing an order: the basket its live lines make, for `keel-pricing`.

use core::num::NonZeroU32;

use keel_pricing::{Basket, Dining, Modifier};

use super::state::Order;
use super::types::{ChosenModifier, Mode};

impl Order {
    /// The basket that prices the order: its live lines, each with the prices it was rung up
    /// with, and comped if it was. `None` until the order is created.
    ///
    /// The basket's lines are the order's [live lines](Order::live_lines), in the same order, so
    /// `order.live_lines().zip(&totals.lines)` pairs each line with its amounts. Orders don't
    /// record discounts or exemptions yet, so the basket has none. An order is eaten on the
    /// premises when its mode is dine-in; every other mode takes it away.
    pub fn basket(&self) -> Option<Basket> {
        let info = self.info()?;
        let lines = self
            .live_lines()
            .map(|line| keel_pricing::Line {
                unit_price: line.item().unit_price,
                modifiers: line.modifiers().iter().map(modifier).collect(),
                quantity: line.quantity(),
                tax_category: line.item().tax_category.cast(),
                comped: line.comp().is_some(),
                discounts: Vec::new(),
            })
            .collect();
        let dining = if info.mode == Mode::DineIn { Dining::OnPremises } else { Dining::ToGo };
        Some(Basket {
            currency: info.currency,
            lines,
            discounts: Vec::new(),
            dining,
            exemptions: Vec::new(),
        })
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
