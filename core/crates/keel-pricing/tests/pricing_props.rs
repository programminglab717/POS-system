//! Property tests for the pricing engine:
//! - every amount agrees with an exact oracle written from ADR-0014, and when an amount doesn't
//!   fit in the engine's range, the engine reports an overflow rather than a wrong number;
//! - the parts add up, nothing is negative, and every allocated share is within one minor unit
//!   of its exact share;
//! - the trace explains the totals: replaying it gives every line's amounts;
//! - the order of the lines doesn't change what the order costs, where it can't;
//! - the parts of a shared line add up to the line, each within one minor unit of its exact
//!   share;
//! - every kind of invalid input is refused with the error that names it.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU32;

use keel_pricing::{
    Basket, Discount, DiscountRef, MAX_MODIFIER_DEPTH, Modifier, PricingError, Rules, Share, Step,
    TaxScope, Totals, price, round_cash,
};
use keel_types::{Currency, Decimal, Money, Quantity, Rate, RoundingRule};
use num_bigint::BigInt;
use proptest::prelude::*;
use proptest::sample::Index;
use support::{Expected, ExpectedLine, any_case, any_mode, expected, round_rational};

fn minor(money: Money) -> BigInt {
    BigInt::from(money.minor())
}

fn minors(amounts: &[Money]) -> Vec<BigInt> {
    amounts.iter().copied().map(minor).collect()
}

/// The engine's totals, in the oracle's terms. The whole line's gross, before a share of it is
/// taken, is only in the trace.
fn observed(totals: &Totals) -> Expected {
    let whole = |at: usize| {
        totals.trace.iter().find_map(|step| match step {
            Step::Extended { line, gross, .. } if *line == at => Some(minor(*gross)),
            _ => None,
        })
    };
    Expected {
        lines: totals
            .lines
            .iter()
            .enumerate()
            .map(|(at, line)| ExpectedLine {
                unit_price: minor(line.unit_price),
                whole: whole(at).unwrap_or_default(),
                gross: minor(line.gross),
                comp: minor(line.comp),
                discounts: minors(&line.discounts),
                order_discounts: minors(&line.order_discounts),
                net: minor(line.net),
                taxes: line
                    .taxes
                    .iter()
                    .map(|tax| tax.map(|tax| (minor(tax.taxable), minor(tax.tax))))
                    .collect(),
                tax: minor(line.tax),
                total: minor(line.total),
            })
            .collect(),
        discounts: minors(&totals.discounts),
        taxes: totals
            .taxes
            .iter()
            .map(|tax| (tax.exempt, minor(tax.taxable), minor(tax.tax)))
            .collect(),
        gross: minor(totals.gross),
        discounted: minor(totals.discounted),
        net: minor(totals.net),
        tax: minor(totals.tax),
        total: minor(totals.total),
    }
}

fn sum(amounts: impl IntoIterator<Item = Money>) -> BigInt {
    amounts.into_iter().map(minor).sum()
}

/// Whether `share` is within one minor unit of `amount × weight / total`.
fn near_exact_share(share: Money, amount: Money, weight: Money, total: &BigInt) -> bool {
    near_exact(share, amount, &minor(weight), total)
}

/// Whether `share` is within one minor unit of `amount × weight / total`, for any weights.
fn near_exact(share: Money, amount: Money, weight: &BigInt, total: &BigInt) -> bool {
    if *total == BigInt::ZERO {
        return share.is_zero();
    }
    // |share × total − amount × weight| < total
    let gap = minor(share) * total - minor(amount) * weight;
    gap < *total && -gap < *total
}

/// A fault to plant in a valid basket or its rules, and the error it must cause.
#[derive(Clone, Debug)]
enum Fault {
    NegativePrice(Index),
    NegativeModifier(Index),
    Quantity(Index, i64),
    LineDiscount(Index, Discount),
    OrderDiscount(Discount),
    NegativeRate(Index),
    RepeatedTax(Index),
    OtherCurrency(Index),
    TooDeep(Index),
    InvalidShare(Index, Share),
}

