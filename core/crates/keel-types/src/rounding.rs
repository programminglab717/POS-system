//! Rounding: the only place where amounts lose precision, and always on purpose.
//!
//! Keel rounds in exactly three places (see `docs/architecture/domain-model.md` §4):
//! - tax calculation;
//! - allocating order-level amounts to lines;
//! - cash tender rounding.
//!
//! Each place names its [`RoundingMode`] or [`RoundingRule`] explicitly; there is no default.
//!
//! Every function here rounds the exact value exactly once. Intermediate results are never
//! rounded: a product too large for a [`Decimal`] is computed with 192-bit integers instead.

use core::cmp::Ordering;
use core::num::NonZeroU32;

use rust_decimal::{Decimal, RoundingStrategy};

/// How to round a value that lies between two representable values.
///
/// The examples round to whole units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoundingMode {
    /// To the nearest; ties away from zero. 2.5 → 3, −2.5 → −3. The usual commercial rule
    /// ("round half up" in most tax legislation).
    HalfAwayFromZero,
    /// To the nearest; ties to the even neighbour ("banker's rounding"). 2.5 → 2, 3.5 → 4.
    HalfEven,
    /// To the nearest; ties toward zero. 2.5 → 2, −2.5 → −2.
    HalfTowardZero,
    /// Away from zero ("round up" in magnitude). 2.1 → 3, −2.1 → −3.
    AwayFromZero,
    /// Toward zero ("truncate", "round down" in magnitude). 2.9 → 2, −2.9 → −2.
    TowardZero,
    /// Toward positive infinity. 2.1 → 3, −2.9 → −2.
    Ceiling,
    /// Toward negative infinity. 2.9 → 2, −2.1 → −3.
    Floor,
}

impl RoundingMode {
    /// Every rounding mode, for exhaustive tests and configuration UIs.
    pub const ALL: [RoundingMode; 7] = [
        RoundingMode::HalfAwayFromZero,
        RoundingMode::HalfEven,
        RoundingMode::HalfTowardZero,
        RoundingMode::AwayFromZero,
        RoundingMode::TowardZero,
        RoundingMode::Ceiling,
        RoundingMode::Floor,
    ];

    const fn strategy(self) -> RoundingStrategy {
        match self {
            RoundingMode::HalfAwayFromZero => RoundingStrategy::MidpointAwayFromZero,
            RoundingMode::HalfEven => RoundingStrategy::MidpointNearestEven,
            RoundingMode::HalfTowardZero => RoundingStrategy::MidpointTowardZero,
            RoundingMode::AwayFromZero => RoundingStrategy::AwayFromZero,
            RoundingMode::TowardZero => RoundingStrategy::ToZero,
            RoundingMode::Ceiling => RoundingStrategy::ToPositiveInfinity,
            RoundingMode::Floor => RoundingStrategy::ToNegativeInfinity,
        }
    }

    /// Rounds `value` to `decimal_places` decimal places.
    ///
    /// Values that already have `decimal_places` or fewer decimal places are returned unchanged.
    pub fn round(self, value: Decimal, decimal_places: u32) -> Decimal {
        value.round_dp_with_strategy(decimal_places, self.strategy())
    }

    /// Rounds an integer to a multiple of `increment`, for example an amount in cents to the
    /// nearest 5 cents.
    ///
    /// Returns `None` only if the rounded value doesn't fit in an `i64`.
    pub fn round_to_increment(self, value: i64, increment: NonZeroU32) -> Option<i64> {
        let negative = value < 0;
        let increment = u128::from(increment.get());
        let magnitude = u128::from(value.unsigned_abs());
        let truncated = magnitude.checked_div(increment)?;
        let discarded = Discarded::of(magnitude.checked_rem(increment)?, increment)?;
        let units = self.round_magnitude(negative, truncated, discarded)?;
        to_signed(negative, units.checked_mul(increment)?)
    }

    /// Rounds the exact product `value × factor` to an integer.
    ///
    /// The product is computed exactly, however many digits it has, and rounded once.
    /// Multiplying `Decimal`s instead would silently round any product needing more than 96 bits,
    /// and rounding that again can be off by one: 1007 × 0.9935451837140019860973187686 is
    /// 1000.4999…, but as a `Decimal` product it becomes 1000.5, which rounds half away from zero
    /// to 1001 instead of 1000.
    ///
    /// Returns `None` only if the result doesn't fit in an `i64`.
    pub(crate) fn round_product(self, value: i64, factor: Decimal) -> Option<i64> {
        // factor = mantissa / 10^scale. The scale is at most 28, so 10^scale is the product of
        // two powers of ten that each fit in a u64: 10^19 and 10^9 at most.
        let scale = factor.scale();
        let low_digits = scale.min(19);
        let divisors =
            [10_u64.checked_pow(low_digits)?, 10_u64.checked_pow(scale.checked_sub(low_digits)?)?];
        let magnitude = factor.mantissa().unsigned_abs();
        self.round_ratio(value, factor.is_sign_negative(), magnitude, divisors)
    }

