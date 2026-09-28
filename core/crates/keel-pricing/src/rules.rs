//! How a location prices: its taxes, and how each rounding point rounds.

use keel_types::{Id, Rate, RoundingMode};

use crate::basket::{Dining, TaxCategory};

/// A location's pricing rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rules {
    /// How a unit price times a fractional quantity rounds to the minor unit.
    pub extension: RoundingMode,
    /// How a percentage discount rounds to the minor unit.
    pub discounts: RoundingMode,
    /// The taxes that may apply, each identified in the totals by its position here.
    pub taxes: Vec<Tax>,
    /// How tax rounds, as the jurisdiction requires.
    pub tax_rounding: TaxRounding,
}

impl Rules {
    /// Rules with no taxes, rounding half away from zero everywhere, and tax rounded per
    /// document.
    pub fn untaxed() -> Rules {
        Rules {
            extension: RoundingMode::HalfAwayFromZero,
            discounts: RoundingMode::HalfAwayFromZero,
            taxes: Vec::new(),
            tax_rounding: TaxRounding {
                scope: TaxScope::Document,
                mode: RoundingMode::HalfAwayFromZero,
            },
        }
    }
}

/// A tax added to the price of the lines it applies to, such as a state and city sales tax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tax {
    /// The tax's identifier, which exemptions refer to.
    pub id: Id<Tax>,
    /// Its name, as receipts and reports show it: "NYC sales tax".
    pub name: String,
    /// Its rate, 0 or more: 8.875% is `0.08875`.
    pub rate: Rate,
    /// The tax categories it applies to.
    pub categories: Vec<Id<TaxCategory>>,
    /// If set, the tax applies only when the order is eaten there, such as a tax on prepared food
    /// eaten on the premises.
    pub dining: Option<Dining>,
}

impl Tax {
    /// Whether the tax applies to an item of `category` in an order eaten as `dining`.
    pub fn applies(&self, category: Id<TaxCategory>, dining: Dining) -> bool {
        self.categories.contains(&category) && self.dining.is_none_or(|only| only == dining)
    }
}

/// How tax is rounded to the minor unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaxRounding {
    /// Where tax is rounded.
    pub scope: TaxScope,
    /// How.
    pub mode: RoundingMode,
}

/// Where tax is rounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaxScope {
    /// Each line's tax is rounded.
    Line,
    /// Each tax's total for the order is rounded once, then allocated to the lines.
    Document,
}
