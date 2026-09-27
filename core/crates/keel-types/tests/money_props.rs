//! Property tests: money arithmetic is exact, checked, fair and currency-safe.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_types::{Currency, Money, MoneyError};
use num_bigint::BigInt;
use proptest::prelude::*;
use support::{
    any_currency, any_decimal, any_minor, any_mode, moderate_decimal, near_boundary_product,
    negatable_minor, pow10, round_rational,
};

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::USD)
}

/// The expected result of an exact computation: the amount if it fits, otherwise overflow.
fn expect(exact: &BigInt, currency: Currency) -> Result<Money, MoneyError> {
    i64::try_from(exact)
        .map(|minor| Money::from_minor(minor, currency))
        .map_err(|_| MoneyError::Overflow)
}

/// Allocation weights: zeros, small weights, and weights across the whole `u64` range.
fn weights() -> impl Strategy<Value = Vec<u64>> {
    let weight = prop_oneof![
        2 => Just(0_u64),
        4 => 1_u64..=100,
        2 => any::<u64>(),
        1 => Just(u64::MAX),
    ];
    prop::collection::vec(weight, 1..=12)
}

/// Allocation weights with at least one non-zero weight.
fn nonzero_weights() -> impl Strategy<Value = Vec<u64>> {
    weights().prop_filter("some weight is non-zero", |weights| weights.iter().any(|&w| w > 0))
}

/// A recognizer for `Money::parse`'s grammar (`-?[0-9]+(\.[0-9]{1,minor_units})?`), written as a
/// state machine so it shares no code or structure with the parser.
fn matches_grammar(text: &str, minor_units: u8) -> bool {
    #[derive(Clone, Copy)]
    enum State {
        Start,
        Sign,
        Integer,
        Point,
        Fraction(u8),
    }
    let mut state = State::Start;
    for character in text.chars() {
        state = match (state, character) {
            (State::Start, '-') => State::Sign,
            (State::Start | State::Sign | State::Integer, '0'..='9') => State::Integer,
            (State::Integer, '.') => State::Point,
            (State::Point, '0'..='9') => State::Fraction(1),
            (State::Fraction(digits), '0'..='9') => State::Fraction(digits + 1),
            _ => return false,
        };
    }
    match state {
        State::Integer => true,
        State::Fraction(digits) => digits <= minor_units,
        State::Start | State::Sign | State::Point => false,
    }
}

/// The exact value, in minor units, of text that matches the grammar.
fn grammar_value(text: &str, minor_units: u8) -> BigInt {
    let (negative, unsigned) = text.strip_prefix('-').map_or((false, text), |rest| (true, rest));
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = format!("{integer}{fraction:0<width$}", width = usize::from(minor_units));
    let magnitude = BigInt::parse_bytes(digits.as_bytes(), 10).unwrap();
    if negative { -magnitude } else { magnitude }
}