    /// Rounds the exact value `value × numerator / (divisors[0] × divisors[1])` to an integer,
    /// negated if `negative_ratio`.
    ///
    /// Every intermediate value is exact: the numerator may need up to 192 bits.
    ///
    /// Returns `None` if a divisor is zero or the result doesn't fit in an `i64`.
    pub(crate) fn round_ratio(
        self,
        value: i64,
        negative_ratio: bool,
        numerator: u128,
        divisors: [u64; 2],
    ) -> Option<i64> {
        let negative = (value < 0) != negative_ratio;
        let product = widening_mul(value.unsigned_abs(), numerator)?;
        let [first, second] = divisors;
        let (quotient, first_remainder) = div_rem(product, first)?;
        let (quotient, second_remainder) = div_rem(quotient, second)?;

        // Division discarded `second_remainder × first + first_remainder` out of
        // `first × second`. Both fit in a u128, since each divisor is below 2^64.
        let remainder = u128::from(second_remainder)
            .checked_mul(u128::from(first))?
            .checked_add(u128::from(first_remainder))?;
        let divisor = u128::from(first).checked_mul(u128::from(second))?;
        let discarded = Discarded::of(remainder, divisor)?;

        // A quotient needing more than 64 bits is out of range whichever way it rounds.
        let [truncated, 0, 0] = quotient else {
            return None;
        };
        let magnitude = self.round_magnitude(negative, u128::from(truncated), discarded)?;
        to_signed(negative, magnitude)
    }

    /// Rounds a value given as its sign, its magnitude truncated toward zero, and what the
    /// truncation discarded. Returns the rounded magnitude.
    fn round_magnitude(
        self,
        negative: bool,
        truncated: u128,
        discarded: Discarded,
    ) -> Option<u128> {
        let truncated_is_odd = truncated.checked_rem(2)? == 1;
        let away_from_zero = discarded != Discarded::Nothing
            && match self {
                RoundingMode::HalfAwayFromZero => discarded >= Discarded::Half,
                RoundingMode::HalfEven => {
                    discarded > Discarded::Half
                        || (discarded == Discarded::Half && truncated_is_odd)
                }
                RoundingMode::HalfTowardZero => discarded > Discarded::Half,
                RoundingMode::AwayFromZero => true,
                RoundingMode::TowardZero => false,
                RoundingMode::Ceiling => !negative,
                RoundingMode::Floor => negative,
            };
        if away_from_zero { truncated.checked_add(1) } else { Some(truncated) }
    }
}

/// What truncating a value toward zero discarded, compared with half a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Discarded {
    Nothing,
    BelowHalf,
    Half,
    AboveHalf,
}

impl Discarded {
    /// Classifies the fraction `remainder / divisor`, where `remainder < divisor`.
    fn of(remainder: u128, divisor: u128) -> Option<Discarded> {
        if remainder == 0 {
            return Some(Discarded::Nothing);
        }
        // Compare the remainder with the distance to the next multiple, rather than doubling
        // it: that can't overflow, whatever the divisor.
        Some(match remainder.cmp(&divisor.checked_sub(remainder)?) {
            Ordering::Less => Discarded::BelowHalf,
            Ordering::Equal => Discarded::Half,
            Ordering::Greater => Discarded::AboveHalf,
        })
    }
}

/// Applies a sign to a magnitude, if the result fits in an `i64`.
fn to_signed(negative: bool, magnitude: u128) -> Option<i64> {
    let magnitude = i128::try_from(magnitude).ok()?;
    i64::try_from(if negative { magnitude.checked_neg()? } else { magnitude }).ok()
}

/// `a × b` as a 192-bit number: three 64-bit limbs, least significant first.
fn widening_mul(a: u64, b: u128) -> Option<[u64; 3]> {
    let [b_low, b_high] = split(b)?;
    let low = u128::from(a).checked_mul(u128::from(b_low))?;
    let [limb_0, carry] = split(low)?;
    // At most (2^64 − 1)² + 2^64 − 1 < 2^128: no overflow.
    let high = u128::from(a).checked_mul(u128::from(b_high))?.checked_add(u128::from(carry))?;
    let [limb_1, limb_2] = split(high)?;
    Some([limb_0, limb_1, limb_2])
}

