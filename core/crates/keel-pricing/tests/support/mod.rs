//! Shared support for property tests: an exact oracle for pricing, and strategies for baskets.
//!
//! The oracle follows ADR-0014 step by step with arbitrary-precision integers, so nothing in it
//! can overflow or round by accident. It is built differently from the engine: amounts are plain
//! integers of minor units, rates are exact fractions, and it has its own rounding and its own
//! largest-remainder allocation, so the two are unlikely to share a mistake.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use core::cmp::Ordering;
use core::num::NonZeroU32;

use keel_pricing::{
    Basket, Dining, Discount, Line, Modifier, Rules, Share, Tax, TaxRounding, TaxScope,
};
use keel_types::{Currency, Decimal, Id, Money, Quantity, Rate, RoundingMode, Unit};
use num_bigint::BigInt;
use num_integer::Integer;
use proptest::prelude::*;

// ---------------------------------------------------------------------------------------------
// The oracle.

/// Rounds the rational `numerator / denominator` to an integer. `denominator` must be positive.
///
/// From the floor and the ceiling, as in `keel-types`' own oracle.
pub(crate) fn round_rational(
    numerator: &BigInt,
    denominator: &BigInt,
    mode: RoundingMode,
) -> BigInt {
    let (floor, remainder) = numerator.div_mod_floor(denominator);
    if remainder == BigInt::ZERO {
        return floor;
    }
    let ceiling = &floor + 1;
    let (toward_zero, away_from_zero) =
        if numerator > &BigInt::ZERO { (&floor, &ceiling) } else { (&ceiling, &floor) };
    let even = if floor.is_even() { &floor } else { &ceiling };
    let nearest = |tie: &BigInt| match (&remainder * 2_u32).cmp(denominator) {
        Ordering::Less => floor.clone(),
        Ordering::Equal => tie.clone(),
        Ordering::Greater => ceiling.clone(),
    };
    match mode {
        RoundingMode::HalfAwayFromZero => nearest(away_from_zero),
        RoundingMode::HalfEven => nearest(even),
        RoundingMode::HalfTowardZero => nearest(toward_zero),
        RoundingMode::AwayFromZero => away_from_zero.clone(),
        RoundingMode::TowardZero => toward_zero.clone(),
        RoundingMode::Ceiling => ceiling.clone(),
        RoundingMode::Floor => floor.clone(),
    }
}

/// A decimal as the exact fraction `numerator / denominator`.
fn fraction(value: Decimal) -> (BigInt, BigInt) {
    (BigInt::from(value.mantissa()), BigInt::from(10).pow(value.scale()))
}

/// `amount × value`, rounded with `mode`.
fn times(amount: &BigInt, value: Decimal, mode: RoundingMode) -> BigInt {
    let (numerator, denominator) = fraction(value);
    round_rational(&(amount * numerator), &denominator, mode)
}

/// Splits `amount` into parts proportional to `weights`: each part gets the floor of its exact
/// share, and the units left over go one each to the parts with the largest remainders, the
/// earliest first among equals. All the numbers are zero or more.
pub(crate) fn largest_remainder(amount: &BigInt, weights: &[BigInt]) -> Vec<BigInt> {
    let total: BigInt = weights.iter().sum();
    if *amount == BigInt::ZERO || total == BigInt::ZERO {
        return vec![BigInt::ZERO; weights.len()];
    }
    let mut parts: Vec<BigInt> = Vec::new();
    let mut remainders: Vec<(BigInt, usize)> = Vec::new();
    for (index, weight) in weights.iter().enumerate() {
        let (part, remainder) = (amount * weight).div_mod_floor(&total);
        parts.push(part);
        remainders.push((remainder, index));
    }
    let given: BigInt = parts.iter().sum();
    let left = amount - given;
    remainders.sort_by(|(a, i), (b, j)| b.cmp(a).then(i.cmp(j)));
    let mut remaining = left;
    for (_, index) in remainders {
        if remaining == BigInt::ZERO {
            break;
        }
        parts[index] += 1;
        remaining -= 1;
    }
    parts
}