fn any_fault() -> impl Strategy<Value = Fault> {
    let euros = || (0_i64..100).prop_map(|minor| Money::from_minor(minor, Currency::EUR));
    let bad_discount = prop_oneof![
        (1_i64..20_000).prop_map(|excess| Discount::Percent(Rate::from_fraction(
            Decimal::ONE + Decimal::new(excess, 6)
        ))),
        (1_i64..10_000).prop_map(|bp| Discount::Percent(Rate::from_basis_points(-bp))),
        (1_i64..10_000)
            .prop_map(|minor| Discount::Amount(Money::from_minor(-minor, Currency::USD))),
        euros().prop_map(Discount::Amount),
    ];
    prop_oneof![
        any::<Index>().prop_map(Fault::NegativePrice),
        any::<Index>().prop_map(Fault::NegativeModifier),
        (any::<Index>(), prop_oneof![Just(0_i64), i64::MIN..0])
            .prop_map(|(i, m)| Fault::Quantity(i, m)),
        (any::<Index>(), bad_discount.clone()).prop_map(|(i, d)| Fault::LineDiscount(i, d)),
        bad_discount.prop_map(Fault::OrderDiscount),
        any::<Index>().prop_map(Fault::NegativeRate),
        any::<Index>().prop_map(Fault::RepeatedTax),
        any::<Index>().prop_map(Fault::OtherCurrency),
        any::<Index>().prop_map(Fault::TooDeep),
        (any::<Index>(), any_invalid_share()).prop_map(|(i, s)| Fault::InvalidShare(i, s)),
    ]
}

/// A share with no weights, a weight of zero, or an index past its weights.
fn any_invalid_share() -> impl Strategy<Value = Share> {
    let weights = || prop::collection::vec(1_u64..=3, 1..=4);
    prop_oneof![
        Just(Share { weights: Vec::new(), index: 0 }),
        (weights(), any::<Index>(), any::<Index>()).prop_map(|(mut weights, zero, index)| {
            let parts = weights.len();
            weights[zero.index(parts)] = 0;
            Share { weights, index: index.index(parts) }
        }),
        (weights(), 0_usize..3)
            .prop_map(|(weights, past)| Share { index: weights.len() + past, weights }),
    ]
}

/// Plants `fault` in a valid basket and rules, and returns the error it must cause, or `None`
/// if the case has nowhere to plant it.
fn plant(fault: &Fault, basket: &mut Basket, rules: &mut Rules) -> Option<PricingError> {
    let lines = basket.lines.len();
    let currency = basket.currency;
    let other = if currency == Currency::EUR { Currency::USD } else { Currency::EUR };
    let deep = |levels: usize| free_chain(levels, currency);
    match fault {
        Fault::NegativePrice(at) if lines > 0 => {
            let line = at.index(lines);
            basket.lines[line].unit_price = Money::from_minor(-1, currency);
            Some(PricingError::NegativePrice { line })
        }
        Fault::NegativeModifier(at) if lines > 0 => {
            let line = at.index(lines);
            let mut negative = deep(1);
            negative.unit_price = Money::from_minor(-1, currency);
            basket.lines[line].modifiers.push(negative);
            Some(PricingError::NegativePrice { line })
        }
        Fault::Quantity(at, micros) if lines > 0 => {
            let line = at.index(lines);
            let unit = basket.lines[line].quantity.unit();
            basket.lines[line].quantity = Quantity::from_micros(*micros, unit);
            Some(PricingError::QuantityNotPositive { line })
        }
        Fault::LineDiscount(at, discount) if lines > 0 => {
            let line = at.index(lines);
            let position = basket.lines[line].discounts.len();
            basket.lines[line].discounts.push(retarget(*discount, currency));
            let at = DiscountRef::Line { line, discount: position };
            Some(expected_discount_error(*discount, currency, at))
        }
        Fault::OrderDiscount(discount) => {
            let position = basket.discounts.len();
            basket.discounts.push(retarget(*discount, currency));
            let at = DiscountRef::Order { discount: position };
            Some(expected_discount_error(*discount, currency, at))
        }
        Fault::NegativeRate(at) if !rules.taxes.is_empty() => {
            let tax = at.index(rules.taxes.len());
            rules.taxes[tax].rate = Rate::from_basis_points(-1);
            Some(PricingError::NegativeTaxRate { tax })
        }
        Fault::RepeatedTax(at) if !rules.taxes.is_empty() => {
            let copy = rules.taxes[at.index(rules.taxes.len())].clone();
            rules.taxes.push(copy);
            Some(PricingError::DuplicateTax { tax: rules.taxes.len() - 1 })
        }
        Fault::OtherCurrency(at) if lines > 0 => {
            let line = at.index(lines);
            let price = basket.lines[line].unit_price;
            basket.lines[line].unit_price = Money::from_minor(price.minor(), other);
            Some(PricingError::CurrencyMismatch { expected: currency, found: other })
        }
        Fault::TooDeep(at) if lines > 0 => {
            let line = at.index(lines);
            basket.lines[line].modifiers.push(deep(MAX_MODIFIER_DEPTH + 1));
            Some(PricingError::ModifiersTooDeep { line })
        }
        Fault::InvalidShare(at, share) if lines > 0 => {
            let line = at.index(lines);
            basket.lines[line].share = Some(share.clone());
            Some(PricingError::InvalidShare { line })
        }
        _ => None,
    }
}

