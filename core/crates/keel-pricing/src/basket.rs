//! What to price: an order's lines as they were rung up, and its discounts.

use core::num::NonZeroU32;

use keel_types::{Currency, Id, Money, Quantity, Rate};

use crate::rules::Tax;

/// Marks the identifiers of tax categories, which say how an item is taxed ("prepared food",
/// "grocery", "alcohol"). The catalog assigns one to each item, and a line records it when it is
/// rung up.
#[derive(Debug)]
pub enum TaxCategory {}

/// An order to price, in one currency.
///
/// Lines, and the order's discounts, are identified in the [`Totals`](crate::Totals) and the
/// trace by their position here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Basket {
    /// The currency of every amount in the basket.
    pub currency: Currency,
    /// The lines that count, in order: removed and voided lines are left out.
    pub lines: Vec<Line>,
    /// Discounts on the whole order, applied in turn after the lines' own.
    pub discounts: Vec<Discount>,
    /// Whether the order is eaten on the premises or taken away, which some taxes depend on.
    pub dining: Dining,
    /// The taxes the customer doesn't pay, such as under an exemption certificate.
    pub exemptions: Vec<Id<Tax>>,
}

/// A line as it was rung up: the prices are those the catalog gave when it was added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The item's price for one unit of the quantity: each, or per kilogram.
    pub unit_price: Money,
    /// The modifiers chosen, each priced per unit of the line's quantity.
    pub modifiers: Vec<Modifier>,
    /// How much: a count, or a weight or measure.
    pub quantity: Quantity,
    /// How the item is taxed.
    pub tax_category: Id<TaxCategory>,
    /// Whether the line is given away: a comp takes all of it.
    pub comped: bool,
    /// Discounts on this line, applied in turn.
    pub discounts: Vec<Discount>,
    /// The part of the line this basket holds, when the line is shared among several baskets,
    /// such as the checks a bottle of wine is split between. `None` for the whole line.
    pub share: Option<Share>,
}

/// One basket's part of a line shared among several, such as one check's third of a bottle of
/// wine split three ways.
///
/// The line's gross is split among all the parts by largest remainder, in proportion to their
/// weights, so the parts always add up to the line. Every basket holding a part of the line
/// gives the same weights in the same order, and its own index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    /// The weight of every part of the line, in order: each 1 or more.
    pub weights: Vec<u64>,
    /// Which part this basket holds, counted from zero.
    pub index: usize,
}

/// A modifier chosen for a line, or for another modifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modifier {
    /// Its price, for one of it.
    pub unit_price: Money,
    /// How many of it, per unit of what it modifies: "2× extra cheese".
    pub quantity: NonZeroU32,
    /// Modifiers chosen for it, each priced per one of it.
    pub modifiers: Vec<Modifier>,
}

/// A discount on a line or on the order. Each takes from what is left after the discounts
/// before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discount {
    /// A share of what is left, from 0% to 100%, rounded to the minor unit.
    Percent(Rate),
    /// An amount, but never more than what is left.
    Amount(Money),
}

/// Where an order is eaten.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dining {
    /// On the premises: dine-in.
    OnPremises,
    /// Taken away: takeout, pickup, delivery and the like.
    ToGo,
}
