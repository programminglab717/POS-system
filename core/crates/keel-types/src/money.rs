//! Money: exact amounts, as integers in a currency's minor unit.

use core::cmp::Ordering;
use core::fmt;

use rust_decimal::Decimal;

use crate::currency::Currency;
use crate::fixed_point::{self, FixedPointError};
use crate::rounding::{RoundingMode, RoundingRule};

/// An exact amount of money: a whole number of minor units of a [`Currency`].
///
/// USD 12.34 is 1,234 minor units (cents) of USD; JPY 1200 is 1,200 minor units of JPY;
/// KWD 1.250 is 1,250 minor units (fils) of KWD.
///
/// Arithmetic never overflows silently and never mixes currencies: every operation that could
/// fail returns [`MoneyError`]. There are deliberately no `+`/`-` operators and no `PartialOrd`,
/// so every combination of amounts is visibly checked. Use [`Money::checked_add`] and
/// [`Money::compare`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    minor: i64,
    currency: Currency,
}

impl Money {
    /// An amount from a number of minor units.
    pub const fn from_minor(minor: i64, currency: Currency) -> Money {
        Money { minor, currency }
    }

    /// Zero in `currency`.
    pub const fn zero(currency: Currency) -> Money {
        Money { minor: 0, currency }
    }

    /// The amount in minor units.
    pub const fn minor(self) -> i64 {
        self.minor
    }

    /// The currency.
    pub const fn currency(self) -> Currency {
        self.currency
    }

    /// Whether the amount is zero.
    pub const fn is_zero(self) -> bool {
        self.minor == 0
    }

    /// Whether the amount is greater than zero.
    pub const fn is_positive(self) -> bool {
        self.minor > 0
    }

    /// Whether the amount is less than zero.
    pub const fn is_negative(self) -> bool {
        self.minor < 0
    }

    /// `self + rhs`.
    ///
    /// # Errors
    /// [`MoneyError::CurrencyMismatch`] if the currencies differ, [`MoneyError::Overflow`] if the
    /// result is out of range.
    pub fn checked_add(self, rhs: Money) -> Result<Money, MoneyError> {
        let currency = self.same_currency(rhs)?;
        let minor = self.minor.checked_add(rhs.minor).ok_or(MoneyError::Overflow)?;
        Ok(Money { minor, currency })
    }

    /// `self - rhs`.
    ///
    /// # Errors
    /// [`MoneyError::CurrencyMismatch`] if the currencies differ, [`MoneyError::Overflow`] if the
    /// result is out of range.
    pub fn checked_sub(self, rhs: Money) -> Result<Money, MoneyError> {
        let currency = self.same_currency(rhs)?;
        let minor = self.minor.checked_sub(rhs.minor).ok_or(MoneyError::Overflow)?;
        Ok(Money { minor, currency })
    }

    /// `-self`.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] for the one amount whose negation is out of range.
    pub fn checked_neg(self) -> Result<Money, MoneyError> {
        let minor = self.minor.checked_neg().ok_or(MoneyError::Overflow)?;
        Ok(Money { minor, ..self })
    }

    /// `|self|`.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] for the one amount whose absolute value is out of range.
    pub fn checked_abs(self) -> Result<Money, MoneyError> {
        let minor = self.minor.checked_abs().ok_or(MoneyError::Overflow)?;
        Ok(Money { minor, ..self })
    }

