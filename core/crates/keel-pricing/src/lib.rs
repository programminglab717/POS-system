//! # keel-pricing
//!
//! Keel's pricing engine: what an order costs, to the minor unit, and why.
//!
//! [`price`] turns a [`Basket`] (an order's lines as they were rung up, with their discounts) and a
//! location's [`Rules`] (its taxes and rounding modes) into [`Totals`]: each line's gross,
//! discounts, net and taxes; each discount's allocation to the lines; each tax; and the order's
//! totals. A [`Step`]-by-step trace explains every amount.
//!
//! The pipeline runs in a fixed order, and rounds at five points only, each with an explicit
//! rounding mode (ADR-0014):
//!
//! 1. **Extension**: a line's unit price, the item's plus its modifiers', times its quantity,
//!    rounded when the quantity is fractional (weighed or measured items). A line shared among
//!    several baskets, such as the checks a bottle of wine is split between, is split by largest
//!    remainder, and the basket holds its part ([`Share`]).
//! 2. **Comps and line discounts**, each taking from what is left of the line; a percentage is
//!    rounded.
//! 3. **Order discounts**, each taking from what is left of the order, a percentage rounded once,
//!    then **allocated** to the lines by largest remainder, so the shares add up exactly.
//! 4. **Tax**, on each line's net, rounded per line or once per document, as the jurisdiction
//!    requires; a per-document tax is allocated to the lines like a discount.
//! 5. **Cash rounding**, when the customer pays in cash ([`round_cash`]).
//!
//! ```
//! use keel_pricing::{Basket, Dining, Discount, Line, Rules, Tax, TaxRounding, TaxScope, price};
//! use keel_types::{Currency, Id, Money, Quantity, Rate, RoundingMode, Unit};
//!
//! let food = Id::parse("0192f0c1-0000-7000-8000-000000000001")?;
//! let usd = |amount: &str| Money::parse(amount, Currency::USD);
//! let rules = Rules {
//!     taxes: vec![Tax {
//!         id: Id::parse("0192f0c1-0000-7000-8000-000000000002")?,
//!         name: "City sales tax".to_owned(),
//!         rate: Rate::from_basis_points(888),
//!         categories: vec![food],
//!         dining: None,
//!     }],
//!     tax_rounding: TaxRounding { scope: TaxScope::Document, mode: RoundingMode::HalfAwayFromZero },
//!     ..Rules::untaxed()
//! };
//! let basket = Basket {
//!     currency: Currency::USD,
//!     lines: vec![Line {
//!         unit_price: usd("4.50")?,
//!         modifiers: Vec::new(),
//!         quantity: Quantity::from_whole(2, Unit::Each)?,
//!         tax_category: food,
//!         comped: false,
//!         discounts: vec![Discount::Percent(Rate::from_basis_points(1000))],
//!         share: None,
//!     }],
//!     discounts: Vec::new(),
//!     dining: Dining::ToGo,
//!     exemptions: Vec::new(),
//! };
//! let totals = price(&basket, &rules)?;
//! // Two at USD 4.50 is USD 9.00; 10% off leaves USD 8.10; 8.88% tax on it is USD 0.719..., so
//! // USD 0.72.
//! assert_eq!(totals.net, usd("8.10")?);
//! assert_eq!(totals.tax, usd("0.72")?);
//! assert_eq!(totals.total, usd("8.82")?);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Like the rest of the kernel, this crate is deterministic, never panics, and builds for
//! `wasm32`.

mod basket;
mod cash;
mod engine;
mod rules;
#[cfg(test)]
mod tests;
mod totals;
mod trace;

pub use basket::{Basket, Dining, Discount, Line, Modifier, Share, TaxCategory};
pub use cash::{CashDue, round_cash};
pub use engine::{DiscountRef, MAX_MODIFIER_DEPTH, PricingError, price};
pub use rules::{Rules, Tax, TaxRounding, TaxScope};
pub use totals::{LineTax, LineTotals, TaxTotal, Totals};
pub use trace::Step;
