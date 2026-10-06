//! # keel-types
//!
//! The value types every other part of Keel is built from: money and currencies, rounding,
//! rates, quantities and units, identifiers, timestamps, hybrid logical clocks, business dates,
//! and the locales amounts are shown in.
//!
//! This crate is the bottom of the kernel, and everything above it handles real money, so its
//! rules are strict:
//!
//! - **Exact arithmetic.** Amounts are integers in a currency's minor unit, and quantities are
//!   integers in millionths of a unit. Rates are exact decimals. Floating point is banned by lint.
//! - **No hidden failure.** Operations that can overflow, or mix currencies or units, return a
//!   `Result`. Nothing in this crate panics, and the lints enforce it.
//! - **Rounding only on purpose.** Every rounding step takes an explicit [`RoundingMode`] or
//!   [`RoundingRule`], and rounds the exact value exactly once.
//! - **Deterministic.** Nothing here reads the system clock or random number generator by
//!   itself: time comes from a [`Clock`] and randomness from an [`Entropy`] source, both
//!   injected, and time zone rules come from the IANA database bundled into the crate. The same
//!   inputs give the same outputs on every device, the hub and the cloud, and in simulation.
//!
//! ```
//! use keel_types::{Currency, Money, Quantity, Rate, RoundingMode, Unit};
//!
//! // 0.453 kg of cheese at USD 12.99 per kilogram, plus 8.875% sales tax, split three ways.
//! let per_kg = Money::parse("12.99", Currency::USD)?;
//! let weight = Quantity::parse("0.453", Unit::Kilogram)?;
//! let price = per_kg.mul_decimal(weight.to_decimal(), RoundingMode::HalfAwayFromZero)?;
//! assert_eq!(price.to_string(), "USD 5.88");
//!
//! let tax_rate = Rate::from_percent("8.875".parse()?)?;
//! let tax = tax_rate.apply(price, RoundingMode::HalfAwayFromZero)?;
//! let total = price.checked_add(tax)?;
//! assert_eq!(total.to_string(), "USD 6.40");
//!
//! let shares: Vec<String> = total.allocate(&[1, 1, 1])?.iter().map(Money::to_string).collect();
//! assert_eq!(shares, ["USD 2.14", "USD 2.13", "USD 2.13"]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! See `docs/architecture/domain-model.md` §4 for how these types are used.

pub mod business_date;
pub mod currency;
mod fixed_point;
pub mod hlc;
pub mod id;
pub mod locale;
pub mod money;
pub mod quantity;
pub mod rate;
pub mod rounding;
pub mod time;

pub use business_date::{BusinessDate, BusinessDateError, BusinessDayPolicy};
pub use currency::{Currency, CurrencyError, CurrencyInfo, CurrencyStatus};
pub use hlc::{Hlc, HlcClock, HlcError};
#[cfg(feature = "os")]
pub use id::OsEntropy;
pub use id::{Entropy, EntropyError, Id, IdError, IdGenerator, SeededEntropy};
pub use locale::{Locale, LocaleError};
pub use money::{Money, MoneyError};
pub use quantity::{Dimension, Quantity, QuantityError, Unit};
pub use rate::{Rate, RateError};
pub use rounding::{RoundingMode, RoundingRule};
pub use rust_decimal::Decimal;
#[cfg(feature = "os")]
pub use time::SystemClock;
pub use time::{Clock, ManualClock, Timestamp, TimestampError};
