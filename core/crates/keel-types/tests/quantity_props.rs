//! Property tests: quantity conversions round the exact value once, and arithmetic is exact.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_types::{Decimal, Dimension, Quantity, QuantityError, RoundingMode, Unit};
use num_bigint::BigInt;
use proptest::prelude::*;
use support::{any_decimal, any_minor, any_mode, moderate_decimal, pow10, round_rational};

fn any_unit() -> impl Strategy<Value = Unit> {
    prop::sample::select(Unit::ALL.to_vec())
}

/// Two units measuring the same dimension (possibly the same unit).
fn unit_pair() -> impl Strategy<Value = (Unit, Unit)> {
    prop::sample::select(Dimension::ALL.to_vec()).prop_flat_map(|dimension| {
        (prop::sample::select(units_of(dimension)), prop::sample::select(units_of(dimension)))
    })
}

/// The units of one dimension.
fn units_of(dimension: Dimension) -> Vec<Unit> {
    Unit::ALL.iter().copied().filter(|unit| unit.dimension() == dimension).collect()
}

/// Two units of different dimensions: generated directly, so nothing is rejected.
fn cross_dimension_pair() -> impl Strategy<Value = (Unit, Unit)> {
    let count = Dimension::ALL.len();
    (0..count, 1..count).prop_flat_map(move |(first, offset)| {
        let dimension = |index: usize| Dimension::ALL.get(index % count).copied().unwrap();
        let (from, to) = (dimension(first), dimension(first + offset));
        (prop::sample::select(units_of(from)), prop::sample::select(units_of(to)))
    })
}

/// Two units of the same dimension, the coarser (larger) first.
fn coarse_and_fine() -> impl Strategy<Value = (Unit, Unit)> {
    unit_pair().prop_map(|(a, b)| {
        if a.size_in_base_unit() >= b.size_in_base_unit() { (a, b) } else { (b, a) }
    })
}

/// Modes that round to the nearest value.
fn nearest_mode() -> impl Strategy<Value = RoundingMode> {
    prop::sample::select(vec![
        RoundingMode::HalfAwayFromZero,
        RoundingMode::HalfEven,
        RoundingMode::HalfTowardZero,
    ])
}

/// The exact size of `unit` in base units, as a fraction.
fn size(unit: Unit) -> (BigInt, BigInt) {
    let size = unit.size_in_base_unit();
    (BigInt::from(size.mantissa()), pow10(size.scale()))
}

/// `micros` of `from`, converted exactly to `to` and rounded with `mode`: the oracle.
fn converted(micros: i64, from: Unit, to: Unit, mode: RoundingMode) -> BigInt {
    let (from_numerator, from_denominator) = size(from);
    let (to_numerator, to_denominator) = size(to);
    round_rational(
        &(BigInt::from(micros) * from_numerator * to_denominator),
        &(from_denominator * to_numerator),
        mode,
    )
}

/// The expected result of an exact computation: the quantity if it fits, otherwise overflow.
fn expect(micros: &BigInt, unit: Unit) -> Result<Quantity, QuantityError> {
    i64::try_from(micros)
        .map(|micros| Quantity::from_micros(micros, unit))
        .map_err(|_| QuantityError::Overflow)
}

