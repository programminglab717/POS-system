//! Property tests: every rounding path agrees with exact rational arithmetic, in every mode.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU32;

use keel_types::{Decimal, RoundingMode};
use num_bigint::BigInt;
use proptest::prelude::*;
use support::{
    any_decimal, any_minor, any_mode, near_tie_decimal, negatable_minor, pow10, round_rational,
};

/// Increments: everyday cash increments, and anything else a `u32` allows.
fn any_increment() -> impl Strategy<Value = NonZeroU32> {
    prop_oneof![
        3 => prop::sample::select(vec![1_u32, 2, 5, 10, 20, 25, 50, 100, 500, 1000]),
        2 => 1_u32..=1_000_000,
        1 => 1_u32..=u32::MAX,
    ]
    .prop_map(|increment| NonZeroU32::new(increment).unwrap())
}

/// Modes whose results for `-x` are the negation of their results for `x`.
const SYMMETRIC_MODES: [RoundingMode; 5] = [
    RoundingMode::HalfAwayFromZero,
    RoundingMode::HalfEven,
    RoundingMode::HalfTowardZero,
    RoundingMode::AwayFromZero,
    RoundingMode::TowardZero,
];

proptest! {
    /// `round_to_increment(v, n)` is `n × round(v / n)`, or `None` when that is out of range.
    #[test]
    fn increment_rounding_is_exact(
        value in any_minor(),
        increment in any_increment(),
        mode in any_mode(),
    ) {
        let step = BigInt::from(increment.get());
        let expected = round_rational(&BigInt::from(value), &step, mode) * &step;
        prop_assert_eq!(mode.round_to_increment(value, increment), i64::try_from(&expected).ok());
    }

    /// Whatever the mode, the result is a multiple of the increment, less than one increment
    /// from the value, and between the floor and ceiling results.
    #[test]
    fn increment_rounding_brackets_the_value(value in any_minor(), increment in any_increment()) {
        let step = i128::from(increment.get());
        let exact = i128::from(value);
        let floor = RoundingMode::Floor.round_to_increment(value, increment).map(i128::from);
        let ceiling = RoundingMode::Ceiling.round_to_increment(value, increment).map(i128::from);
        for mode in RoundingMode::ALL {
            let Some(rounded) = mode.round_to_increment(value, increment).map(i128::from) else {
                continue;
            };
            prop_assert_eq!(rounded.rem_euclid(step), 0, "{:?}", mode);
            prop_assert!((rounded - exact).abs() < step, "{:?}", mode);
            prop_assert!(floor.is_none_or(|floor| floor <= rounded), "{:?}", mode);
            prop_assert!(ceiling.is_none_or(|ceiling| rounded <= ceiling), "{:?}", mode);
        }
    }

    /// Rounding treats refunds like sales: symmetric modes mirror around zero, and ceiling is
    /// the mirror of floor.
    #[test]
    fn increment_rounding_is_symmetric(value in negatable_minor(), increment in any_increment()) {
        let pairs = SYMMETRIC_MODES
            .map(|mode| (mode, mode))
            .into_iter()
            .chain([(RoundingMode::Floor, RoundingMode::Ceiling)]);
        for (mode, mirror_mode) in pairs {
            let sale = mode.round_to_increment(value, increment);
            let refund = mirror_mode.round_to_increment(-value, increment);
            match (sale, refund) {
                (Some(sale), Some(refund)) => prop_assert_eq!(refund, -sale, "{:?}", mode),
                // i64's range is asymmetric: −2^63 fits but 2^63 doesn't. That edge is the only
                // place where a result can fit on one side and not on the other.
                (Some(sale), None) => prop_assert_eq!(sale, i64::MIN, "{:?}", mode),
                (None, Some(refund)) => prop_assert_eq!(refund, i64::MIN, "{:?}", mode),
                (None, None) => {}
            }
        }
    }

    /// `round(d, places)` (implemented by rust_decimal) rounds the exact value once.
    #[test]
    fn decimal_rounding_is_exact(value in any_decimal(), places in 0_u32..=28, mode in any_mode()) {
        let rounded = mode.round(value, places);
        if places >= value.scale() {
            prop_assert_eq!(rounded, value);
            prop_assert_eq!(rounded.scale(), value.scale());
        } else {
            let expected = round_rational(
                &BigInt::from(value.mantissa()),
                &pow10(value.scale() - places),
                mode,
            );
            // rounded = expected / 10^places, compared without division.
            prop_assert_eq!(
                BigInt::from(rounded.mantissa()) * pow10(places),
                expected * pow10(rounded.scale())
            );
        }
    }

    /// The same, exactly on ties and one unit in the last place either side of them, where
    /// rounding modes differ and implementations most often go wrong.
    #[test]
    fn decimal_rounding_is_exact_near_ties((value, places) in near_tie_decimal(), mode in any_mode()) {
        let expected = round_rational(
            &BigInt::from(value.mantissa()),
            &pow10(value.scale() - places),
            mode,
        );
        let rounded = mode.round(value, places);
        prop_assert_eq!(
            BigInt::from(rounded.mantissa()) * pow10(places),
            expected * pow10(rounded.scale())
        );
    }

    /// Two independent implementations agree: rust_decimal's rounding to a number of decimal
    /// places, and Keel's rounding to a power-of-ten increment.
    #[test]
    fn decimal_and_increment_rounding_agree(
        value in any_minor(),
        digits in 0_u32..=9,
        mode in any_mode(),
    ) {
        let increment = NonZeroU32::new(10_u32.pow(digits)).unwrap();
        let via_decimal = mode.round(Decimal::new(value, digits), 0) * Decimal::from(increment.get());
        match mode.round_to_increment(value, increment) {
            Some(rounded) => prop_assert_eq!(Decimal::from(rounded), via_decimal),
            None => prop_assert!(
                via_decimal > Decimal::from(i64::MAX) || via_decimal < Decimal::from(i64::MIN)
            ),
        }
    }
}