/// What the oracle expects of one line, in minor units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpectedLine {
    pub(crate) unit_price: BigInt,
    /// The whole line's gross, before the basket takes its share of it.
    pub(crate) whole: BigInt,
    /// The basket's gross: the whole line's, or its share of it.
    pub(crate) gross: BigInt,
    pub(crate) comp: BigInt,
    pub(crate) discounts: Vec<BigInt>,
    pub(crate) order_discounts: Vec<BigInt>,
    pub(crate) net: BigInt,
    /// For each tax: `None` if it doesn't apply, else the taxable amount and the tax.
    pub(crate) taxes: Vec<Option<(BigInt, BigInt)>>,
    pub(crate) tax: BigInt,
    pub(crate) total: BigInt,
}

/// What the oracle expects of a basket, in minor units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Expected {
    pub(crate) lines: Vec<ExpectedLine>,
    pub(crate) discounts: Vec<BigInt>,
    /// For each tax: exempt, taxable amount, tax.
    pub(crate) taxes: Vec<(bool, BigInt, BigInt)>,
    pub(crate) gross: BigInt,
    pub(crate) discounted: BigInt,
    pub(crate) net: BigInt,
    pub(crate) tax: BigInt,
    pub(crate) total: BigInt,
}

impl Expected {
    /// Every amount the calculation defines, to check that they all fit in the engine's range.
    pub(crate) fn amounts(&self) -> Vec<&BigInt> {
        let mut all = vec![&self.gross, &self.discounted, &self.net, &self.tax, &self.total];
        all.extend(&self.discounts);
        for (_, taxable, tax) in &self.taxes {
            all.extend([taxable, tax]);
        }
        for line in &self.lines {
            all.extend([&line.unit_price, &line.whole, &line.gross, &line.comp, &line.net]);
            all.push(&line.tax);
            all.push(&line.total);
            all.extend(&line.discounts);
            all.extend(&line.order_discounts);
            for (taxable, tax) in line.taxes.iter().flatten() {
                all.extend([taxable, tax]);
            }
        }
        all
    }

    /// Whether every amount fits in an `i64`, as the engine's amounts must.
    pub(crate) fn representable(&self) -> bool {
        let (low, high) = (BigInt::from(i64::MIN), BigInt::from(i64::MAX));
        self.amounts().into_iter().all(|amount| (&low..=&high).contains(&amount))
    }
}

/// The price of `modifiers` for one of what they modify.
fn modifiers_price(modifiers: &[Modifier]) -> BigInt {
    modifiers
        .iter()
        .map(|modifier| {
            let own =
                BigInt::from(modifier.unit_price.minor()) + modifiers_price(&modifier.modifiers);
            own * modifier.quantity.get()
        })
        .sum()
}

/// A discount taken from `rest`.
fn take(rest: &BigInt, discount: Discount, mode: RoundingMode) -> BigInt {
    match discount {
        Discount::Percent(rate) => times(rest, rate.as_fraction(), mode),
        Discount::Amount(amount) => BigInt::from(amount.minor()).min(rest.clone()),
    }
}

/// A line's extension, its share, its comp and its own discounts.
fn expected_line(line: &Line, rules: &Rules) -> ExpectedLine {
    let unit_price = BigInt::from(line.unit_price.minor()) + modifiers_price(&line.modifiers);
    let whole = round_rational(
        &(&unit_price * line.quantity.micros()),
        &BigInt::from(1_000_000),
        rules.extension,
    );
    let gross = match &line.share {
        None => whole.clone(),
        Some(share) => {
            let weights: Vec<BigInt> =
                share.weights.iter().map(|&weight| BigInt::from(weight)).collect();
            largest_remainder(&whole, &weights)[share.index].clone()
        }
    };
    let comp = if line.comped { gross.clone() } else { BigInt::ZERO };
    let mut rest = &gross - &comp;
    let mut discounts = Vec::new();
    for &discount in &line.discounts {
        let taken = take(&rest, discount, rules.discounts);
        rest -= &taken;
        discounts.push(taken);
    }
    ExpectedLine {
        unit_price,
        whole,
        gross,
        comp,
        discounts,
        order_discounts: Vec::new(),
        net: rest,
        taxes: Vec::new(),
        tax: BigInt::ZERO,
        total: BigInt::ZERO,
    }
}

