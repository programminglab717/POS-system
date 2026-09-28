//! What a basket costs: every line's amounts, every discount and tax, and the order's totals.

use keel_types::{Currency, Id, Money};

use crate::rules::Tax;
use crate::trace::Step;

/// What a basket costs, and how each amount was reached.
///
/// Every set of parts adds up to its total exactly: the lines' amounts to the order's, each order
/// discount's shares to the discount, and each tax's lines to the tax. No amount is negative.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Totals {
    /// The currency of every amount.
    pub currency: Currency,
    /// Each line's amounts, in the basket's order.
    pub lines: Vec<LineTotals>,
    /// What each of the order's discounts took, in the basket's order.
    pub discounts: Vec<Money>,
    /// Each tax in the rules, in the rules' order.
    pub taxes: Vec<TaxTotal>,
    /// The lines' gross amounts, added up.
    pub gross: Money,
    /// Everything that comps and discounts took.
    pub discounted: Money,
    /// Gross less comps and discounts: what the lines are sold for, before tax.
    pub net: Money,
    /// All the tax.
    pub tax: Money,
    /// Net plus tax: what the customer pays.
    pub total: Money,
    /// Every step of the calculation, in order.
    pub trace: Vec<Step>,
}

/// One line's amounts.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LineTotals {
    /// The unit price with its modifiers: the item's price plus each modifier's.
    pub unit_price: Money,
    /// The unit price times the quantity.
    pub gross: Money,
    /// What a comp took: the whole gross if the line is comped, otherwise zero.
    pub comp: Money,
    /// What each of the line's own discounts took, in order.
    pub discounts: Vec<Money>,
    /// The line's share of each of the order's discounts, in order.
    pub order_discounts: Vec<Money>,
    /// What is left of the gross after comps and discounts: the line's price before tax.
    pub net: Money,
    /// Each tax in the rules, in order: `None` if it doesn't apply to the line.
    pub taxes: Vec<Option<LineTax>>,
    /// All the line's tax.
    pub tax: Money,
    /// Net plus tax.
    pub total: Money,
}

/// A tax on one line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LineTax {
    /// The amount taxed: the line's net.
    pub taxable: Money,
    /// The tax.
    pub tax: Money,
}

/// A tax on the whole order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TaxTotal {
    /// The tax.
    pub id: Id<Tax>,
    /// Whether the customer is exempt from it: then nothing is taxed.
    pub exempt: bool,
    /// The amount taxed: the nets of the lines it applies to.
    pub taxable: Money,
    /// The tax.
    pub tax: Money,
}
