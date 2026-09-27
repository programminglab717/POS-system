//! Rates: exact proportions such as tax rates, discounts and commissions.

use core::fmt;

use rust_decimal::Decimal;

use crate::money::{Money, MoneyError};
use crate::rounding::RoundingMode;

/// An exact proportion, stored as a decimal fraction: 8.875% is `0.08875`.
///
/// Used for tax rates, percentage discounts, service charges and commissions. A rate may be
/// negative; whether that makes sense is the caller's rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rate(Decimal);

impl Rate {
    /// A rate of zero.
    pub const ZERO: Rate = Rate(Decimal::ZERO);

    /// A rate from a fraction: `0.2` is 20%.
    pub const fn from_fraction(fraction: Decimal) -> Rate {
        Rate(fraction)
    }

    /// A rate from a percentage: `8.875` is 8.875%.
    ///
    /// The conversion is exact: it only moves the decimal point.
    ///
    /// # Errors
    /// [`RateError::TooPrecise`] if the percentage has more than 26 significant decimal places,
    /// which leaves no room to shift the decimal point exactly.
    pub fn from_percent(percent: Decimal) -> Result<Rate, RateError> {
        // Trailing zeros don't count against the precision limit.
        let percent = percent.normalize();
        let scale = percent.scale().checked_add(2).ok_or(RateError::TooPrecise)?;
        Decimal::try_from_i128_with_scale(percent.mantissa(), scale)
            .map(Rate)
            .map_err(|_| RateError::TooPrecise)
    }

    /// A rate from basis points: `25` is 0.25%.
    pub fn from_basis_points(basis_points: i64) -> Rate {
        Rate(Decimal::new(basis_points, 4))
    }

    /// The rate as a fraction: 8.875% → `0.08875`.
    pub const fn as_fraction(self) -> Decimal {
        self.0
    }

    /// The rate as a percentage: 0.08875 → `8.875`. `None` only for absurdly large rates.
    pub fn as_percent(self) -> Option<Decimal> {
        self.0.checked_mul(Decimal::ONE_HUNDRED).map(|percent| percent.normalize())
    }

    /// `amount × rate`, rounded once to the currency's minor unit with `mode`.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] if the result is out of range.
    pub fn apply(self, amount: Money, mode: RoundingMode) -> Result<Money, MoneyError> {
        amount.mul_decimal(self.0, mode)
    }
}

impl fmt::Display for Rate {
    /// The rate as a percentage: `8.875%`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_percent() {
            Some(percent) => write!(f, "{percent}%"),
            None => write!(f, "{}×", self.0.normalize()),
        }
    }
}

/// Errors from creating a [`Rate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RateError {
    /// The value had too many decimal places to convert exactly.
    #[error("rate has too many decimal places to represent exactly")]
    TooPrecise,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency::Currency;

    fn dec(text: &str) -> Decimal {
        text.parse().unwrap()
    }

    #[test]
    fn constructors_are_exact() {
        assert_eq!(Rate::from_percent(dec("8.875")).unwrap().as_fraction(), dec("0.08875"));
        assert_eq!(Rate::from_percent(dec("20")).unwrap().as_fraction(), dec("0.2"));
        assert_eq!(Rate::from_basis_points(25).as_fraction(), dec("0.0025"));
        assert_eq!(Rate::from_fraction(dec("0.09975")).as_percent(), Some(dec("9.975")));
    }

    #[test]
    fn too_precise_percentages_are_rejected() {
        let percent = Decimal::from_i128_with_scale(1, 28); // 1e-28 %
        assert_eq!(Rate::from_percent(percent), Err(RateError::TooPrecise));
        let percent = Decimal::from_i128_with_scale(1, 27); // 1e-27 %
        assert_eq!(Rate::from_percent(percent), Err(RateError::TooPrecise));
        // 26 decimal places is the limit; trailing zeros don't count.
        let percent = Decimal::from_i128_with_scale(1, 26);
        let fraction = Decimal::from_i128_with_scale(1, 28);
        assert_eq!(Rate::from_percent(percent).unwrap().as_fraction(), fraction);
        let padded = dec("8.875000000000000000000000000"); // 27 decimal places
        assert_eq!(Rate::from_percent(padded).unwrap().as_fraction(), dec("0.08875"));
    }

    #[test]
    fn display_as_percentage() {
        assert_eq!(Rate::from_percent(dec("8.875")).unwrap().to_string(), "8.875%");
        assert_eq!(Rate::from_basis_points(1500).to_string(), "15%");
        assert_eq!(Rate::ZERO.to_string(), "0%");
    }

    proptest::proptest! {
        /// Percent conversions only move the decimal point, so they are exact and reversible.
        #[test]
        fn percent_conversions_are_exact(mantissa in proptest::num::i64::ANY, scale in 0_u32..=26) {
            let percent = Decimal::from_i128_with_scale(i128::from(mantissa), scale);
            let rate = Rate::from_percent(percent).unwrap();
            proptest::prop_assert_eq!(rate.as_percent(), Some(percent.normalize()));
            proptest::prop_assert_eq!(Rate::from_percent(rate.as_percent().unwrap()), Ok(rate));
        }
    }

    #[test]
    fn apply_rounds_once() {
        let rate = Rate::from_percent(dec("8.875")).unwrap();
        let amount = Money::from_minor(1000, Currency::USD);
        assert_eq!(rate.apply(amount, RoundingMode::HalfAwayFromZero).unwrap().minor(), 89);
        assert_eq!(rate.apply(amount, RoundingMode::HalfEven).unwrap().minor(), 89);
        assert_eq!(rate.apply(amount, RoundingMode::TowardZero).unwrap().minor(), 88);
    }
}