/// What ADR-0014 says a valid basket costs.
pub(crate) fn expected(basket: &Basket, rules: &Rules) -> Expected {
    let mut lines: Vec<ExpectedLine> =
        basket.lines.iter().map(|line| expected_line(line, rules)).collect();

    // Order discounts, each allocated by what is left of each line.
    let mut discounts = Vec::new();
    for &discount in &basket.discounts {
        let rests: Vec<BigInt> = lines.iter().map(|line| line.net.clone()).collect();
        let subtotal: BigInt = rests.iter().sum();
        let taken = take(&subtotal, discount, rules.discounts);
        for (line, share) in lines.iter_mut().zip(largest_remainder(&taken, &rests)) {
            line.net -= &share;
            line.order_discounts.push(share);
        }
        discounts.push(taken);
    }

    // Taxes.
    let mut taxes = Vec::new();
    for tax in &rules.taxes {
        if basket.exemptions.contains(&tax.id) {
            for line in &mut lines {
                line.taxes.push(None);
            }
            taxes.push((true, BigInt::ZERO, BigInt::ZERO));
            continue;
        }
        let covered: Vec<bool> = basket
            .lines
            .iter()
            .map(|line| {
                tax.categories.contains(&line.tax_category)
                    && tax.dining.is_none_or(|dining| dining == basket.dining)
            })
            .collect();
        let bases: Vec<BigInt> = lines
            .iter()
            .zip(&covered)
            .map(|(line, &covered)| if covered { line.net.clone() } else { BigInt::ZERO })
            .collect();
        let taxable: BigInt = bases.iter().sum();
        let rate = tax.rate.as_fraction();
        let mode = rules.tax_rounding.mode;
        let amounts: Vec<BigInt> = match rules.tax_rounding.scope {
            TaxScope::Line => bases.iter().map(|base| times(base, rate, mode)).collect(),
            TaxScope::Document => largest_remainder(&times(&taxable, rate, mode), &bases),
        };
        let total: BigInt = amounts.iter().zip(&covered).filter(|(_, c)| **c).map(|(a, _)| a).sum();
        for ((line, &covered), (base, amount)) in
            lines.iter_mut().zip(&covered).zip(bases.into_iter().zip(amounts))
        {
            line.taxes.push(covered.then_some((base, amount)));
        }
        taxes.push((false, taxable, total));
    }

    for line in &mut lines {
        line.tax = line.taxes.iter().flatten().map(|(_, tax)| tax).sum();
        line.total = &line.net + &line.tax;
    }
    let gross: BigInt = lines.iter().map(|line| &line.gross).sum();
    let net: BigInt = lines.iter().map(|line| &line.net).sum();
    let tax: BigInt = taxes.iter().map(|(_, _, tax)| tax).sum();
    Expected {
        discounted: &gross - &net,
        total: &net + &tax,
        lines,
        discounts,
        taxes,
        gross,
        net,
        tax,
    }
}

// ---------------------------------------------------------------------------------------------
// Strategies: small numbers that land on rounding ties and allocation ties, and the extremes.

/// Puts first among the order's discounts an amount of exactly what is left of the order after
/// the lines' own discounts, if that fits in an amount.
fn exactly_discount_the_order(basket: &mut Basket, rules: &Rules) {
    let before = Basket { discounts: Vec::new(), ..basket.clone() };
    let left: BigInt = expected(&before, rules).lines.iter().map(|line| &line.net).sum();
    if let Ok(left) = i64::try_from(left) {
        basket.discounts.insert(0, Discount::Amount(Money::from_minor(left, basket.currency)));
    }
}

/// A valid UUIDv7 ending in `n`.
pub(crate) fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

pub(crate) fn any_mode() -> impl Strategy<Value = RoundingMode> {
    prop::sample::select(RoundingMode::ALL.to_vec())
}

/// Currencies with 2, 0 and 3 decimal places.
pub(crate) fn any_currency() -> impl Strategy<Value = Currency> {
    prop_oneof![4 => Just(Currency::USD), 1 => Just(Currency::JPY), 1 => Just(Currency::KWD)]
}

/// A price in minor units: small ones mostly, where ties are common, and sometimes huge ones.
pub(crate) fn any_price() -> impl Strategy<Value = i64> {
    prop_oneof![
        10 => 0_i64..20,
        30 => 0_i64..2_000,
        8 => 0_i64..100_000_000,
        1 => prop::sample::select(vec![i64::MAX / 4, i64::MAX / 2, i64::MAX - 1, i64::MAX]),
    ]
}

