//! Locales: amounts and quantities as people read them, the same on every screen and receipt
//! (ADR-0023).
//!
//! The kernel formats what it shows, rather than each platform's libraries, whose symbols,
//! spaces and digits differ from one another and from what a printer prints. A [`Locale`]
//! follows CLDR's data for it (CLDR 48, `data/cldr/`), with one deliberate difference:
//!
//! - **The currency's symbol** comes first: `$1,234.56`, `€12.34`, `CA$5.00`. A currency without a
//!   symbol of its own in the locale shows its ISO 4217 code, and a symbol ending in a letter is
//!   kept apart from the digits by a no-break space, as CLDR's `alphaNextToNumber` pattern does:
//!   `EUR 12.34` in `es-US`.
//! - **Digits** are grouped by thousands from four digits on, with the locale's decimal and
//!   grouping symbols.
//! - **A negative amount** has the minus sign before the symbol: `-$0.05`.
//! - **Every minor unit is shown**, from the currency's ISO 4217 exponent: an amount reads exactly
//!   as it is held, never rounded for display. (CLDR's own digit counts differ from ISO's for a
//!   few currencies; the kernel never shows fewer digits than an amount has.)
//!
//! v0 knows `en-US` and `es-US`.
//!
//! ```
//! use keel_types::{Currency, Locale, Money};
//!
//! let total = Money::parse("1234.5", Currency::USD)?;
//! assert_eq!(Locale::EN_US.money(total), "$1,234.50");
//! let refund = Money::parse("-12.34", Currency::EUR)?;
//! assert_eq!(Locale::EN_US.money(refund), "-€12.34");
//! assert_eq!(Locale::ES_US.money(refund), "-EUR\u{a0}12.34");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use core::fmt;

use crate::money::Money;
use crate::quantity::Quantity;

// Generated data: one currency per line, so rustfmt leaves it alone.
#[rustfmt::skip]
mod table;

/// A locale Keel formats amounts and quantities for, by its BCP 47 tag.
#[derive(Clone, Copy)]
pub struct Locale(&'static LocaleData);

/// What a locale formats with, from CLDR.
pub(crate) struct LocaleData {
    tag: &'static str,
    decimal: char,
    group: char,
    minus: char,
    /// The currencies whose symbol in the locale isn't their ISO 4217 code, sorted by code.
    symbols: &'static [Symbol],
}

/// A currency's symbol in a locale.
pub(crate) struct Symbol {
    code: &'static str,
    text: &'static str,
    /// Whether a no-break space comes between the symbol and the digits: the symbol ends in a
    /// letter or other character that isn't a symbol, such as `EUR` or `Cg.`.
    spaced: bool,
}

/// The space between a symbol ending in a letter and the digits.
const NO_BREAK_SPACE: char = '\u{a0}';

/// A quantity's fraction digits: quantities are whole millionths of a unit.
const QUANTITY_DECIMALS: usize = 6;

impl Locale {
    /// American English.
    pub const EN_US: Locale = Locale(&table::EN_US);
    /// Spanish as spoken in the United States.
    pub const ES_US: Locale = Locale(&table::ES_US);

    /// Every locale Keel knows.
    pub const ALL: [Locale; 2] = [Locale::EN_US, Locale::ES_US];

    /// The locale for a BCP 47 tag, such as `en-US`, in any case.
    ///
    /// # Errors
    /// [`LocaleError::Unknown`] if Keel doesn't format for that locale.
    pub fn from_tag(tag: &str) -> Result<Locale, LocaleError> {
        Locale::ALL
            .into_iter()
            .find(|locale| locale.0.tag.eq_ignore_ascii_case(tag))
            .ok_or_else(|| LocaleError::Unknown(tag.to_owned()))
    }

    /// The locale's BCP 47 tag, such as `en-US`.
    pub const fn tag(self) -> &'static str {
        self.0.tag
    }

    /// `amount` as people read it in this locale, such as `$1,234.56`.
    pub fn money(self, amount: Money) -> String {
        let currency = amount.currency();
        let (symbol, spaced) = self.symbol(currency.code());
        let mut text = String::new();
        if amount.minor() < 0 {
            text.push(self.0.minus);
        }
        text.push_str(symbol);
        if spaced {
            text.push(NO_BREAK_SPACE);
        }
        self.digits(&mut text, amount.minor().unsigned_abs(), usize::from(currency.minor_units()));
        text
    }

    /// The number of `quantity`, without its unit, as people read it in this locale: whole
    /// quantities without decimals (`2`, `1,000`), others with as many as they need (`0.453`).
    pub fn quantity(self, quantity: Quantity) -> String {
        let mut text = String::new();
        if quantity.micros() < 0 {
            text.push(self.0.minus);
        }
        let micros = quantity.micros().unsigned_abs();
        // Drop the fraction's trailing zeros: 1.500000 has one decimal, 2.000000 none.
        let mut decimals = QUANTITY_DECIMALS;
        let mut value = micros;
        while decimals > 0 && value.is_multiple_of(10) {
            value /= 10;
            decimals = decimals.saturating_sub(1);
        }
        self.digits(&mut text, value, decimals);
        text
    }

    /// The symbol of the currency with ISO 4217 code `code`, and whether a no-break space
    /// follows it: its own symbol in this locale, or else its code.
    fn symbol(self, code: &'static str) -> (&'static str, bool) {
        self.0
            .symbols
            .binary_search_by(|symbol| symbol.code.cmp(code))
            .ok()
            .and_then(|index| self.0.symbols.get(index))
            .map_or((code, true), |symbol| (symbol.text, symbol.spaced))
    }

    /// Appends `value`, a whole number of units of the last of `decimals` decimal places, to
    /// `text`: its integer digits grouped by thousands, then the decimal symbol and the fraction
    /// if it has decimals.
    fn digits(self, text: &mut String, value: u64, decimals: usize) {
        // Pad to at least one integer digit: 5 cents is 0.05.
        let padded = format!("{value:0>width$}", width = decimals.saturating_add(1));
        let split = padded.len().saturating_sub(decimals);
        let (integer, fraction) = padded.split_at_checked(split).unwrap_or((&padded, ""));
        for (index, digit) in integer.chars().enumerate() {
            let left = integer.len().saturating_sub(index);
            if index > 0 && left.is_multiple_of(3) {
                text.push(self.0.group);
            }
            text.push(digit);
        }
        if !fraction.is_empty() {
            text.push(self.0.decimal);
            text.push_str(fraction);
        }
    }
}