/// Free modifiers nested `levels` deep: each one's only modifier is the next.
fn free_chain(levels: usize, currency: Currency) -> Modifier {
    let mut chain = Modifier {
        unit_price: Money::zero(currency),
        quantity: NonZeroU32::MIN,
        modifiers: Vec::new(),
    };
    for _ in 1..levels {
        chain = Modifier {
            unit_price: chain.unit_price,
            quantity: chain.quantity,
            modifiers: vec![chain],
        };
    }
    chain
}

/// A faulty discount in the basket's currency, unless the fault is the currency.
fn retarget(discount: Discount, currency: Currency) -> Discount {
    match discount {
        Discount::Amount(amount) if amount.currency() == Currency::USD => {
            Discount::Amount(Money::from_minor(amount.minor(), currency))
        }
        other => other,
    }
}

fn expected_discount_error(
    discount: Discount,
    currency: Currency,
    at: DiscountRef,
) -> PricingError {
    match discount {
        Discount::Amount(amount)
            if amount.currency() == Currency::EUR && currency != Currency::EUR =>
        {
            PricingError::CurrencyMismatch { expected: currency, found: Currency::EUR }
        }
        Discount::Amount(amount) if amount.currency() == Currency::EUR => {
            // In a basket in euros, a euro amount is valid unless negative; ours never is, so
            // this fault was retargeted into a valid discount by the caller's currency.
            PricingError::InvalidDiscount(at)
        }
        _ => PricingError::InvalidDiscount(at),
    }
}

/// A trace being replayed: each line's amounts as the steps so far give them.
struct Replay<'a> {
    basket: &'a Basket,
    rules: &'a Rules,
    gross: Vec<Option<Money>>,
    /// What is left of each line.
    rest: Vec<Money>,
    /// Whether each line's share was taken.
    shared: Vec<bool>,
    comp: Vec<Money>,
    discounts: Vec<Vec<Money>>,
    shares: Vec<Vec<Money>>,
    order_discounts: Vec<Money>,
    /// Each line's tax from each tax in the rules.
    taxes: Vec<Vec<Option<Money>>>,
    last_tax: Option<usize>,
    /// Which taxes the trace exempts, and which it rounds once for the document.
    exempted: Vec<bool>,
    documented: Vec<bool>,
}