/// A positive quantity: counts, and weights aimed at halves, quarters and eighths, which make
/// ties with odd prices.
pub(crate) fn any_quantity() -> impl Strategy<Value = Quantity> {
    prop_oneof![
        4 => (1_i64..=5).prop_map(|count| Quantity::from_whole(count, Unit::Each).unwrap()),
        2 => prop::sample::select(vec![500_000_i64, 250_000, 750_000, 125_000, 1_500_000, 5])
            .prop_map(|micros| Quantity::from_micros(micros, Unit::Kilogram)),
        2 => (1_i64..=5_000_000).prop_map(|micros| Quantity::from_micros(micros, Unit::Kilogram)),
    ]
}

/// A rate: round ones, which make ties, and arbitrary basis points and fine fractions.
pub(crate) fn any_rate(max_basis_points: i64) -> impl Strategy<Value = Rate> {
    let round = prop::sample::select(vec![0_i64, 250, 500, 1_000, 1_250, 2_500, 5_000, 10_000])
        .prop_filter("within range", move |&bp| bp <= max_basis_points);
    prop_oneof![
        3 => round.prop_map(Rate::from_basis_points),
        2 => prop::sample::select(vec!["8.875", "6.25", "10.25", "7", "33.33", "0.5"])
            .prop_map(|percent| Rate::from_percent(percent.parse().unwrap()).unwrap())
            .prop_filter("within range", move |rate| {
                rate.as_fraction() <= Decimal::new(max_basis_points, 4)
            }),
        2 => (0_i64..=max_basis_points).prop_map(Rate::from_basis_points),
        1 => (0_i64..=max_basis_points * 1_000)
            .prop_map(|micro_points| Rate::from_fraction(Decimal::new(micro_points, 7))),
    ]
}

/// A discount: a percentage from 0% to 100%, or an amount, which is often more than what it
/// applies to.
pub(crate) fn any_discount(currency: Currency) -> impl Strategy<Value = Discount> {
    prop_oneof![
        any_rate(10_000).prop_map(Discount::Percent),
        prop_oneof![0_i64..500, 0_i64..5_000]
            .prop_map(move |minor| Discount::Amount(Money::from_minor(minor, currency))),
    ]
}

fn any_modifier(currency: Currency, depth: u32) -> BoxedStrategy<Modifier> {
    let children = if depth == 0 {
        Just(Vec::new()).boxed()
    } else {
        prop::collection::vec(any_modifier(currency, depth - 1), 0..2).boxed()
    };
    (prop_oneof![0_i64..300, any_price()], 1_u32..=3, children)
        .prop_map(move |(minor, quantity, modifiers)| Modifier {
            unit_price: Money::from_minor(minor, currency),
            quantity: NonZeroU32::new(quantity).unwrap(),
            modifiers,
        })
        .boxed()
}

/// A share of a line: up to four parts, mostly of small weights, which tie often, and sometimes
/// of huge ones.
pub(crate) fn any_share() -> impl Strategy<Value = Share> {
    let weight = prop_oneof![
        8 => 1_u64..=3,
        1 => 1_u64..1_000_000,
        1 => prop::sample::select(vec![u64::MAX, u64::MAX / 3, 1_u64 << 40]),
    ];
    prop::collection::vec(weight, 1..=4).prop_flat_map(|weights| {
        let parts = weights.len();
        (Just(weights), 0..parts).prop_map(|(weights, index)| Share { weights, index })
    })
}

/// A line in one of three tax categories.
pub(crate) fn any_line(currency: Currency) -> impl Strategy<Value = Line> {
    prop_oneof![9 => any_rung_up_line(currency), 1 => any_exactly_discounted_line(currency)]
}

/// A line as it might be rung up.
fn any_rung_up_line(currency: Currency) -> impl Strategy<Value = Line> {
    (
        any_price(),
        prop::collection::vec(any_modifier(currency, 2), 0..3),
        any_quantity(),
        1_u64..=3,
        prop::bool::weighted(0.1),
        prop_oneof![6 => Just(0_usize), 3 => Just(1_usize), 1 => Just(2_usize)]
            .prop_flat_map(move |count| prop::collection::vec(any_discount(currency), count)),
        prop::option::weighted(0.2, any_share()),
    )
        .prop_map(move |(minor, modifiers, quantity, category, comped, discounts, share)| {
            Line {
                unit_price: Money::from_minor(minor, currency),
                modifiers,
                quantity,
                tax_category: id(category),
                comped,
                discounts,
                share,
            }
        })
}