/// Divides a 192-bit number by `divisor`, returning the quotient and the remainder.
fn div_rem(dividend: [u64; 3], divisor: u64) -> Option<([u64; 3], u64)> {
    let divisor = u128::from(divisor);
    let mut quotient = [0; 3];
    let mut remainder: u128 = 0;
    for (quotient_limb, dividend_limb) in quotient.iter_mut().zip(dividend).rev() {
        // `remainder < divisor < 2^64`, so the shift loses nothing.
        let current = remainder.checked_shl(64)? | u128::from(dividend_limb);
        *quotient_limb = u64::try_from(current.checked_div(divisor)?).ok()?;
        remainder = current.checked_rem(divisor)?;
    }
    Some((quotient, u64::try_from(remainder).ok()?))
}

/// Splits a `u128` into its low and high 64-bit halves.
fn split(value: u128) -> Option<[u64; 2]> {
    let low = u64::try_from(value & u128::from(u64::MAX)).ok()?;
    let high = u64::try_from(value.checked_shr(64)?).ok()?;
    Some([low, high])
}

/// A rounding rule for amounts: a [`RoundingMode`] and an increment in minor units.
///
/// - Rounding to the currency's minor unit (a cent) uses increment 1.
/// - Cash rounding to the nearest 5 cents (Canada, Australia, Switzerland) uses increment 5.
/// - Cash rounding to the nearest 10 cents (New Zealand) uses increment 10.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoundingRule {
    mode: RoundingMode,
    increment: NonZeroU32,
}

impl RoundingRule {
    /// A rule rounding to multiples of `increment` minor units.
    pub const fn new(mode: RoundingMode, increment: NonZeroU32) -> Self {
        Self { mode, increment }
    }

    /// A rule rounding to the currency's minor unit.
    pub const fn to_minor_unit(mode: RoundingMode) -> Self {
        Self { mode, increment: NonZeroU32::MIN }
    }

    /// The rounding mode.
    pub const fn mode(self) -> RoundingMode {
        self.mode
    }

