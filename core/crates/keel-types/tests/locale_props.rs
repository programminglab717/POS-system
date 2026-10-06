//! Property tests of how a locale shows amounts and quantities, against a model built
//! differently: from arbitrary-precision integers, grouped by repeated division by 1,000 rather
//! than by counting digits.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

mod support;

use std::fmt::Write as _;

use keel_types::{Currency, Locale, Money, Quantity, Unit};
use num_bigint::BigInt;
use num_integer::Integer;
use proptest::prelude::*;
use support::any_minor;

/// The decimal and grouping symbols of `locale`: both of Keel's locales use `.` and `,`, as
/// CLDR's `en` and `es-US` say.
fn separators(locale: Locale) -> (char, char) {
    match locale.tag() {
        "en-US" | "es-US" => ('.', ','),
        other => panic!("no model for {other}"),
    }
}

/// `magnitude`, a whole number of units of the last of `decimals` decimal places, as digits
/// grouped by thousands with `decimals` fraction digits.
fn model_digits(locale: Locale, magnitude: &BigInt, decimals: u32) -> String {
    let (decimal, group) = separators(locale);
    let (mut integer, fraction) = magnitude.div_mod_floor(&BigInt::from(10).pow(decimals));
    // Groups of three digits, the most significant first, each but the first zero-padded.
    let thousand = BigInt::from(1000);
    let mut groups = Vec::new();
    loop {
        let (rest, group_value) = integer.div_mod_floor(&thousand);
        groups.push(group_value);
        if rest == BigInt::ZERO {
            break;
        }
        integer = rest;
    }
    groups.reverse();
    let mut text = groups[0].to_string();
    for group_value in &groups[1..] {
        text.push(group);
        write!(text, "{group_value:0>3}").unwrap();
    }
    if decimals > 0 {
        text.push(decimal);
        let width = usize::try_from(decimals).unwrap();
        write!(text, "{fraction:0>width$}").unwrap();
    }
    text
}

fn any_locale() -> impl Strategy<Value = Locale> {
    prop::sample::select(Locale::ALL.to_vec())
}

fn any_currency() -> impl Strategy<Value = Currency> {
    prop::sample::select(Currency::known().collect::<Vec<_>>())
}

proptest! {
    /// An amount reads as its sign, then the symbol its currency shows with zero, then its
    /// digits, every minor unit of them.
    #[test]
    fn an_amount_reads_as_its_sign_symbol_and_digits(
        locale in any_locale(),
        currency in any_currency(),
        minor in any_minor(),
    ) {
        let text = locale.money(Money::from_minor(minor, currency));
        let decimals = u32::from(currency.minor_units());
        let zero = model_digits(locale, &BigInt::ZERO, decimals);
        let shown = locale.money(Money::from_minor(0, currency));
        let symbol = shown.strip_suffix(&zero).unwrap();
        let sign = if minor < 0 { "-" } else { "" };
        let digits = model_digits(locale, &BigInt::from(minor.unsigned_abs()), decimals);
        prop_assert_eq!(text, format!("{sign}{symbol}{digits}"));
    }

    /// A quantity reads as its sign and digits, with as many decimals as it needs and no more.
    #[test]
    fn a_quantity_reads_with_the_decimals_it_needs(
        locale in any_locale(),
        micros in prop_oneof![
            any::<i64>(),
            -10_000_000_i64..=10_000_000,
            (-1_000_i64..=1_000).prop_map(|whole| whole * 1_000_000),
            prop::sample::select(vec![i64::MIN, i64::MAX, 0, 1, -1, 999_999, 1_000_000]),
        ],
    ) {
        let text = locale.quantity(Quantity::from_micros(micros, Unit::Each));
        let mut magnitude = BigInt::from(micros.unsigned_abs());
        let mut decimals = 6;
        while decimals > 0 && magnitude.is_multiple_of(&BigInt::from(10)) {
            magnitude /= 10;
            decimals -= 1;
        }
        let sign = if micros < 0 { "-" } else { "" };
        prop_assert_eq!(text, format!("{sign}{}", model_digits(locale, &magnitude, decimals)));
    }
}