proptest! {
    /// `mul_decimal` rounds the exact product once, however many bits the product needs.
    #[test]
    fn mul_decimal_is_exact(
        minor in any_minor(),
        factor in prop_oneof![moderate_decimal(), any_decimal()],
        mode in any_mode(),
    ) {
        let product = BigInt::from(minor) * BigInt::from(factor.mantissa());
        let expected = round_rational(&product, &pow10(factor.scale()), mode);
        prop_assert_eq!(usd(minor).mul_decimal(factor, mode), expect(&expected, Currency::USD));
    }

    /// The same, with the product on a rounding boundary or a hair either side of it: exactly
    /// where rounding an intermediate result would push the answer across.
    #[test]
    fn mul_decimal_is_exact_near_boundaries(
        (minor, factor) in near_boundary_product(),
        mode in any_mode(),
    ) {
        let product = BigInt::from(minor) * BigInt::from(factor.mantissa());
        let expected = round_rational(&product, &pow10(factor.scale()), mode);
        prop_assert_eq!(usd(minor).mul_decimal(factor, mode), expect(&expected, Currency::USD));
    }

    /// Allocation is exact (parts sum to the whole) and fair (each part is within one minor
    /// unit of its exact share; zero weights get nothing).
    #[test]
    fn allocation_is_exact_and_fair(minor in any_minor(), weights in weights()) {
        let result = usd(minor).allocate(&weights);
        if weights.iter().all(|&weight| weight == 0) {
            prop_assert_eq!(result, Err(MoneyError::ZeroTotalWeight));
            return Ok(());
        }
        let parts = result.unwrap();
        prop_assert_eq!(parts.len(), weights.len());
        let sum: BigInt = parts.iter().map(|part| BigInt::from(part.minor())).sum();
        prop_assert_eq!(sum, BigInt::from(minor));

        let total_weight: BigInt = weights.iter().map(|&weight| BigInt::from(weight)).sum();
        for (part, &weight) in parts.iter().zip(&weights) {
            prop_assert_eq!(part.currency(), Currency::USD);
            // |part − minor × weight / total| < 1, scaled by the total weight.
            let error = BigInt::from(part.minor()) * &total_weight
                - BigInt::from(minor) * BigInt::from(weight);
            prop_assert!(error.magnitude() < total_weight.magnitude());
            if weight == 0 {
                prop_assert_eq!(part.minor(), 0);
            }
        }
    }

    /// A larger weight never receives less. Equal weights differ by at most one minor unit,
    /// and the earlier part receives the extra unit.
    #[test]
    fn allocation_is_monotonic_and_ordered(minor in any_minor(), weights in nonzero_weights()) {
        let parts = usd(minor).allocate(&weights).unwrap();
        let magnitudes: Vec<u64> = parts.iter().map(|part| part.minor().unsigned_abs()).collect();
        for i in 0..weights.len() {
            for j in 0..weights.len() {
                if weights[i] > weights[j] {
                    prop_assert!(magnitudes[i] >= magnitudes[j]);
                }
                if weights[i] == weights[j] && i < j {
                    prop_assert!(magnitudes[i] == magnitudes[j] || magnitudes[i] == magnitudes[j] + 1);
                }
            }
        }
    }

    /// Refunds split like sales: allocating `-x` gives the negated parts of allocating `x`.
    #[test]
    fn allocation_is_symmetric(minor in negatable_minor(), weights in nonzero_weights()) {
        let sale = usd(minor).allocate(&weights).unwrap();
        let refund = usd(-minor).allocate(&weights).unwrap();
        for (sale_part, refund_part) in sale.iter().zip(&refund) {
            prop_assert_eq!(refund_part.minor(), -sale_part.minor());
        }
    }

    /// Checked arithmetic returns the exact result, or `Overflow` exactly when it's out of range.
    #[test]
    fn checked_arithmetic_is_exact(a in any_minor(), b in any_minor()) {
        let (x, y) = (BigInt::from(a), BigInt::from(b));
        prop_assert_eq!(usd(a).checked_add(usd(b)), expect(&(&x + &y), Currency::USD));
        prop_assert_eq!(usd(a).checked_sub(usd(b)), expect(&(&x - &y), Currency::USD));
        prop_assert_eq!(usd(a).checked_mul(b), expect(&(&x * &y), Currency::USD));
        prop_assert_eq!(usd(a).checked_neg(), expect(&-&x, Currency::USD));
        prop_assert_eq!(usd(a).checked_abs(), expect(&BigInt::from(x.magnitude().clone()), Currency::USD));
        prop_assert_eq!(usd(a).compare(usd(b)), Ok(a.cmp(&b)));
    }

    /// A sum is the exact total, in any order.
    #[test]
    fn sum_is_exact_in_any_order(amounts in prop::collection::vec(any_minor(), 0..8)) {
        let exact: BigInt = amounts.iter().map(|&minor| BigInt::from(minor)).sum();
        let expected = expect(&exact, Currency::USD);
        prop_assert_eq!(Money::sum(Currency::USD, amounts.iter().copied().map(usd)), expected.clone());
        prop_assert_eq!(Money::sum(Currency::USD, amounts.iter().rev().copied().map(usd)), expected);
    }

    /// Every amount in every currency survives a trip through `Display` and `parse`.
    #[test]
    fn display_and_parse_round_trip(minor in any_minor(), currency in any_currency()) {
        let money = Money::from_minor(minor, currency);
        let text = money.to_string();
        let (code, amount) = text.split_once(' ').unwrap();
        prop_assert_eq!(code, currency.code());
        prop_assert_eq!(Money::parse(amount, currency), Ok(money));
    }

    /// `parse` accepts exactly its grammar, with the exact value, and rejects everything else.
    #[test]
    fn parse_accepts_exactly_its_grammar(
        // Junk from a small alphabet (incl. Arabic-Indic and fullwidth digits), or near-valid text.
        text in "-?[-0-9.,+e ١１]{0,8}|-?[0-9]{1,20}(\\.[0-9]{0,4})?",
        currency in any_currency(),
    ) {
        let result = Money::parse(&text, currency);
        if matches_grammar(&text, currency.minor_units()) {
            let exact = grammar_value(&text, currency.minor_units());
            prop_assert_eq!(result, expect(&exact, currency));
        } else {
            prop_assert!(
                matches!(result, Err(MoneyError::InvalidFormat { .. })),
                "{:?} should be rejected, got {:?}", text, result
            );
        }
    }

    /// `to_decimal` is exact, and converting back gives the same amount in every mode.
    #[test]
    fn decimal_round_trip(minor in any_minor(), currency in any_currency(), mode in any_mode()) {
        let money = Money::from_minor(minor, currency);
        let value = money.to_decimal();
        let minor_units = u32::from(currency.minor_units());
        prop_assert_eq!(
            BigInt::from(value.mantissa()) * pow10(minor_units),
            BigInt::from(minor) * pow10(value.scale())
        );
        prop_assert_eq!(Money::from_decimal(value, currency, mode), Ok(money));
        prop_assert_eq!(Money::from_decimal_exact(value, currency), Ok(money));
    }

    /// `from_decimal` rounds the exact value once; `from_decimal_exact` accepts exactly the
    /// values that need no rounding.
    #[test]
    fn from_decimal_rounds_once(
        value in any_decimal(),
        currency in any_currency(),
        mode in any_mode(),
    ) {
        let minor_units = u32::from(currency.minor_units());
        let numerator = BigInt::from(value.mantissa()) * pow10(minor_units);
        let denominator = pow10(value.scale());
        let expected = round_rational(&numerator, &denominator, mode);
        prop_assert_eq!(Money::from_decimal(value, currency, mode), expect(&expected, currency));

        // Too precise is reported first; otherwise the exact value, if it's in range.
        let needs_rounding = &numerator % &denominator != BigInt::ZERO;
        let expected_exact = if needs_rounding {
            Err(MoneyError::PrecisionExceeded { value, currency })
        } else {
            expect(&(&numerator / &denominator), currency)
        };
        prop_assert_eq!(Money::from_decimal_exact(value, currency), expected_exact);
    }

    /// Mixing currencies is always an error, never a silent conversion.
    #[test]
    fn currencies_never_mix(
        a in any_minor(),
        b in any_minor(),
        (left, right) in (any_currency(), any_currency())
            .prop_filter("different currencies", |(left, right)| left != right),
    ) {
        let mismatch = MoneyError::CurrencyMismatch { left, right };
        let (x, y) = (Money::from_minor(a, left), Money::from_minor(b, right));
        prop_assert_eq!(x.checked_add(y), Err(mismatch.clone()));
        prop_assert_eq!(x.checked_sub(y), Err(mismatch.clone()));
        prop_assert_eq!(x.compare(y), Err(mismatch.clone()));
        prop_assert_eq!(Money::sum(left, [x, y]), Err(mismatch));
    }
}
