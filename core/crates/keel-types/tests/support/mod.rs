//! Shared support for property tests: an exact oracle and input strategies.
//!
//! The oracle rounds from the definitions, with arbitrary-precision integers. It is deliberately
//! built differently from Keel's implementation (from the floor and ceiling rather than the sign
//! and magnitude), so the two are unlikely to share a mistake.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use core::cmp::Ordering;

use keel_types::{Currency, Decimal, RoundingMode};
use num_bigint::BigInt;
use num_integer::Integer;
use proptest::prelude::*;

/// The largest `Decimal` mantissa: 2^96 − 1.
pub(crate) const MAX_MANTISSA: i128 = 79_228_162_514_264_337_593_543_950_335;

/// Rounds the rational `numerator / denominator` to an integer. `denominator` must be positive.
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
    // `remainder` is the distance to the floor; the ceiling is `denominator − remainder` away.
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

/// 10^exponent.
pub(crate) fn pow10(exponent: u32) -> BigInt {
    BigInt::from(10).pow(exponent)
}

/// Amounts in minor units: the whole range, everyday values, and the edges.
pub(crate) fn any_minor() -> impl Strategy<Value = i64> {
    prop_oneof![
        3 => any::<i64>(),
        3 => -1_000_000_i64..=1_000_000,
        1 => prop::sample::select(vec![i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX]),
    ]
}

/// Amounts whose negation is also an amount: everything but `i64::MIN`.
pub(crate) fn negatable_minor() -> impl Strategy<Value = i64> {
    any_minor().prop_filter("negatable", |&minor| minor != i64::MIN)
}

/// Any `Decimal`: every scale, long and short mantissas, and the edges.
pub(crate) fn any_decimal() -> impl Strategy<Value = Decimal> {
    let mantissa = prop_oneof![
        3 => -MAX_MANTISSA..=MAX_MANTISSA,
        3 => -1_000_000_i128..=1_000_000,
        1 => prop::sample::select(vec![-MAX_MANTISSA, -1, 0, 1, MAX_MANTISSA]),
    ];
    (mantissa, 0_u32..=28)
        .prop_map(|(mantissa, scale)| Decimal::from_i128_with_scale(mantissa, scale))
}

/// Decimals of at most ±10 with up to 28 decimal places. As factors, they keep most products
/// with an amount in range while needing up to 156 bits to compute exactly.
pub(crate) fn moderate_decimal() -> impl Strategy<Value = Decimal> {
    (0_u32..=28).prop_flat_map(|scale| {
        let bound = 10_i128.pow(scale + 1).min(MAX_MANTISSA);
        (-bound..=bound).prop_map(move |mantissa| Decimal::from_i128_with_scale(mantissa, scale))
    })
}

/// Amount and factor pairs whose exact product lies on a rounding boundary (a whole or half
/// minor unit) or a hair either side of one, while needing up to 126 bits to compute.
///
/// This is exactly where rounding an intermediate result pushes the answer across the boundary.
/// Uniformly random inputs essentially never land this close, so without this generator a
/// double-rounding bug passes thousands of cases unnoticed.
pub(crate) fn near_boundary_product() -> impl Strategy<Value = (i64, Decimal)> {
    let magnitudes = (1_i64..=1_000_000_000, 0..=MAX_MANTISSA, 20_u32..=28);
    let signs_and_side = (any::<bool>(), any::<bool>(), any::<bool>());
    (magnitudes, signs_and_side).prop_filter_map(
        "factor out of range",
        |((minor, mantissa, scale), (above, negative_minor, negative_factor))| {
            let divisor = BigInt::from(minor);
            // The boundary nearest the random product, as a multiple of half of 10^scale.
            let half_unit = pow10(scale) / 2_u32;
            let product = &divisor * BigInt::from(mantissa);
            let boundary =
                round_rational(&product, &half_unit, RoundingMode::HalfEven) * &half_unit;
            // The mantissa putting the product at (or just below) the boundary, or just above it.
            let (at_or_below, _) = boundary.div_mod_floor(&divisor);
            let mantissa = if above { at_or_below + 1_u32 } else { at_or_below };
            let mantissa = i128::try_from(&mantissa).ok().filter(|&m| m <= MAX_MANTISSA)?;
            let minor = if negative_minor { -minor } else { minor };
            let mantissa = if negative_factor { -mantissa } else { mantissa };
            Some((minor, Decimal::from_i128_with_scale(mantissa, scale)))
        },
    )
}

/// Decimals exactly on a rounding tie, or one unit in the last place either side of it,
/// together with the number of decimal places at which that tie occurs.
pub(crate) fn near_tie_decimal() -> impl Strategy<Value = (Decimal, u32)> {
    (1_u32..=28)
        .prop_flat_map(|scale| (Just(scale), 0..scale))
        .prop_flat_map(|(scale, places)| {
            // Rounding to `places` works in steps of 10^(scale − places) mantissa units.
            let step = 10_i128.pow(scale - places);
            let whole_steps = 0..MAX_MANTISSA / step;
            (Just(scale), Just(places), whole_steps, any::<bool>(), -1_i128..=1)
        })
        .prop_map(|(scale, places, whole_steps, negative, nudge)| {
            let step = 10_i128.pow(scale - places);
            let tie = whole_steps * step + step / 2;
            let mantissa = if negative { -tie } else { tie } + nudge;
            (Decimal::from_i128_with_scale(mantissa, scale), places)
        })
}

/// Any rounding mode.
pub(crate) fn any_mode() -> impl Strategy<Value = RoundingMode> {
    prop::sample::select(RoundingMode::ALL.to_vec())
}

/// Any currency in the table.
pub(crate) fn any_currency() -> impl Strategy<Value = Currency> {
    prop::sample::select(Currency::known().collect::<Vec<_>>())
}