    /// The increment, in minor units.
    pub const fn increment(self) -> NonZeroU32 {
        self.increment
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigUint;
    use proptest::prelude::*;

    use super::*;

    fn increment(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn dec(text: &str) -> Decimal {
        text.parse().unwrap()
    }

    fn limbs_to_big(limbs: [u64; 3]) -> BigUint {
        limbs.iter().rev().fold(BigUint::ZERO, |acc, &limb| (acc << 64_u32) + limb)
    }

    /// Rounding to whole units, for every mode, over the classic reference values.
    #[test]
    fn rounding_reference_table() {
        use RoundingMode::{
            AwayFromZero, Ceiling, Floor, HalfAwayFromZero, HalfEven, HalfTowardZero, TowardZero,
        };
        // value, then expected result per mode in RoundingMode::ALL order.
        let table: [(&str, [i64; 7]); 10] = [
            ("5.5", [6, 6, 5, 6, 5, 6, 5]),
            ("2.5", [3, 2, 2, 3, 2, 3, 2]),
            ("1.6", [2, 2, 2, 2, 1, 2, 1]),
            ("1.1", [1, 1, 1, 2, 1, 2, 1]),
            ("1.0", [1, 1, 1, 1, 1, 1, 1]),
            ("-1.0", [-1, -1, -1, -1, -1, -1, -1]),
            ("-1.1", [-1, -1, -1, -2, -1, -1, -2]),
            ("-1.6", [-2, -2, -2, -2, -1, -1, -2]),
            ("-2.5", [-3, -2, -2, -3, -2, -2, -3]),
            ("-5.5", [-6, -6, -5, -6, -5, -5, -6]),
        ];
        assert_eq!(
            RoundingMode::ALL,
            [HalfAwayFromZero, HalfEven, HalfTowardZero, AwayFromZero, TowardZero, Ceiling, Floor]
        );
        for (text, expected) in table {
            let value = dec(text);
            let tenths = i64::try_from(value.mantissa()).unwrap();
            for (mode, want) in RoundingMode::ALL.into_iter().zip(expected) {
                // The same table drives all three rounding paths.
                assert_eq!(mode.round(value, 0), Decimal::from(want), "{mode:?} round({text})");
                assert_eq!(
                    mode.round_to_increment(tenths, increment(10)),
                    want.checked_mul(10),
                    "{mode:?} round_to_increment({text})"
                );
                assert_eq!(
                    mode.round_product(tenths, dec("0.1")),
                    Some(want),
                    "{mode:?} round_product({text})"
                );
            }
        }
    }

    #[test]
    fn increment_rounding_matches_reference_table() {
        // Cents to the nearest 5 cents, as in Canadian cash rounding: 1–2 round down, 3–4 up.
        let nickel = increment(5);
        let half_up = RoundingMode::HalfAwayFromZero;
        for (cents, expected) in [(100, 100), (101, 100), (102, 100), (103, 105), (104, 105)] {
            assert_eq!(half_up.round_to_increment(cents, nickel), Some(expected), "{cents}");
        }
        for (cents, expected) in [(106, 105), (107, 105), (108, 110), (109, 110), (-103, -105)] {
            assert_eq!(half_up.round_to_increment(cents, nickel), Some(expected), "{cents}");
        }
        // Exact ties only exist for even increments: 0.05 is halfway between 0.00 and 0.10.
        let dime = increment(10);
        let ties = [
            (RoundingMode::HalfAwayFromZero, [(5, 10), (15, 20), (-5, -10), (-15, -20)]),
            (RoundingMode::HalfTowardZero, [(5, 0), (15, 10), (-5, 0), (-15, -10)]),
            (RoundingMode::HalfEven, [(5, 0), (15, 20), (-5, 0), (-15, -20)]),
        ];
        for (mode, cases) in ties {
            for (value, expected) in cases {
                assert_eq!(
                    mode.round_to_increment(value, dime),
                    Some(expected),
                    "{mode:?} {value}"
                );
            }
        }
    }

    #[test]
    fn directed_increment_rounding() {
        let five = increment(5);
        let cases = [
            (RoundingMode::AwayFromZero, 101, 105),
            (RoundingMode::AwayFromZero, -101, -105),
            (RoundingMode::TowardZero, 104, 100),
            (RoundingMode::TowardZero, -104, -100),
            (RoundingMode::Ceiling, -104, -100),
            (RoundingMode::Floor, -101, -105),
        ];
        for (mode, value, expected) in cases {
            assert_eq!(mode.round_to_increment(value, five), Some(expected), "{mode:?} {value}");
        }
    }

    #[test]
    fn increment_rounding_reports_overflow_instead_of_panicking() {
        let five = increment(5);
        assert_eq!(RoundingMode::Ceiling.round_to_increment(i64::MAX, five), None);
        assert_eq!(RoundingMode::Floor.round_to_increment(i64::MIN, five), None);
        assert_eq!(
            RoundingMode::Floor.round_to_increment(i64::MAX, five),
            Some(9_223_372_036_854_775_805)
        );
        assert_eq!(
            RoundingMode::Ceiling.round_to_increment(i64::MIN, increment(1)),
            Some(i64::MIN)
        );
        // 2^63 / (2^32 − 1) is just over 2^31 + ½, so it rounds up, past the i64 range.
        let largest = increment(u32::MAX);
        assert_eq!(RoundingMode::HalfEven.round_to_increment(i64::MIN, largest), None);
        // The i64 range is asymmetric, so mirror-image values can land on opposite sides of it.
        let away = RoundingMode::AwayFromZero;
        assert_eq!(away.round_to_increment(i64::MAX, increment(2)), None);
        assert_eq!(away.round_to_increment(i64::MIN + 1, increment(2)), Some(i64::MIN));
    }

    /// Products too large for a `Decimal`: rounding them as `Decimal`s first would be off by one.
    #[test]
    fn products_are_rounded_once() {
        let cases = [
            // 1007 × f = 1000.4999…: rounds down. As a Decimal product: 1000.5, rounded to 1001.
            (1007, "0.9935451837140019860973187686", RoundingMode::HalfAwayFromZero, 1000),
            (-1007, "0.9935451837140019860973187686", RoundingMode::HalfAwayFromZero, -1000),
            // 1007 × f = 5000.99999…: truncates to 5000. As a Decimal product: 5001.
            (1007, "4.9662363455809334657398212512", RoundingMode::TowardZero, 5000),
            (1007, "4.9662363455809334657398212512", RoundingMode::Floor, 5000),
            (1007, "4.9662363455809334657398212512", RoundingMode::Ceiling, 5001),
        ];
        for (value, factor, mode, expected) in cases {
            assert_eq!(
                mode.round_product(value, dec(factor)),
                Some(expected),
                "{value} × {factor}"
            );
        }
    }

    #[test]
    fn product_edge_cases() {
        let half_even = RoundingMode::HalfEven;
        assert_eq!(half_even.round_product(1, dec("0.5")), Some(0));
        assert_eq!(half_even.round_product(3, dec("0.5")), Some(2));
        assert_eq!(half_even.round_product(-3, dec("0.5")), Some(-2));
        assert_eq!(half_even.round_product(-3, dec("-0.5")), Some(2));
        assert_eq!(half_even.round_product(1234, Decimal::ZERO), Some(0));
        assert_eq!(RoundingMode::Floor.round_product(-1234, -Decimal::ZERO), Some(0));
        assert_eq!(half_even.round_product(i64::MIN, Decimal::ONE), Some(i64::MIN));
        assert_eq!(half_even.round_product(i64::MIN, Decimal::NEGATIVE_ONE), None);
        assert_eq!(half_even.round_product(i64::MAX, Decimal::MAX), None);
        // i64::MAX × (1 + 10^-28) is a hair above i64::MAX: nearest fits, ceiling doesn't.
        let a_hair_above_one = dec("1.0000000000000000000000000001");
        assert_eq!(half_even.round_product(i64::MAX, a_hair_above_one), Some(i64::MAX));
        assert_eq!(RoundingMode::Ceiling.round_product(i64::MAX, a_hair_above_one), None);
        // The smallest factor: 10^-28.
        let tiny = dec("0.0000000000000000000000000001");
        assert_eq!(RoundingMode::Ceiling.round_product(i64::MAX, tiny), Some(1));
        assert_eq!(RoundingMode::Floor.round_product(i64::MIN, tiny), Some(-1));
        assert_eq!(half_even.round_product(i64::MAX, tiny), Some(0));
        // A 156-bit product: 10^18 × (2^96 − 1) × 10^-28 = 7922816251426433759.354…
        let widest = dec("7.9228162514264337593543950335");
        let quintillion = 1_000_000_000_000_000_000;
        assert_eq!(half_even.round_product(quintillion, widest), Some(7_922_816_251_426_433_759));
        assert_eq!(
            RoundingMode::Ceiling.round_product(quintillion, widest),
            Some(7_922_816_251_426_433_760)
        );
    }

    #[test]
    fn wide_arithmetic_known_answers() {
        // (2^64 − 1)(2^128 − 1) = 2^192 − 2^128 − 2^64 + 1.
        let largest_product = [1, u64::MAX, u64::MAX - 1];
        assert_eq!(widening_mul(u64::MAX, u128::MAX), Some(largest_product));
        assert_eq!(widening_mul(0, u128::MAX), Some([0, 0, 0]));
        assert_eq!(widening_mul(2, 1 << 127), Some([0, 0, 1]));
        // … and back: dividing by 2^64 − 1 leaves 2^128 − 1.
        assert_eq!(div_rem(largest_product, u64::MAX), Some(([u64::MAX, u64::MAX, 0], 0)));
        assert_eq!(div_rem([7, 0, 0], 10), Some(([0, 0, 0], 7)));
        assert_eq!(div_rem([0, 0, 1], 1), Some(([0, 0, 1], 0)));
        assert_eq!(div_rem([1, 2, 3], 0), None);
    }

    proptest! {
        #[test]
        fn widening_mul_is_exact(a in any::<u64>(), b in any::<u128>()) {
            let product = widening_mul(a, b).unwrap();
            prop_assert_eq!(limbs_to_big(product), BigUint::from(a) * BigUint::from(b));
        }

        #[test]
        fn div_rem_is_exact(dividend in any::<[u64; 3]>(), divisor in 1..=u64::MAX) {
            let (quotient, remainder) = div_rem(dividend, divisor).unwrap();
            prop_assert!(remainder < divisor);
            prop_assert_eq!(
                limbs_to_big(quotient) * BigUint::from(divisor) + BigUint::from(remainder),
                limbs_to_big(dividend)
            );
        }
    }

    #[test]
    fn rule_constructors() {
        let rule = RoundingRule::to_minor_unit(RoundingMode::HalfEven);
        assert_eq!(rule.increment().get(), 1);
        assert_eq!(rule.mode(), RoundingMode::HalfEven);
        let cash = RoundingRule::new(RoundingMode::HalfAwayFromZero, increment(5));
        assert_eq!(cash.increment().get(), 5);
    }
}