impl<'a> Replay<'a> {
    fn new(basket: &'a Basket, rules: &'a Rules) -> Replay<'a> {
        let lines = basket.lines.len();
        let zero = Money::zero(basket.currency);
        Replay {
            basket,
            rules,
            gross: vec![None; lines],
            rest: vec![zero; lines],
            shared: vec![false; lines],
            comp: vec![zero; lines],
            discounts: vec![Vec::new(); lines],
            shares: vec![Vec::new(); lines],
            order_discounts: Vec::new(),
            taxes: vec![vec![None; rules.taxes.len()]; lines],
            last_tax: None,
            exempted: vec![false; rules.taxes.len()],
            documented: vec![false; rules.taxes.len()],
        }
    }

    /// Whether `tax` applies to `line`.
    fn covers(&self, tax: usize, line: usize) -> bool {
        let rule = &self.rules.taxes[tax];
        !self.basket.exemptions.contains(&rule.id)
            && rule.applies(self.basket.lines[line].tax_category, self.basket.dining)
    }

    /// Replays a step, checking its inputs against the basket, the rules and the steps before.
    fn step(&mut self, step: &Step, totals: &Totals) -> Result<(), TestCaseError> {
        let (basket, rules) = (self.basket, self.rules);
        match step {
            Step::Extended { line, item, modifiers, quantity, mode, gross } => {
                prop_assert_eq!(*item, basket.lines[*line].unit_price);
                prop_assert_eq!(*quantity, basket.lines[*line].quantity);
                prop_assert_eq!(*mode, rules.extension);
                let unit_price = item.checked_add(*modifiers).unwrap();
                prop_assert_eq!(unit_price, totals.lines[*line].unit_price);
                prop_assert!(self.gross[*line].is_none());
                self.gross[*line] = Some(*gross);
                self.rest[*line] = *gross;
            }
            Step::Shared { line, whole, weights, index, part } => {
                let share = basket.lines[*line].share.as_ref();
                prop_assert_eq!(
                    share.map(|share| (&share.weights, share.index)),
                    Some((weights, *index))
                );
                prop_assert_eq!(Some(*whole), self.gross[*line], "the share follows the extension");
                prop_assert!(!self.shared[*line] && self.comp[*line].is_zero());
                let total: BigInt = weights.iter().map(|&weight| BigInt::from(weight)).sum();
                prop_assert!(near_exact(*part, *whole, &BigInt::from(weights[*index]), &total));
                self.shared[*line] = true;
                self.gross[*line] = Some(*part);
                self.rest[*line] = *part;
            }
            Step::Comped { line, amount } => {
                prop_assert!(basket.lines[*line].comped);
                prop_assert_eq!(Some(*amount), self.gross[*line]);
                self.comp[*line] = *amount;
                self.rest[*line] = Money::zero(basket.currency);
            }
            Step::LineDiscounted { line, discount, value, base, mode, amount, limited } => {
                prop_assert_eq!(*discount, self.discounts[*line].len());
                prop_assert_eq!(*value, basket.lines[*line].discounts[*discount]);
                prop_assert_eq!(*base, self.rest[*line]);
                prop_assert_eq!(*mode, rules.discounts);
                prop_assert_eq!(*limited, exceeds(*value, *base));
                self.discounts[*line].push(*amount);
                self.rest[*line] = self.rest[*line].checked_sub(*amount).unwrap();
            }
            Step::OrderDiscounted { discount, value, base, mode, amount, limited, shares } => {
                let total = sum(self.rest.iter().copied());
                prop_assert_eq!(*discount, self.order_discounts.len());
                prop_assert_eq!(*value, basket.discounts[*discount]);
                prop_assert_eq!(minor(*base), total.clone());
                prop_assert_eq!(*mode, rules.discounts);
                prop_assert_eq!(*limited, exceeds(*value, *base));
                prop_assert_eq!(sum(shares.iter().copied()), minor(*amount));
                for (line, share) in shares.iter().enumerate() {
                    prop_assert!(near_exact_share(*share, *amount, self.rest[line], &total));
                    self.shares[line].push(*share);
                    self.rest[line] = self.rest[line].checked_sub(*share).unwrap();
                }
                self.order_discounts.push(*amount);
            }
            _ => self.tax_step(step)?,
        }
        Ok(())
    }

    fn tax_step(&mut self, step: &Step) -> Result<(), TestCaseError> {
        let rules = self.rules;
        match step {
            Step::TaxedLine { tax, line, rate, taxable, mode, amount } => {
                prop_assert!(self.last_tax <= Some(*tax), "taxes come in the rules' order");
                self.last_tax = Some(*tax);
                prop_assert_eq!(rules.tax_rounding.scope, TaxScope::Line);
                prop_assert_eq!(*rate, rules.taxes[*tax].rate);
                prop_assert_eq!(*mode, rules.tax_rounding.mode);
                prop_assert!(self.covers(*tax, *line));
                prop_assert_eq!(*taxable, self.rest[*line]);
                prop_assert!(self.taxes[*line][*tax].is_none());
                self.taxes[*line][*tax] = Some(*amount);
            }
            Step::TaxedDocument { tax, rate, taxable, mode, amount, shares } => {
                prop_assert!(self.last_tax < Some(*tax), "each tax is rounded once, in order");
                self.last_tax = Some(*tax);
                self.documented[*tax] = true;
                prop_assert_eq!(rules.tax_rounding.scope, TaxScope::Document);
                prop_assert_eq!(*rate, rules.taxes[*tax].rate);
                prop_assert_eq!(*mode, rules.tax_rounding.mode);
                let covered =
                    shares.iter().zip(&self.rest).filter_map(|(share, net)| share.map(|_| *net));
                prop_assert_eq!(minor(*taxable), sum(covered));
                prop_assert_eq!(sum(shares.iter().flatten().copied()), minor(*amount));
                for (line, share) in shares.iter().enumerate() {
                    prop_assert_eq!(share.is_some(), self.covers(*tax, line));
                    if let Some(share) = share {
                        let near =
                            near_exact_share(*share, *amount, self.rest[line], &minor(*taxable));
                        prop_assert!(near);
                    }
                    self.taxes[line][*tax] = *share;
                }
            }
            Step::Exempted { tax } => {
                prop_assert!(self.last_tax < Some(*tax), "each tax is exempted once, in order");
                self.last_tax = Some(*tax);
                prop_assert!(self.basket.exemptions.contains(&rules.taxes[*tax].id));
                self.exempted[*tax] = true;
            }
            other => prop_assert!(false, "a step the replay doesn't know: {:?}", other),
        }
        Ok(())
    }

    /// Checks that the replayed amounts are the totals'.
    fn finish(self, totals: &Totals) -> Result<(), TestCaseError> {
        for (line, amounts) in totals.lines.iter().enumerate() {
            prop_assert_eq!(self.shared[line], self.basket.lines[line].share.is_some());
            prop_assert_eq!(self.gross[line], Some(amounts.gross));
            prop_assert_eq!(self.comp[line], amounts.comp);
            prop_assert_eq!(&self.discounts[line], &amounts.discounts);
            prop_assert_eq!(&self.shares[line], &amounts.order_discounts);
            prop_assert_eq!(self.rest[line], amounts.net);
            let reported: Vec<Option<Money>> =
                amounts.taxes.iter().map(|tax| tax.map(|tax| tax.tax)).collect();
            prop_assert_eq!(&self.taxes[line], &reported);
        }
        prop_assert_eq!(&self.order_discounts, &totals.discounts);
        // Every tax is accounted for: an exempt one by its exemption, and one rounded per document
        // by its step, even when it applies to no line.
        let per_document = self.rules.tax_rounding.scope == TaxScope::Document;
        for (position, tax) in self.rules.taxes.iter().enumerate() {
            let exempt = self.basket.exemptions.contains(&tax.id);
            prop_assert_eq!(self.exempted[position], exempt, "tax {} exempt", position);
            let documented = per_document && !exempt;
            prop_assert_eq!(self.documented[position], documented, "tax {} rounded", position);
        }
        Ok(())
    }
}