proptest! {
    /// Conversion is the exact value, rounded once, or `Overflow` exactly when out of range.
    #[test]
    fn conversion_rounds_the_exact_value_once(
        micros in any_minor(),
        (from, to) in unit_pair(),
        mode in any_mode(),
    ) {
        let quantity = Quantity::from_micros(micros, from);
        let expected = expect(&converted(micros, from, to, mode), to);
        prop_assert_eq!(quantity.convert_to(to, mode), expected);
    }

    /// Units of different dimensions never convert.
    #[test]
    fn conversion_never_crosses_dimensions(
        micros in any_minor(),
        (from, to) in cross_dimension_pair(),
        mode in any_mode(),
    ) {
        let quantity = Quantity::from_micros(micros, from);
        prop_assert_eq!(
            quantity.convert_to(to, mode),
            Err(QuantityError::DimensionMismatch { from, to })
        );
    }

    /// An exact conversion succeeds exactly when no rounding is needed.
    #[test]
    fn exact_conversion_succeeds_only_without_rounding(
        micros in prop_oneof![any_minor(), (-1_000_000_i64..=1_000_000).prop_map(|units| units * 1_000_000)],
        (from, to) in unit_pair(),
    ) {
        let quantity = Quantity::from_micros(micros, from);
        let floor = expect(&converted(micros, from, to, RoundingMode::Floor), to);
        let ceiling = expect(&converted(micros, from, to, RoundingMode::Ceiling), to);
        let expected = match (floor, ceiling) {
            (Ok(floor), Ok(ceiling)) if floor == ceiling => Ok(floor),
            (Ok(_), Ok(_)) => Err(QuantityError::InexactConversion { quantity, to }),
            _ => Err(QuantityError::Overflow),
        };
        prop_assert_eq!(quantity.convert_exact(to), expected);
    }

    /// Converting to a finer unit and back, rounding to the nearest both ways, loses nothing.
    #[test]
    fn round_trip_through_a_finer_unit_is_lossless(
        micros in any_minor(),
        (coarse, fine) in coarse_and_fine(),
        there in nearest_mode(),
        back in nearest_mode(),
    ) {
        let quantity = Quantity::from_micros(micros, coarse);
        if let Ok(converted) = quantity.convert_to(fine, there) {
            prop_assert_eq!(converted.convert_to(coarse, back), Ok(quantity));
        }
    }

    /// Conversion preserves order: rounding is monotonic.
    #[test]
    fn conversion_is_monotonic(
        a in any_minor(),
        b in any_minor(),
        (from, to) in unit_pair(),
        mode in any_mode(),
    ) {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        let low = Quantity::from_micros(low, from).convert_to(to, mode);
        let high = Quantity::from_micros(high, from).convert_to(to, mode);
        if let (Ok(low), Ok(high)) = (low, high) {
            prop_assert!(low.micros() <= high.micros());
        }
    }

    /// Checked arithmetic returns the exact result, or `Overflow` exactly when out of range.
    #[test]
    fn arithmetic_is_exact(a in any_minor(), b in any_minor(), unit in any_unit()) {
        let (x, y) = (Quantity::from_micros(a, unit), Quantity::from_micros(b, unit));
        let (exact_a, exact_b) = (BigInt::from(a), BigInt::from(b));
        prop_assert_eq!(x.checked_add(y), expect(&(&exact_a + &exact_b), unit));
        prop_assert_eq!(x.checked_sub(y), expect(&(&exact_a - &exact_b), unit));
        prop_assert_eq!(x.checked_mul(b), expect(&(&exact_a * &exact_b), unit));
        prop_assert_eq!(x.checked_neg(), expect(&-&exact_a, unit));
        prop_assert_eq!(x.compare(y), Ok(a.cmp(&b)));
    }

    /// A sum is the exact total, in any order.
    #[test]
    fn sum_is_exact_in_any_order(values in prop::collection::vec(any_minor(), 0..8), unit in any_unit()) {
        let exact: BigInt = values.iter().map(|&micros| BigInt::from(micros)).sum();
        let quantities = values.iter().map(|&micros| Quantity::from_micros(micros, unit));
        let expected = expect(&exact, unit);
        prop_assert_eq!(Quantity::sum(unit, quantities.clone()), expected.clone());
        prop_assert_eq!(Quantity::sum(unit, quantities.rev()), expected);
    }

    /// Scaling by a decimal rounds the exact product once.
    #[test]
    fn mul_decimal_rounds_the_exact_product_once(
        micros in any_minor(),
        factor in prop_oneof![moderate_decimal(), any_decimal()],
        unit in any_unit(),
        mode in any_mode(),
    ) {
        let product = BigInt::from(micros) * BigInt::from(factor.mantissa());
        let expected = round_rational(&product, &pow10(factor.scale()), mode);
        let quantity = Quantity::from_micros(micros, unit);
        prop_assert_eq!(quantity.mul_decimal(factor, mode), expect(&expected, unit));
    }

    /// Rounding to decimal places rounds to a multiple of the dropped power of ten.
    #[test]
    fn rounding_to_places_is_exact(micros in any_minor(), places in 0_u32..=8, mode in any_mode()) {
        let quantity = Quantity::from_micros(micros, Unit::Kilogram);
        let step = pow10(6_u32.saturating_sub(places));
        let expected = round_rational(&BigInt::from(micros), &step, mode) * &step;
        prop_assert_eq!(quantity.round_to_places(places, mode), expect(&expected, Unit::Kilogram));
    }

    /// Every quantity survives a trip through `Display` and `parse`, and through decimals.
    #[test]
    fn text_and_decimal_round_trips(micros in any_minor(), unit in any_unit(), mode in any_mode()) {
        let quantity = Quantity::from_micros(micros, unit);
        let text = quantity.to_string();
        let (value, code) = text.split_once(' ').unwrap();
        prop_assert_eq!(code, unit.code());
        prop_assert_eq!(Quantity::parse(value, unit), Ok(quantity));
        prop_assert_eq!(Unit::from_code(code), Ok(unit));

        let decimal = quantity.to_decimal();
        prop_assert_eq!(decimal, Decimal::new(micros, 6));
        prop_assert_eq!(Quantity::from_decimal(decimal, unit, mode), Ok(quantity));
        prop_assert_eq!(Quantity::from_decimal_exact(decimal, unit), Ok(quantity));
    }
}