    /// `self × factor`, for whole-number multiples (three coffees at USD 4.50).
    ///
    /// For fractional factors (weights, rates), use [`Money::mul_decimal`].
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] if the result is out of range.
    pub fn checked_mul(self, factor: i64) -> Result<Money, MoneyError> {
        let minor = self.minor.checked_mul(factor).ok_or(MoneyError::Overflow)?;
        Ok(Money { minor, ..self })
    }

    /// `self × factor`, rounded to the minor unit with `mode`.
    ///
    /// For example, a price per kilogram times a weight, or an amount times a rate. The product
    /// is computed exactly, however many digits it has, then rounded once.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] if the result is out of range.
    pub fn mul_decimal(self, factor: Decimal, mode: RoundingMode) -> Result<Money, MoneyError> {
        let minor = mode.round_product(self.minor, factor).ok_or(MoneyError::Overflow)?;
        Ok(Money { minor, ..self })
    }

    /// Compares two amounts in the same currency.
    ///
    /// # Errors
    /// [`MoneyError::CurrencyMismatch`] if the currencies differ.
    pub fn compare(self, other: Money) -> Result<Ordering, MoneyError> {
        self.same_currency(other)?;
        Ok(self.minor.cmp(&other.minor))
    }

    /// The sum of `amounts`, all of which must be in `currency`. An empty sum is zero.
    ///
    /// The sum is exact and doesn't depend on the order of `amounts`: running totals may leave
    /// the representable range, as long as the final total is back in it.
    ///
    /// # Errors
    /// [`MoneyError::CurrencyMismatch`] if any amount is in another currency,
    /// [`MoneyError::Overflow`] if the sum is out of range.
    pub fn sum<I>(currency: Currency, amounts: I) -> Result<Money, MoneyError>
    where
        I: IntoIterator<Item = Money>,
    {
        let mut total: i128 = 0;
        for amount in amounts {
            if amount.currency != currency {
                return Err(MoneyError::CurrencyMismatch {
                    left: currency,
                    right: amount.currency,
                });
            }
            // Can't overflow before 2^64 amounts; checked regardless.
            total = total.checked_add(i128::from(amount.minor)).ok_or(MoneyError::Overflow)?;
        }
        let minor = i64::try_from(total).map_err(|_| MoneyError::Overflow)?;
        Ok(Money { minor, currency })
    }

    /// Rounds the amount with `rule`, for example cash rounding to the nearest 5 cents.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] if the rounded amount is out of range.
    pub fn round_to(self, rule: RoundingRule) -> Result<Money, MoneyError> {
        let minor = rule
            .mode()
            .round_to_increment(self.minor, rule.increment())
            .ok_or(MoneyError::Overflow)?;
        Ok(Money { minor, ..self })
    }

    /// The amount in major units, exactly, with the currency's decimal places: USD 12.30 →
    /// `12.30`.
    pub fn to_decimal(self) -> Decimal {
        fixed_point::to_decimal(self.minor, self.minor_units())
    }

    /// An amount from a decimal in major units, rounded to the currency's minor unit with `mode`.
    ///
    /// # Errors
    /// [`MoneyError::Overflow`] if the amount is out of range.
    pub fn from_decimal(
        value: Decimal,
        currency: Currency,
        mode: RoundingMode,
    ) -> Result<Money, MoneyError> {
        let places = u32::from(currency.minor_units());
        fixed_point::from_decimal(value, places, mode)
            .map(|minor| Money { minor, currency })
            .map_err(|_| MoneyError::Overflow)
    }

    /// An amount from a decimal in major units that must be exactly representable.
    ///
    /// # Errors
    /// [`MoneyError::PrecisionExceeded`] if `value` has more significant decimal places than the
    /// currency allows (USD 12.345), otherwise [`MoneyError::Overflow`] if it is out of range.
    pub fn from_decimal_exact(value: Decimal, currency: Currency) -> Result<Money, MoneyError> {
        let places = u32::from(currency.minor_units());
        match fixed_point::from_decimal_exact(value, places) {
            Ok(minor) => Ok(Money { minor, currency }),
            Err(FixedPointError::PrecisionExceeded) => {
                Err(MoneyError::PrecisionExceeded { value, currency })
            }
            Err(FixedPointError::Overflow | FixedPointError::InvalidFormat) => {
                Err(MoneyError::Overflow)
            }
        }
    }

    /// Parses an amount in major units, such as `"12.34"` or `"-0.50"`.
    ///
    /// The format is strict, for APIs, imports and tests: an optional `-`, at least one digit, and
    /// optionally a `.` followed by 1 to `minor_units` digits. Thousands separators, exponents,
    /// leading `+`, whitespace and extra decimal places (even zeros) are rejected.
    /// Human-facing, locale-aware input belongs in the UI layer.
    ///
    /// # Errors
    /// [`MoneyError::InvalidFormat`] if the text doesn't match the format,
    /// [`MoneyError::Overflow`] if the amount is out of range.
    pub fn parse(text: &str, currency: Currency) -> Result<Money, MoneyError> {
        match fixed_point::parse(text, u32::from(currency.minor_units())) {
            Ok(minor) => Ok(Money { minor, currency }),
            Err(FixedPointError::Overflow) => Err(MoneyError::Overflow),
            Err(FixedPointError::InvalidFormat | FixedPointError::PrecisionExceeded) => {
                Err(MoneyError::InvalidFormat { text: text.to_owned(), currency })
            }
        }
    }

    /// Splits the amount into parts proportional to `weights`, exactly.
    ///
    /// The parts always sum to exactly the original amount, and each part is within one minor
    /// unit of its exact proportional share. This is how Keel splits checks, spreads order
    /// discounts over lines, and allocates anything else where the pieces must add up.
    ///
    /// Method: largest remainder. Every part first receives its share rounded toward zero. The
    /// minor units left over go one each to the parts with the largest remainders; ties go to the
    /// earlier part. Parts with weight zero receive nothing. Negative amounts are split like their
    /// absolute value, then negated, so allocation is symmetric.
    ///
    /// ```
    /// # use keel_types::{Currency, Money};
    /// let bill = Money::parse("10.00", Currency::USD)?;
    /// let parts = bill.allocate(&[1, 1, 1])?;
    /// let parts: Vec<String> = parts.iter().map(ToString::to_string).collect();
    /// assert_eq!(parts, ["USD 3.34", "USD 3.33", "USD 3.33"]);
    /// # Ok::<(), keel_types::MoneyError>(())
    /// ```
    ///
    /// # Errors
    /// [`MoneyError::EmptyAllocation`] if `weights` is empty, [`MoneyError::ZeroTotalWeight`] if
    /// all weights are zero, and [`MoneyError::Overflow`] if the weights' sum overflows.
    pub fn allocate(self, weights: &[u64]) -> Result<Vec<Money>, MoneyError> {
        if weights.is_empty() {
            return Err(MoneyError::EmptyAllocation);
        }
        let total_weight = weights
            .iter()
            .try_fold(0_u128, |sum, &weight| sum.checked_add(u128::from(weight)))
            .ok_or(MoneyError::Overflow)?;
        if total_weight == 0 {
            return Err(MoneyError::ZeroTotalWeight);
        }

        // |amount| ≤ 2^63 and weight < 2^64, so every product is below 2^127: it fits in a u128.
        let magnitude = u128::from(self.minor.unsigned_abs());
        let mut shares = Vec::with_capacity(weights.len());
        let mut remainders = Vec::with_capacity(weights.len());
        let mut distributed: u128 = 0;
        for (index, &weight) in weights.iter().enumerate() {
            let product = magnitude.checked_mul(u128::from(weight)).ok_or(MoneyError::Overflow)?;
            let share = product.checked_div(total_weight).ok_or(MoneyError::Overflow)?;
            let remainder = product.checked_rem(total_weight).ok_or(MoneyError::Overflow)?;
            distributed = distributed.checked_add(share).ok_or(MoneyError::Overflow)?;
            shares.push(share);
            remainders.push((remainder, index));
        }

        // The leftover equals Σ remainders / total_weight. Each remainder is below total_weight,
        // so at least `leftover` parts have a non-zero remainder: taking the first `leftover`
        // entries by descending remainder never reaches a zero-weight part.
        let leftover = magnitude.checked_sub(distributed).ok_or(MoneyError::Overflow)?;
        let leftover = usize::try_from(leftover).map_err(|_| MoneyError::Overflow)?;
        remainders.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(_, index) in remainders.iter().take(leftover) {
            if let Some(share) = shares.get_mut(index) {
                *share = share.checked_add(1).ok_or(MoneyError::Overflow)?;
            }
        }

        let negative = self.minor < 0;
        shares
            .into_iter()
            .map(|share| {
                let share = i128::try_from(share).map_err(|_| MoneyError::Overflow)?;
                let signed = if negative { share.checked_neg() } else { Some(share) };
                let minor = signed
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or(MoneyError::Overflow)?;
                Ok(Money { minor, currency: self.currency })
            })
            .collect()
    }

    fn minor_units(self) -> u32 {
        u32::from(self.currency.minor_units())
    }

    fn same_currency(self, other: Money) -> Result<Currency, MoneyError> {
        if self.currency == other.currency {
            Ok(self.currency)
        } else {
            Err(MoneyError::CurrencyMismatch { left: self.currency, right: other.currency })
        }
    }
}