/// One of an item, with an amount discount of exactly its price, then maybe more discounts: the
/// first takes all of it without being limited, and the rest find nothing left.
fn any_exactly_discounted_line(currency: Currency) -> impl Strategy<Value = Line> {
    (0_i64..2_000, 1_u64..=3, prop::collection::vec(any_discount(currency), 0..2)).prop_map(
        move |(minor, category, more)| {
            let price = Money::from_minor(minor, currency);
            let mut discounts = vec![Discount::Amount(price)];
            discounts.extend(more);
            Line {
                unit_price: price,
                modifiers: Vec::new(),
                quantity: Quantity::from_whole(1, Unit::Each).unwrap(),
                tax_category: id(category),
                comped: false,
                discounts,
                share: None,
            }
        },
    )
}

pub(crate) fn any_dining() -> impl Strategy<Value = Dining> {
    prop_oneof![Just(Dining::OnPremises), Just(Dining::ToGo)]
}

/// Up to three taxes over the three categories, some only for one way of dining. Most rates are
/// like sales taxes; some reach 100%, and a few exceed it, as some excise taxes do.
pub(crate) fn any_rules() -> impl Strategy<Value = Rules> {
    let tax = (
        prop_oneof![6 => any_rate(3_000), 2 => any_rate(10_000), 1 => any_rate(50_000)],
        prop::collection::btree_set(1_u64..=3, 0..=3),
        prop::option::weighted(0.2, any_dining()),
    );
    (any_mode(), any_mode(), prop::collection::vec(tax, 0..=3), any_mode(), any::<bool>()).prop_map(
        |(extension, discounts, taxes, mode, per_line)| Rules {
            extension,
            discounts,
            taxes: (0_u64..)
                .zip(taxes)
                .map(|(n, (rate, categories, dining))| Tax {
                    id: id(0x100 + n),
                    name: format!("tax {n}"),
                    rate,
                    categories: categories.into_iter().map(id).collect(),
                    dining,
                })
                .collect(),
            tax_rounding: TaxRounding {
                scope: if per_line { TaxScope::Line } else { TaxScope::Document },
                mode,
            },
        },
    )
}

/// A basket, and rules to price it by.
pub(crate) fn any_case() -> impl Strategy<Value = (Basket, Rules)> {
    (any_currency(), any_rules()).prop_flat_map(|(currency, rules)| {
        let taxes = rules.taxes.len();
        (
            prop_oneof![1 => Just(0_usize), 9 => 1_usize..6]
                .prop_flat_map(move |count| prop::collection::vec(any_line(currency), count)),
            // Repeated lines, as in "three flat whites", have equal weights when a discount or a
            // tax is allocated, which puts the tie-breaking rule to work.
            prop::option::weighted(0.4, (any::<prop::sample::Index>(), 1_usize..=2)),
            prop_oneof![5 => Just(0_usize), 4 => Just(1_usize), 1 => Just(2_usize)]
                .prop_flat_map(move |count| prop::collection::vec(any_discount(currency), count)),
            any_dining(),
            prop::collection::vec(0..taxes.max(1), 0..=1),
            prop::bool::weighted(0.1),
            Just(rules),
        )
            .prop_map(
                move |(mut lines, repeat, discounts, dining, exempt, exact, rules)| {
                    if let Some((at, copies)) = repeat.filter(|_| !lines.is_empty()) {
                        let line = lines[at.index(lines.len())].clone();
                        lines.extend(core::iter::repeat_n(line, copies));
                    }
                    let exemptions = exempt
                        .into_iter()
                        .filter_map(|index| rules.taxes.get(index).map(|tax| tax.id))
                        .collect();
                    let mut basket = Basket { currency, lines, discounts, dining, exemptions };
                    if exact {
                        exactly_discount_the_order(&mut basket, &rules);
                    }
                    (basket, rules)
                },
            )
    })
}