/// Whether an amount discount asks for more than `base`.
fn exceeds(discount: Discount, base: Money) -> bool {
    matches!(discount, Discount::Amount(given) if given.minor() > base.minor())
}

proptest! {
    /// Every amount agrees with the exact oracle. When some amount doesn't fit in the engine's
    /// range, the engine reports an overflow, and only then.
    #[test]
    fn prices_match_the_exact_oracle((basket, rules) in any_case()) {
        let expected = expected(&basket, &rules);
        match price(&basket, &rules) {
            Ok(totals) => {
                prop_assert!(expected.representable());
                prop_assert_eq!(observed(&totals), expected);
            }
            Err(error) => {
                prop_assert_eq!(error, PricingError::Overflow);
                prop_assert!(!expected.representable());
            }
        }
    }

    /// The parts add up exactly, and nothing is negative.
    #[test]
    fn the_parts_add_up((basket, rules) in any_case()) {
        let Ok(totals) = price(&basket, &rules) else { return Ok(()) };
        for line in &totals.lines {
            let taken = minor(line.comp)
                + sum(line.discounts.iter().copied())
                + sum(line.order_discounts.iter().copied());
            prop_assert_eq!(minor(line.gross) - taken, minor(line.net));
            prop_assert_eq!(minor(line.tax), sum(line.taxes.iter().flatten().map(|tax| tax.tax)));
            prop_assert_eq!(minor(line.net) + minor(line.tax), minor(line.total));
            let all = [line.unit_price, line.gross, line.comp, line.net, line.tax, line.total];
            prop_assert!(all.iter().chain(&line.discounts).chain(&line.order_discounts).all(|amount| !amount.is_negative()));
            prop_assert!(line.taxes.iter().flatten().all(|tax| !tax.tax.is_negative() && tax.taxable == line.net));
        }
        for (position, amount) in totals.discounts.iter().enumerate() {
            prop_assert_eq!(sum(totals.lines.iter().map(|line| line.order_discounts[position])), minor(*amount));
        }
        for (position, tax) in totals.taxes.iter().enumerate() {
            let lines = totals.lines.iter().filter_map(|line| line.taxes[position]);
            let (taxable, amount): (Vec<Money>, Vec<Money>) = lines.map(|part| (part.taxable, part.tax)).unzip();
            prop_assert_eq!(sum(taxable), minor(tax.taxable));
            prop_assert_eq!(sum(amount), minor(tax.tax));
        }
        prop_assert_eq!(sum(totals.lines.iter().map(|line| line.gross)), minor(totals.gross));
        prop_assert_eq!(sum(totals.lines.iter().map(|line| line.net)), minor(totals.net));
        prop_assert_eq!(minor(totals.gross) - minor(totals.net), minor(totals.discounted));
        prop_assert_eq!(sum(totals.taxes.iter().map(|tax| tax.tax)), minor(totals.tax));
        prop_assert_eq!(minor(totals.net) + minor(totals.tax), minor(totals.total));
    }

    /// Replaying the trace gives every line's amounts, and each step's inputs are the basket's
    /// and the rules'.
    #[test]
    fn the_trace_explains_the_totals((basket, rules) in any_case()) {
        let Ok(totals) = price(&basket, &rules) else { return Ok(()) };
        let mut replay = Replay::new(&basket, &rules);
        for step in &totals.trace {
            replay.step(step, &totals)?;
        }
        replay.finish(&totals)?;
    }

    /// Reordering the lines doesn't change the order's gross, discounts or net. Without order
    /// discounts, whose leftover cents go to earlier lines, it doesn't change the tax either.
    #[test]
    fn the_order_of_lines_changes_no_totals((basket, rules) in any_case(), turn in any::<Index>()) {
        let Ok(totals) = price(&basket, &rules) else { return Ok(()) };
        let mut turned = basket.clone();
        let lines = turned.lines.len();
        if lines > 0 {
            turned.lines.rotate_left(turn.index(lines));
        }
        turned.lines.reverse();
        let other = price(&turned, &rules).unwrap();
        prop_assert_eq!((other.gross, other.discounted, other.net), (totals.gross, totals.discounted, totals.net));
        prop_assert_eq!(&other.discounts, &totals.discounts);
        if basket.discounts.is_empty() {
            prop_assert_eq!(&other.taxes, &totals.taxes);
            prop_assert_eq!(other.total, totals.total);
        }
    }

    /// Splitting a line among baskets conserves it: the parts' gross, and their comps, add up to
    /// the whole line's, and each part is within one minor unit of its exact share.
    #[test]
    fn the_parts_of_a_shared_line_add_up(
        (basket, rules) in any_case(),
        at in any::<Index>(),
        weights in prop::collection::vec(prop_oneof![4 => 1_u64..=3, 1 => 1_u64..1_000], 1..=5),
    ) {
        if basket.lines.is_empty() {
            return Ok(());
        }
        let line = at.index(basket.lines.len());
        let mut unshared = basket.clone();
        unshared.lines[line].share = None;
        let Ok(whole) = price(&unshared, &rules) else { return Ok(()) };
        let whole = &whole.lines[line];
        let total: BigInt = weights.iter().map(|&weight| BigInt::from(weight)).sum();
        let (mut gross, mut comp) = (BigInt::ZERO, BigInt::ZERO);
        for (index, &weight) in weights.iter().enumerate() {
            let mut shared = basket.clone();
            shared.lines[line].share = Some(Share { weights: weights.clone(), index });
            let totals = price(&shared, &rules).unwrap();
            let part = &totals.lines[line];
            prop_assert!(near_exact(part.gross, whole.gross, &BigInt::from(weight), &total));
            gross += minor(part.gross);
            comp += minor(part.comp);
        }
        prop_assert_eq!(gross, minor(whole.gross));
        prop_assert_eq!(comp, minor(whole.comp));
    }

    /// Each kind of invalid input is refused, with the error that names it.
    #[test]
    fn invalid_input_is_refused((basket, rules) in any_case(), fault in any_fault()) {
        let (mut basket, mut rules) = (basket, rules);
        let Some(error) = plant(&fault, &mut basket, &mut rules) else { return Ok(()) };
        prop_assert_eq!(price(&basket, &rules).unwrap_err(), error);
    }

    /// Modifiers nested exactly as deeply as allowed are priced: free ones change nothing.
    #[test]
    fn the_deepest_allowed_modifiers_are_priced((basket, rules) in any_case(), at in any::<Index>()) {
        if basket.lines.is_empty() {
            return Ok(());
        }
        let mut deepest = basket.clone();
        let line = at.index(deepest.lines.len());
        deepest.lines[line].modifiers.push(free_chain(MAX_MODIFIER_DEPTH, basket.currency));
        prop_assert_eq!(price(&deepest, &rules), price(&basket, &rules));
    }

    /// Cash rounding agrees with the exact oracle: the amount due is the amount rounded to a
    /// multiple of the increment, and the rounding is the difference.
    #[test]
    fn cash_rounding_matches_the_exact_oracle(
        cents in prop_oneof![-100_000_i64..100_000, any::<i64>()],
        increment in prop_oneof![prop::sample::select(vec![1_u32, 5, 10, 25, 50, 100]), 1_u32..1_000],
        mode in any_mode(),
    ) {
        let amount = Money::from_minor(cents, Currency::CAD);
        let rule = RoundingRule::new(mode, NonZeroU32::new(increment).unwrap());
        let due = round_rational(&BigInt::from(cents), &BigInt::from(increment), mode) * increment;
        match round_cash(amount, rule) {
            Ok(cash) => {
                prop_assert_eq!(minor(cash.due), due.clone());
                prop_assert_eq!(minor(cash.rounding), due - cents);
            }
            Err(error) => {
                prop_assert_eq!(error, PricingError::Overflow);
                prop_assert!(i64::try_from(&due).is_err());
            }
        }
    }
}
