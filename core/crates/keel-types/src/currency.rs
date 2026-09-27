//! ISO 4217 currencies.
//!
//! A [`Currency`] can only be obtained from Keel's built-in, verified ISO 4217 table: through
//! [`Currency::from_code`], [`Currency::from_numeric`], or constants such as [`Currency::USD`].
//! A given code therefore always carries the same number of minor units, on every device.
//!
//! The table holds every circulating ISO 4217 currency. Funds, precious metals, bond market units
//! and testing codes are excluded. It also holds currencies withdrawn since 2020, flagged as
//! [`CurrencyStatus::Withdrawn`], so recent history can still be imported. See
//! `data/iso4217/README.md` for provenance and verification.
//!
//! Minor units follow ISO 4217. Some payment processors represent a few currencies differently
//! (for example with zero decimals where ISO specifies two). That translation belongs in the
//! payment connectors, never here.
//!
//! Note that `Currency::ALL` is the Albanian lek (ISO code `ALL`). To enumerate every currency,
//! use [`Currency::known`].

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};

// Generated data: one currency per line, so rustfmt leaves it alone.
#[rustfmt::skip]
mod table;

/// A currency from the ISO 4217 table: its alphabetic code and number of minor units.
///
/// Equality, ordering and hashing use the alphabetic code.
#[derive(Clone, Copy)]
pub struct Currency(&'static CurrencyInfo);

impl Currency {
    /// Looks up a currency by its ISO 4217 alphabetic code, such as `"EUR"`.
    ///
    /// Codes are matched exactly and must be uppercase.
    ///
    /// # Errors
    /// Returns [`CurrencyError::UnknownCode`] if the code isn't in the table.
    pub fn from_code(code: &str) -> Result<Currency, CurrencyError> {
        table::TABLE
            .binary_search_by(|info| info.code.cmp(code))
            .ok()
            .and_then(|index| table::TABLE.get(index))
            .map(|info| Currency(info))
            .ok_or_else(|| CurrencyError::UnknownCode(code.to_owned()))
    }

    /// Looks up a currency by its ISO 4217 numeric code, such as `978` for the euro.
    ///
    /// Payment terminals and EMV data identify currencies by numeric code. When a numeric code
    /// has been reused (for example 532, now the Caribbean guilder and formerly the Netherlands
    /// Antillean guilder), the circulating currency wins.
    pub fn from_numeric(numeric: u16) -> Option<Currency> {
        let mut withdrawn_match = None;
        for info in table::TABLE {
            if info.numeric == numeric {
                if info.status == CurrencyStatus::Circulating {
                    return Some(Currency(info));
                }
                withdrawn_match = Some(Currency(info));
            }
        }
        withdrawn_match
    }

    /// Every currency in the table, circulating and withdrawn, in alphabetic-code order.
    pub fn known() -> impl Iterator<Item = Currency> {
        table::TABLE.iter().map(|info| Currency(info))
    }

    /// The ISO 4217 alphabetic code, such as `"USD"`.
    pub const fn code(self) -> &'static str {
        self.0.code
    }

    /// The ISO 4217 numeric code, such as `840` for the US dollar.
    pub const fn numeric(self) -> u16 {
        self.0.numeric
    }

    /// The number of decimal places in the minor unit: 2 for USD (cents), 0 for JPY, 3 for KWD.
    pub const fn minor_units(self) -> u8 {
        self.0.minor_units
    }

    /// The ISO 4217 currency name, such as `"US Dollar"`.
    pub const fn name(self) -> &'static str {
        self.0.name
    }

    /// Whether the currency is circulating or has been withdrawn.
    pub const fn status(self) -> CurrencyStatus {
        self.0.status
    }

    /// Whether the currency is currently circulating. New configuration (for example a new
    /// location's currency) must only use circulating currencies.
    pub const fn is_circulating(self) -> bool {
        matches!(self.0.status, CurrencyStatus::Circulating)
    }

    /// The full table entry for this currency.
    pub const fn info(self) -> &'static CurrencyInfo {
        self.0
    }
}

impl PartialEq for Currency {
    fn eq(&self, other: &Self) -> bool {
        self.0.code == other.0.code
    }
}

impl Eq for Currency {}

impl Hash for Currency {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.code.hash(state);
    }
}

impl PartialOrd for Currency {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Currency {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.code.cmp(other.0.code)
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.code)
    }
}

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Currency({})", self.0.code)
    }
}