impl PartialEq for Locale {
    fn eq(&self, other: &Locale) -> bool {
        self.0.tag == other.0.tag
    }
}

impl Eq for Locale {}

impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Locale({})", self.0.tag)
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.tag)
    }
}

/// Errors from looking up a locale.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LocaleError {
    /// Keel doesn't format for this locale.
    #[error("unknown locale {0:?}")]
    Unknown(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency::Currency;
    use crate::quantity::Unit;

    fn money(text: &str, code: &str) -> Money {
        Money::parse(text, Currency::from_code(code).unwrap()).unwrap()
    }

    #[test]
    fn amounts_read_as_cldr_says() {
        let cases = [
            (Locale::EN_US, "0", "USD", "$0.00"),
            (Locale::EN_US, "0.05", "USD", "$0.05"),
            (Locale::EN_US, "-0.05", "USD", "-$0.05"),
            (Locale::EN_US, "999.99", "USD", "$999.99"),
            (Locale::EN_US, "1000", "USD", "$1,000.00"),
            (Locale::EN_US, "1234567.89", "USD", "$1,234,567.89"),
            (Locale::EN_US, "-1234567.89", "USD", "-$1,234,567.89"),
            (Locale::EN_US, "12.34", "EUR", "€12.34"),
            (Locale::EN_US, "5", "CAD", "CA$5.00"),
            (Locale::EN_US, "5", "MXN", "MX$5.00"),
            (Locale::EN_US, "1200", "JPY", "¥1,200"),
            (Locale::EN_US, "1.25", "KWD", "KWD\u{a0}1.250"),
            (Locale::EN_US, "-1.25", "KWD", "-KWD\u{a0}1.250"),
            (Locale::EN_US, "7", "XOF", "F\u{202f}CFA\u{a0}7"),
            (Locale::ES_US, "1234.5", "USD", "$1,234.50"),
            (Locale::ES_US, "-0.01", "USD", "-$0.01"),
            (Locale::ES_US, "12.34", "EUR", "EUR\u{a0}12.34"),
            (Locale::ES_US, "5", "MXN", "MXN\u{a0}5.00"),
            (Locale::ES_US, "1200", "JPY", "¥1,200"),
        ];
        for (locale, amount, code, expected) in cases {
            assert_eq!(locale.money(money(amount, code)), expected, "{locale} {amount} {code}");
        }
    }

    #[test]
    fn the_largest_amounts_read_in_full() {
        let usd = Currency::USD;
        assert_eq!(
            Locale::EN_US.money(Money::from_minor(i64::MAX, usd)),
            "$92,233,720,368,547,758.07"
        );
        assert_eq!(
            Locale::EN_US.money(Money::from_minor(i64::MIN, usd)),
            "-$92,233,720,368,547,758.08"
        );
        let jpy = Currency::JPY;
        assert_eq!(
            Locale::EN_US.money(Money::from_minor(i64::MIN, jpy)),
            "-¥9,223,372,036,854,775,808"
        );
    }

    #[test]
    fn quantities_read_with_the_decimals_they_need() {
        let cases = [
            (0, "0"),
            (2_000_000, "2"),
            (1_500_000, "1.5"),
            (453_000, "0.453"),
            (1, "0.000001"),
            (-250_000, "-0.25"),
            (1_000_000_000, "1,000"),
            (1_234_567_890_000, "1,234,567.89"),
            (i64::MIN, "-9,223,372,036,854.775808"),
        ];
        for (micros, expected) in cases {
            let quantity = Quantity::from_micros(micros, Unit::Kilogram);
            assert_eq!(Locale::EN_US.quantity(quantity), expected, "{micros}");
            assert_eq!(Locale::ES_US.quantity(quantity), expected, "{micros}");
        }
    }

    #[test]
    fn locales_are_found_by_their_tags_in_any_case() {
        assert_eq!(Locale::from_tag("en-US"), Ok(Locale::EN_US));
        assert_eq!(Locale::from_tag("EN-us"), Ok(Locale::EN_US));
        assert_eq!(Locale::from_tag("es-US"), Ok(Locale::ES_US));
        for unknown in ["en", "es", "en-GB", "es-MX", "", "en_US", "en-US-x"] {
            assert_eq!(Locale::from_tag(unknown), Err(LocaleError::Unknown(unknown.to_owned())));
        }
        assert_eq!(Locale::EN_US.to_string(), "en-US");
        assert_eq!(format!("{:?}", Locale::ES_US), "Locale(es-US)");
    }

    #[test]
    fn each_table_is_sorted_so_lookups_find_every_symbol() {
        for locale in Locale::ALL {
            let codes: Vec<&str> = locale.0.symbols.iter().map(|symbol| symbol.code).collect();
            let mut sorted = codes.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(codes, sorted, "{locale}");
            for symbol in locale.0.symbols {
                assert_eq!(locale.symbol(symbol.code), (symbol.text, symbol.spaced));
            }
        }
    }
}