impl fmt::Display for Money {
    /// Canonical, locale-independent form: `USD 12.34`, `USD -0.05`, `JPY 1200`, `KWD 1.250`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let minor_units = u32::from(self.currency.minor_units());
        let magnitude = self.minor.unsigned_abs();
        let divisor = 10_u64.checked_pow(minor_units).ok_or(fmt::Error)?;
        let integer = magnitude.checked_div(divisor).ok_or(fmt::Error)?;
        let fraction = magnitude.checked_rem(divisor).ok_or(fmt::Error)?;
        let sign = if self.minor < 0 { "-" } else { "" };
        write!(f, "{} {sign}{integer}", self.currency)?;
        if minor_units > 0 {
            let width = usize::try_from(minor_units).map_err(|_| fmt::Error)?;
            write!(f, ".{fraction:0width$}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Money({self})")
    }
}

/// Errors from money arithmetic, conversion and parsing.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MoneyError {
    /// Two amounts in different currencies were combined.
    #[error("currency mismatch: {left} and {right}")]
    CurrencyMismatch {
        /// The left-hand currency.
        left: Currency,
        /// The right-hand currency.
        right: Currency,
    },
    /// The result doesn't fit in the representable range.
    #[error("amount out of range")]
    Overflow,
    /// A value had more decimal places than the currency's minor unit allows.
    #[error("{value} has more decimal places than {currency} allows")]
    PrecisionExceeded {
        /// The value that couldn't be represented exactly.
        value: Decimal,
        /// The currency it was meant for.
        currency: Currency,
    },
    /// Text didn't match the strict amount format.
    #[error("invalid {currency} amount {text:?}")]
    InvalidFormat {
        /// The rejected text.
        text: String,
        /// The currency it was parsed for.
        currency: Currency,
    },
    /// An allocation was requested over no parts.
    #[error("cannot allocate over zero parts")]
    EmptyAllocation,
    /// An allocation was requested with weights that are all zero.
    #[error("allocation weights sum to zero")]
    ZeroTotalWeight,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(text: &str) -> Money {
        Money::parse(text, Currency::USD).unwrap()
    }

    fn dec(text: &str) -> Decimal {
        text.parse().unwrap()
    }

    #[test]
    fn display_per_minor_units() {
        assert_eq!(usd("12.34").to_string(), "USD 12.34");
        assert_eq!(usd("-0.05").to_string(), "USD -0.05");
        assert_eq!(usd("0").to_string(), "USD 0.00");
        assert_eq!(Money::from_minor(1200, Currency::JPY).to_string(), "JPY 1200");
        assert_eq!(Money::from_minor(1250, Currency::KWD).to_string(), "KWD 1.250");
        assert_eq!(
            Money::from_minor(i64::MIN, Currency::USD).to_string(),
            "USD -92233720368547758.08"
        );
        assert_eq!(format!("{:?}", usd("1.50")), "Money(USD 1.50)");
    }

    #[test]
    fn parse_accepts_the_strict_format() {
        assert_eq!(usd("12.34").minor(), 1234);
        assert_eq!(usd("12.3").minor(), 1230);
        assert_eq!(usd("12").minor(), 1200);
        assert_eq!(usd("-0.50").minor(), -50);
        assert_eq!(usd("0007.10").minor(), 710);
        assert_eq!(Money::parse("1200", Currency::JPY).unwrap().minor(), 1200);
        assert_eq!(Money::parse("1.250", Currency::KWD).unwrap().minor(), 1250);
        assert_eq!(usd("92233720368547758.07").minor(), i64::MAX);
        assert_eq!(usd("-92233720368547758.08").minor(), i64::MIN);
        assert_eq!(usd("-0").minor(), 0);
    }

    #[test]
    fn parse_rejects_everything_else() {
        for text in [
            "", "-", ".", "12.", ".5", "+1", " 1", "1 ", "1,000.00", "1e3", "12.345", "12.340",
            "--1", "1.2.3", "１２", "0x10", "NaN",
        ] {
            assert!(
                matches!(Money::parse(text, Currency::USD), Err(MoneyError::InvalidFormat { .. })),
                "{text:?} should be rejected"
            );
        }
        assert!(matches!(
            Money::parse("1.0", Currency::JPY),
            Err(MoneyError::InvalidFormat { .. })
        ));
        for out_of_range in
            ["92233720368547758.08", "-92233720368547758.09", "184467440737095516.16"]
        {
            assert_eq!(Money::parse(out_of_range, Currency::USD), Err(MoneyError::Overflow));
        }
    }

    #[test]
    fn arithmetic_is_checked() {
        assert_eq!(usd("1.10").checked_add(usd("2.25")), Ok(usd("3.35")));
        assert_eq!(usd("1.10").checked_sub(usd("2.25")), Ok(usd("-1.15")));
        assert_eq!(usd("1.10").checked_mul(3), Ok(usd("3.30")));
        assert_eq!(usd("-1.10").checked_abs(), Ok(usd("1.10")));
        assert_eq!(usd("1.10").checked_neg(), Ok(usd("-1.10")));
        let max = Money::from_minor(i64::MAX, Currency::USD);
        assert_eq!(max.checked_add(usd("0.01")), Err(MoneyError::Overflow));
        assert_eq!(
            Money::from_minor(i64::MIN, Currency::USD).checked_neg(),
            Err(MoneyError::Overflow)
        );
        assert_eq!(max.checked_mul(2), Err(MoneyError::Overflow));
    }

    #[test]
    fn currencies_never_mix() {
        let euros = Money::from_minor(100, Currency::EUR);
        let mismatch = MoneyError::CurrencyMismatch { left: Currency::USD, right: Currency::EUR };
        assert_eq!(usd("1.00").checked_add(euros), Err(mismatch.clone()));
        assert_eq!(usd("1.00").checked_sub(euros), Err(mismatch.clone()));
        assert_eq!(usd("1.00").compare(euros), Err(mismatch));
        assert!(Money::sum(Currency::USD, [usd("1.00"), euros]).is_err());
    }

    #[test]
    fn compare_and_sum() {
        assert_eq!(usd("1.00").compare(usd("2.00")), Ok(Ordering::Less));
        assert_eq!(Money::sum(Currency::USD, []), Ok(usd("0")));
        assert_eq!(
            Money::sum(Currency::USD, [usd("1.10"), usd("2.20"), usd("-0.30")]),
            Ok(usd("3.00"))
        );
        // Order never matters, even when a running total would leave the range.
        let max = Money::from_minor(i64::MAX, Currency::USD);
        let one = usd("0.01");
        let minus_one = usd("-0.01");
        assert_eq!(Money::sum(Currency::USD, [max, one, minus_one]), Ok(max));
        assert_eq!(Money::sum(Currency::USD, [max, minus_one, one]), Ok(max));
        assert_eq!(Money::sum(Currency::USD, [max, one]), Err(MoneyError::Overflow));
    }

    #[test]
    fn decimal_conversions() {
        assert_eq!(usd("12.34").to_decimal(), dec("12.34"));
        assert_eq!(Money::from_minor(1200, Currency::JPY).to_decimal(), dec("1200"));
        assert_eq!(
            Money::from_decimal(dec("12.345"), Currency::USD, RoundingMode::HalfAwayFromZero),
            Ok(usd("12.35"))
        );
        assert_eq!(
            Money::from_decimal(dec("12.345"), Currency::USD, RoundingMode::HalfEven),
            Ok(usd("12.34"))
        );
        assert_eq!(
            Money::from_decimal(dec("-12.345"), Currency::USD, RoundingMode::HalfAwayFromZero),
            Ok(usd("-12.35"))
        );
        assert_eq!(Money::from_decimal_exact(dec("12.3"), Currency::USD), Ok(usd("12.30")));
        assert_eq!(
            Money::from_decimal_exact(dec("12.345"), Currency::USD),
            Err(MoneyError::PrecisionExceeded { value: dec("12.345"), currency: Currency::USD })
        );
        assert_eq!(
            Money::from_decimal(dec("1e20"), Currency::USD, RoundingMode::HalfEven),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn multiplying_by_a_decimal_rounds_once() {
        // USD 12.99/kg × 0.453 kg = 5.88447 → 5.88.
        let per_kg = usd("12.99");
        assert_eq!(
            per_kg.mul_decimal(dec("0.453"), RoundingMode::HalfAwayFromZero),
            Ok(usd("5.88"))
        );
        // An 8.875% tax on USD 10.00 is 0.8875 → 0.89 (half away from zero) or 0.88 (half even).
        let net = usd("10.00");
        assert_eq!(
            net.mul_decimal(dec("0.08875"), RoundingMode::HalfAwayFromZero),
            Ok(usd("0.89"))
        );
        assert_eq!(net.mul_decimal(dec("0.08875"), RoundingMode::TowardZero), Ok(usd("0.88")));
    }

    #[test]
    fn cash_rounding_to_increment() {
        let nickel = RoundingRule::new(
            RoundingMode::HalfAwayFromZero,
            core::num::NonZeroU32::new(5).unwrap(),
        );
        let cad = |minor| Money::from_minor(minor, Currency::CAD);
        assert_eq!(cad(1012).round_to(nickel), Ok(cad(1010)));
        assert_eq!(cad(1013).round_to(nickel), Ok(cad(1015)));
        assert_eq!(cad(-1013).round_to(nickel), Ok(cad(-1015)));
    }

    #[test]
    fn allocation_reference_cases() {
        let parts = |amount: Money, weights: &[u64]| -> Vec<i64> {
            amount.allocate(weights).unwrap().into_iter().map(Money::minor).collect()
        };
        assert_eq!(parts(usd("10.00"), &[1, 1, 1]), [334, 333, 333]);
        assert_eq!(parts(usd("-10.00"), &[1, 1, 1]), [-334, -333, -333]);
        assert_eq!(parts(usd("0.05"), &[1, 1, 1]), [2, 2, 1]);
        assert_eq!(parts(usd("100.00"), &[70, 30]), [7000, 3000]);
        assert_eq!(parts(usd("0.01"), &[1, 1]), [1, 0]);
        assert_eq!(parts(usd("0.01"), &[1, 3]), [0, 1]);
        assert_eq!(parts(usd("1.00"), &[0, 1, 0]), [0, 100, 0]);
        assert_eq!(parts(usd("0"), &[1, 2]), [0, 0]);
        assert_eq!(parts(Money::from_minor(i64::MIN, Currency::USD), &[1]), [i64::MIN]);
        assert_eq!(
            parts(Money::from_minor(i64::MAX, Currency::USD), &[u64::MAX, u64::MAX]).len(),
            2
        );
    }

    #[test]
    fn allocation_errors() {
        assert_eq!(usd("1.00").allocate(&[]), Err(MoneyError::EmptyAllocation));
        assert_eq!(usd("1.00").allocate(&[0, 0]), Err(MoneyError::ZeroTotalWeight));
    }
}