/// The most minor units a currency may have. ISO 4217 uses 0, 2 and 3 for currencies (and 4 for
/// one unit of account, which Keel excludes). Code converting minor units to decimals relies on
/// this bound.
pub(crate) const MAX_MINOR_UNITS: u8 = 4;

/// One entry of the ISO 4217 table.
#[derive(Debug, PartialEq, Eq)]
pub struct CurrencyInfo {
    code: &'static str,
    numeric: u16,
    minor_units: u8,
    name: &'static str,
    status: CurrencyStatus,
}

impl CurrencyInfo {
    /// Only called to initialize the table's statics, so the assertion runs at compile time:
    /// an out-of-range entry is a build error, never a runtime panic.
    const fn new(
        code: &'static str,
        numeric: u16,
        minor_units: u8,
        name: &'static str,
        status: CurrencyStatus,
    ) -> Self {
        assert!(minor_units <= MAX_MINOR_UNITS, "minor units out of range");
        Self { code, numeric, minor_units, name, status }
    }

    /// The ISO 4217 alphabetic code.
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// The ISO 4217 numeric code.
    pub const fn numeric(&self) -> u16 {
        self.numeric
    }

    /// The number of decimal places in the minor unit.
    pub const fn minor_units(&self) -> u8 {
        self.minor_units
    }

    /// The ISO 4217 currency name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Whether the currency is circulating or withdrawn.
    pub const fn status(&self) -> CurrencyStatus {
        self.status
    }
}

/// Whether a currency is in circulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CurrencyStatus {
    /// Legal tender today.
    Circulating,
    /// Withdrawn from ISO 4217 list one. Kept so recent history remains importable.
    Withdrawn {
        /// Year of withdrawal.
        year: u16,
        /// Month of withdrawal, 1–12.
        month: u8,
    },
}

/// Errors from currency lookups.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CurrencyError {
    /// The code isn't an ISO 4217 currency known to Keel.
    #[error("unknown currency code {0:?}")]
    UnknownCode(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_minor_units() {
        let expectations = [
            ("USD", 2),
            ("EUR", 2),
            ("GBP", 2),
            ("JPY", 0),
            ("KRW", 0),
            ("VND", 0),
            ("CLP", 0),
            ("ISK", 0),
            ("XOF", 0),
            ("XAF", 0),
            ("XPF", 0),
            ("KWD", 3),
            ("BHD", 3),
            ("OMR", 3),
            ("JOD", 3),
            ("TND", 3),
            ("IQD", 3),
            ("LYD", 3),
        ];
        for (code, minor_units) in expectations {
            let currency = Currency::from_code(code).unwrap();
            assert_eq!(currency.minor_units(), minor_units, "{code}");
            assert_eq!(currency.code(), code);
        }
    }

    #[test]
    fn constants_match_lookup() {
        assert_eq!(Currency::USD, Currency::from_code("USD").unwrap());
        assert_eq!(Currency::USD.numeric(), 840);
        assert_eq!(Currency::EUR.numeric(), 978);
        assert_eq!(Currency::JPY.minor_units(), 0);
        assert_eq!(Currency::ALL.name(), "Lek");
    }

    #[test]
    fn unknown_and_malformed_codes_are_rejected() {
        for code in ["", "usd", "US", "USDX", "XXX", "XAU", "CLF", "ABC"] {
            assert_eq!(
                Currency::from_code(code),
                Err(CurrencyError::UnknownCode(code.to_owned())),
                "{code:?}"
            );
        }
    }

    #[test]
    fn numeric_lookup_prefers_circulating_currency() {
        assert_eq!(Currency::from_numeric(978), Some(Currency::EUR));
        // 532 was the Netherlands Antillean guilder and is now the Caribbean guilder.
        assert_eq!(Currency::from_numeric(532), Some(Currency::XCG));
        // A withdrawn currency with no successor on its numeric code is still found.
        assert_eq!(Currency::from_numeric(191), Some(Currency::HRK));
        assert_eq!(Currency::from_numeric(0), None);
    }

    #[test]
    fn withdrawn_currencies_are_flagged() {
        assert!(!Currency::BGN.is_circulating());
        assert_eq!(Currency::BGN.status(), CurrencyStatus::Withdrawn { year: 2026, month: 1 });
        assert!(Currency::XCG.is_circulating());
    }

    #[test]
    fn display_and_debug() {
        assert_eq!(Currency::KWD.to_string(), "KWD");
        assert_eq!(format!("{:?}", Currency::KWD), "Currency(KWD)");
    }
}
