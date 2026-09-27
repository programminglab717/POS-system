//! Fixed-point decimals: integers counting units of 10^-places.
//!
//! Money (minor units of a currency) and quantities (millionths of a unit) are both fixed-point
//! decimals. The parsing and decimal conversions they share live here, written and tested once.

use rust_decimal::Decimal;

use crate::rounding::RoundingMode;

/// Why a value couldn't become a fixed-point integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixedPointError {
    /// Text didn't match the strict format.
    InvalidFormat,
    /// The value has more decimal places than the fixed point allows.
    PrecisionExceeded,
    /// The value is out of range.
    Overflow,
}

/// Parses text in the strict format `-?[0-9]+(\.[0-9]{1,places})?` into a count of 10^-places.
///
/// Thousands separators, exponents, a leading `+`, whitespace and decimal places beyond `places`
/// (even zeros) are rejected. With `places == 0`, no decimal point is allowed.
pub(crate) fn parse(text: &str, places: u32) -> Result<i64, FixedPointError> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let has_point = unsigned.contains('.');
    let places = usize::try_from(places).map_err(|_| FixedPointError::InvalidFormat)?;
    let well_formed = !integer.is_empty()
        && integer.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.bytes().all(|byte| byte.is_ascii_digit())
        && (!has_point || (1..=places).contains(&fraction.len()));
    if !well_formed {
        return Err(FixedPointError::InvalidFormat);
    }

    let padding = places.checked_sub(fraction.len()).ok_or(FixedPointError::InvalidFormat)?;
    let digits = integer.bytes().chain(fraction.bytes()).chain(core::iter::repeat_n(b'0', padding));
    // Accumulate the magnitude unsigned: the most negative value has no positive i64
    // counterpart, and must still parse.
    let mut magnitude: u64 = 0;
    for digit in digits {
        let digit = digit.checked_sub(b'0').ok_or(FixedPointError::InvalidFormat)?;
        magnitude = magnitude
            .checked_mul(10)
            .and_then(|shifted| shifted.checked_add(u64::from(digit)))
            .ok_or(FixedPointError::Overflow)?;
    }
    let value = if negative {
        0_i64.checked_sub_unsigned(magnitude)
    } else {
        i64::try_from(magnitude).ok()
    };
    value.ok_or(FixedPointError::Overflow)
}

/// `value × 10^places`, rounded once to an integer with `mode`.
pub(crate) fn from_decimal(
    value: Decimal,
    places: u32,
    mode: RoundingMode,
) -> Result<i64, FixedPointError> {
    scale_up(mode.round(value, places), places)
}

/// `value × 10^places`, which must be an integer: `value` may have at most `places`
/// significant decimal places.
pub(crate) fn from_decimal_exact(value: Decimal, places: u32) -> Result<i64, FixedPointError> {
    scale_up(value.normalize(), places)
}

/// The exact decimal value of `value × 10^-places`, with exactly `places` decimal places.
///
/// `places` must be at most 28, the most a `Decimal` supports: callers pass a currency's minor
/// units (at most `MAX_MINOR_UNITS`, checked at compile time) or `Quantity::DECIMAL_PLACES`.
pub(crate) fn to_decimal(value: i64, places: u32) -> Decimal {
    Decimal::new(value, places)
}

/// `value × 10^places`, for a value with at most `places` decimal places.
fn scale_up(value: Decimal, places: u32) -> Result<i64, FixedPointError> {
    let shift = places.checked_sub(value.scale()).ok_or(FixedPointError::PrecisionExceeded)?;
    let factor = 10_i128.checked_pow(shift).ok_or(FixedPointError::Overflow)?;
    let scaled = value.mantissa().checked_mul(factor).ok_or(FixedPointError::Overflow)?;
    i64::try_from(scaled).map_err(|_| FixedPointError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(text: &str) -> Decimal {
        text.parse().unwrap()
    }

    #[test]
    fn parse_boundaries() {
        assert_eq!(parse("92233720368547758.07", 2), Ok(i64::MAX));
        assert_eq!(parse("-92233720368547758.08", 2), Ok(i64::MIN));
        assert_eq!(parse("9223372036854.775807", 6), Ok(i64::MAX));
        assert_eq!(parse("-0", 2), Ok(0));
        assert_eq!(parse("92233720368547758.08", 2), Err(FixedPointError::Overflow));
        assert_eq!(parse("184467440737095516.16", 2), Err(FixedPointError::Overflow));
        assert_eq!(parse("1.0", 0), Err(FixedPointError::InvalidFormat));
        assert_eq!(parse("1.0000000", 6), Err(FixedPointError::InvalidFormat));
    }

    #[test]
    fn decimal_conversions() {
        assert_eq!(from_decimal(dec("1.2345675"), 6, RoundingMode::HalfEven), Ok(1_234_568));
        assert_eq!(from_decimal_exact(dec("1.234500000"), 6), Ok(1_234_500));
        assert_eq!(
            from_decimal_exact(dec("1.2345671"), 6),
            Err(FixedPointError::PrecisionExceeded)
        );
        assert_eq!(
            from_decimal_exact(dec("100000000000000000000"), 6),
            Err(FixedPointError::Overflow)
        );
        assert_eq!(to_decimal(1_234_500, 6).to_string(), "1.234500");
        assert_eq!(to_decimal(-5, 2).to_string(), "-0.05");
        assert_eq!(to_decimal(1200, 0).to_string(), "1200");
    }
}
