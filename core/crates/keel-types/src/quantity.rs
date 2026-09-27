//! Quantities: exact amounts of something measurable, in a unit of measure.
//!
//! A [`Quantity`] is, like [`Money`](crate::Money), a fixed-point integer: a whole number of
//! millionths of its [`Unit`]. 1.235 kg is 1,235,000 millionths of a kilogram. Arithmetic is
//! exact and checked, and conversions between units round once, with an explicit mode.
//!
//! Every [`Unit`] has an exact definition in its dimension's base unit. The pound, for example,
//! is exactly 453.59237 g, by the international yard and pound agreement of 1959. Item-specific
//! "units" such as a case of 24 or a 750 ml bottle are not units of measure: they belong to the
//! catalog, as packaging.

use core::cmp::Ordering;
use core::fmt;
use core::num::NonZeroU32;

use rust_decimal::Decimal;

use crate::fixed_point::{self, FixedPointError};
use crate::rounding::RoundingMode;

/// What a quantity measures. Quantities convert only within a dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dimension {
    /// Discrete items: each, dozen.
    Count,
    /// Mass (what a scale calls weight): grams, pounds.
    Mass,
    /// Volume: litres, fluid ounces.
    Volume,
    /// Length: metres, feet.
    Length,
    /// Area: square metres, square feet.
    Area,
    /// Time, for rentals, services and billing: hours, minutes.
    Duration,
}

impl Dimension {
    /// Every dimension.
    pub const ALL: [Dimension; 6] = [
        Dimension::Count,
        Dimension::Mass,
        Dimension::Volume,
        Dimension::Length,
        Dimension::Area,
        Dimension::Duration,
    ];

    /// The unit that every unit of this dimension is defined in.
    pub const fn base_unit(self) -> Unit {
        match self {
            Dimension::Count => Unit::Each,
            Dimension::Mass => Unit::Gram,
            Dimension::Volume => Unit::Millilitre,
            Dimension::Length => Unit::Millimetre,
            Dimension::Area => Unit::SquareMillimetre,
            Dimension::Duration => Unit::Second,
        }
    }
}

/// Declares the unit table. Each line is the single source of truth for one unit: its variant,
/// dimension, stable code, name, and exact size in the dimension's base unit.
macro_rules! units {
    ($(
        $(#[$attribute:meta])*
        $variant:ident: $dimension:ident, $code:literal, $name:literal, $mantissa:literal / 10^$scale:literal;
    )*) => {
        /// A unit of measure, exactly defined in its dimension's base unit.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Unit {
            $($(#[$attribute])* $variant,)*
        }

        // Every definition must be a valid `Decimal`, which allows at most 28 decimal places.
        // Checked when compiling, so `Unit::size_in_base_unit` can never panic.
        const _: () = { $(assert!($scale <= 28, "unit definition has too many decimal places");)* };

        impl Unit {
            /// Every unit, in table order.
            pub const ALL: &'static [Unit] = &[$(Unit::$variant,)*];

            /// The stable, machine-readable code, such as `"kg"` or `"fl_oz_us"`. Codes are
            /// stored in events, so they never change.
            pub const fn code(self) -> &'static str {
                match self { $(Unit::$variant => $code,)* }
            }

            /// The English name, such as `"kilogram"`.
            pub const fn name(self) -> &'static str {
                match self { $(Unit::$variant => $name,)* }
            }

            /// What the unit measures.
            pub const fn dimension(self) -> Dimension {
                match self { $(Unit::$variant => Dimension::$dimension,)* }
            }

            /// The unit's exact size in its dimension's base unit, as `(mantissa, scale)`:
            /// `mantissa × 10^-scale` base units.
            const fn definition(self) -> (u64, u32) {
                match self { $(Unit::$variant => ($mantissa, $scale),)* }
            }
        }
    };
}

units! {
    // Count. Base unit: each.
    /// One item.
    Each: Count, "each", "each", 1 / 10^0;
    /// Twelve items.
    Dozen: Count, "dozen", "dozen", 12 / 10^0;

    // Mass. Base unit: gram. Avoirdupois and troy units follow the 1959 international
    // definitions: 1 lb = 453.59237 g, and 1 grain = 64.79891 mg.
    /// 0.001 g.
    Milligram: Mass, "mg", "milligram", 1 / 10^3;
    /// The base unit of mass.
    Gram: Mass, "g", "gram", 1 / 10^0;
    /// 1000 g.
    Kilogram: Mass, "kg", "kilogram", 1000 / 10^0;
    /// The metric ton: 1000 kg.
    Tonne: Mass, "t", "tonne", 1_000_000 / 10^0;
    /// The metric carat, for gemstones: 200 mg.
    Carat: Mass, "ct", "carat", 2 / 10^1;
    /// The avoirdupois ounce: 1/16 lb, 28.349523125 g.
    Ounce: Mass, "oz", "ounce", 28_349_523_125 / 10^9;
    /// The avoirdupois pound: 453.59237 g.
    Pound: Mass, "lb", "pound", 45_359_237 / 10^5;
    /// The US (short) ton: 2000 lb.
    ShortTon: Mass, "ton_us", "US ton", 90_718_474 / 10^2;
    /// The troy ounce, for precious metals: 480 grains, 31.1034768 g.
    TroyOunce: Mass, "oz_t", "troy ounce", 311_034_768 / 10^7;
    /// The pennyweight, for precious metals: 24 grains, 1.55517384 g.
    Pennyweight: Mass, "dwt", "pennyweight", 155_517_384 / 10^8;

    // Volume. Base unit: millilitre. US units derive from the gallon of 231 cubic inches;
    // imperial units from the gallon of 4.54609 litres.
    /// The base unit of volume.
    Millilitre: Volume, "ml", "millilitre", 1 / 10^0;
    /// 10 ml.
    Centilitre: Volume, "cl", "centilitre", 10 / 10^0;
    /// 100 ml.
    Decilitre: Volume, "dl", "decilitre", 100 / 10^0;
    /// 1000 ml.
    Litre: Volume, "l", "litre", 1000 / 10^0;
    /// 1000 l.
    CubicMetre: Volume, "m3", "cubic metre", 1_000_000 / 10^0;
    /// 1/128 US gallon: 29.5735295625 ml.
    UsFluidOunce: Volume, "fl_oz_us", "US fluid ounce", 295_735_295_625 / 10^10;
    /// 1/8 US gallon: 473.176473 ml.
    UsPint: Volume, "pt_us", "US pint", 473_176_473 / 10^6;
    /// 1/4 US gallon: 946.352946 ml.
    UsQuart: Volume, "qt_us", "US quart", 946_352_946 / 10^6;
    /// 231 cubic inches: 3785.411784 ml.
    UsGallon: Volume, "gal_us", "US gallon", 3_785_411_784 / 10^6;
    /// 1/160 imperial gallon: 28.4130625 ml.
    ImperialFluidOunce: Volume, "fl_oz_imp", "imperial fluid ounce", 284_130_625 / 10^7;
    /// 1/8 imperial gallon: 568.26125 ml.
    ImperialPint: Volume, "pt_imp", "imperial pint", 56_826_125 / 10^5;
    /// 1/4 imperial gallon: 1136.5225 ml.
    ImperialQuart: Volume, "qt_imp", "imperial quart", 11_365_225 / 10^4;
    /// 4.54609 l.
    ImperialGallon: Volume, "gal_imp", "imperial gallon", 454_609 / 10^2;
    /// (2.54 cm)³: 16.387064 ml.
    CubicInch: Volume, "in3", "cubic inch", 16_387_064 / 10^6;
    /// 1728 cubic inches: 28316.846592 ml.
    CubicFoot: Volume, "ft3", "cubic foot", 28_316_846_592 / 10^6;
    /// 27 cubic feet: 764554.857984 ml.
    CubicYard: Volume, "yd3", "cubic yard", 764_554_857_984 / 10^6;

    // Length. Base unit: millimetre. 1 in = 25.4 mm.
    /// The base unit of length.
    Millimetre: Length, "mm", "millimetre", 1 / 10^0;
    /// 10 mm.
    Centimetre: Length, "cm", "centimetre", 10 / 10^0;
    /// 1000 mm.
    Metre: Length, "m", "metre", 1000 / 10^0;
    /// 1000 m.
    Kilometre: Length, "km", "kilometre", 1_000_000 / 10^0;
    /// 25.4 mm.
    Inch: Length, "in", "inch", 254 / 10^1;
    /// 12 in: 304.8 mm.
    Foot: Length, "ft", "foot", 3048 / 10^1;
    /// 3 ft: 914.4 mm.
    Yard: Length, "yd", "yard", 9144 / 10^1;
    /// 5280 ft: 1609.344 m.
    Mile: Length, "mi", "mile", 1_609_344 / 10^0;

    // Area. Base unit: square millimetre.
    /// The base unit of area.
    SquareMillimetre: Area, "mm2", "square millimetre", 1 / 10^0;
    /// 100 mm².
    SquareCentimetre: Area, "cm2", "square centimetre", 100 / 10^0;
    /// 1,000,000 mm².
    SquareMetre: Area, "m2", "square metre", 1_000_000 / 10^0;
    /// 645.16 mm².
    SquareInch: Area, "in2", "square inch", 64_516 / 10^2;
    /// 144 in²: 92903.04 mm².
    SquareFoot: Area, "ft2", "square foot", 9_290_304 / 10^2;
    /// 9 ft²: 836127.36 mm².
    SquareYard: Area, "yd2", "square yard", 83_612_736 / 10^2;

    // Duration. Base unit: second. A day is 24 hours: a duration, not a calendar day.
    /// 0.001 s.
    Millisecond: Duration, "ms", "millisecond", 1 / 10^3;
    /// The base unit of time.
    Second: Duration, "s", "second", 1 / 10^0;
    /// 60 s.
    Minute: Duration, "min", "minute", 60 / 10^0;
    /// 60 min.
    Hour: Duration, "h", "hour", 3600 / 10^0;
    /// 24 h.
    Day: Duration, "d", "day", 86_400 / 10^0;
}

impl Unit {
    /// Looks up a unit by its [code](Unit::code), such as `"kg"`.
    ///
    /// # Errors
    /// [`QuantityError::UnknownUnit`] if no unit has that code.
    pub fn from_code(code: &str) -> Result<Unit, QuantityError> {
        Unit::ALL
            .iter()
            .copied()
            .find(|unit| unit.code() == code)
            .ok_or_else(|| QuantityError::UnknownUnit(code.to_owned()))
    }

    /// The unit's exact size in its dimension's base unit: 453.59237 for the pound (in grams).
    pub fn size_in_base_unit(self) -> Decimal {
        let (mantissa, scale) = self.definition();
        // Cannot panic: every scale is at most 28 (checked when compiling the table).
        Decimal::from_i128_with_scale(i128::from(mantissa), scale)
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Millionths per unit: quantities have six decimal places.
const MICROS_PER_UNIT: i64 = 1_000_000;

/// An exact quantity: a whole number of millionths of a [`Unit`].
///
/// 1.235 kg is 1,235,000 millionths of a kilogram; 3 each is 3,000,000 millionths of an item.
/// Six decimal places is finer than scales, dispensers and recipes need, and keeps every
/// quantity an integer: arithmetic is exact and checked, and the value is the same on every
/// platform.
///
/// Like [`Money`](crate::Money), quantities have no arithmetic operators and no `PartialOrd`, so
/// every operation is visibly checked, and units never mix: combining two quantities requires
/// the same unit. [`Quantity::convert_to`] changes units.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quantity {
    micros: i64,
    unit: Unit,
}

impl Quantity {
    /// The number of decimal places of every quantity.
    pub const DECIMAL_PLACES: u32 = 6;

    /// A quantity from a number of millionths of `unit`.
    pub const fn from_micros(micros: i64, unit: Unit) -> Quantity {
        Quantity { micros, unit }
    }

    /// Zero of `unit`.
    pub const fn zero(unit: Unit) -> Quantity {
        Quantity { micros: 0, unit }
    }

    /// A whole number of `unit`: 3 each, 2 kg.
    ///
    /// # Errors
    /// [`QuantityError::Overflow`] if the quantity is out of range (above about 9.2 trillion).
    pub fn from_whole(value: i64, unit: Unit) -> Result<Quantity, QuantityError> {
        let micros = value.checked_mul(MICROS_PER_UNIT).ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, unit })
    }

    /// The quantity in millionths of its unit.
    pub const fn micros(self) -> i64 {
        self.micros
    }

    /// The unit.
    pub const fn unit(self) -> Unit {
        self.unit
    }

    /// What the quantity measures.
    pub const fn dimension(self) -> Dimension {
        self.unit.dimension()
    }

    /// Whether the quantity is zero.
    pub const fn is_zero(self) -> bool {
        self.micros == 0
    }

    /// Whether the quantity is greater than zero.
    pub const fn is_positive(self) -> bool {
        self.micros > 0
    }

    /// Whether the quantity is less than zero.
    pub const fn is_negative(self) -> bool {
        self.micros < 0
    }

    /// `self + rhs`.
    ///
    /// # Errors
    /// [`QuantityError::UnitMismatch`] if the units differ, [`QuantityError::Overflow`] if the
    /// result is out of range.
    pub fn checked_add(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        let unit = self.same_unit(rhs)?;
        let micros = self.micros.checked_add(rhs.micros).ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, unit })
    }

    /// `self - rhs`.
    ///
    /// # Errors
    /// [`QuantityError::UnitMismatch`] if the units differ, [`QuantityError::Overflow`] if the
    /// result is out of range.
    pub fn checked_sub(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        let unit = self.same_unit(rhs)?;
        let micros = self.micros.checked_sub(rhs.micros).ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, unit })
    }

    /// `-self`.
    ///
    /// # Errors
    /// [`QuantityError::Overflow`] for the one quantity whose negation is out of range.
    pub fn checked_neg(self) -> Result<Quantity, QuantityError> {
        let micros = self.micros.checked_neg().ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, ..self })
    }

    /// `self × factor`, for whole-number multiples (three 250 g packs).
    ///
    /// # Errors
    /// [`QuantityError::Overflow`] if the result is out of range.
    pub fn checked_mul(self, factor: i64) -> Result<Quantity, QuantityError> {
        let micros = self.micros.checked_mul(factor).ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, ..self })
    }

    /// `self × factor`, rounded to the nearest millionth with `mode`. The product is computed
    /// exactly, then rounded once.
    ///
    /// # Errors
    /// [`QuantityError::Overflow`] if the result is out of range.
    pub fn mul_decimal(
        self,
        factor: Decimal,
        mode: RoundingMode,
    ) -> Result<Quantity, QuantityError> {
        let micros = mode.round_product(self.micros, factor).ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, ..self })
    }

    /// Compares two quantities in the same unit.
    ///
    /// # Errors
    /// [`QuantityError::UnitMismatch`] if the units differ.
    pub fn compare(self, other: Quantity) -> Result<Ordering, QuantityError> {
        self.same_unit(other)?;
        Ok(self.micros.cmp(&other.micros))
    }

    /// The sum of `quantities`, all of which must be in `unit`. An empty sum is zero.
    ///
    /// The sum is exact and doesn't depend on the order of `quantities`: running totals may
    /// leave the representable range, as long as the final total is back in it.
    ///
    /// # Errors
    /// [`QuantityError::UnitMismatch`] if any quantity is in another unit,
    /// [`QuantityError::Overflow`] if the sum is out of range.
    pub fn sum<I>(unit: Unit, quantities: I) -> Result<Quantity, QuantityError>
    where
        I: IntoIterator<Item = Quantity>,
    {
        let mut total: i128 = 0;
        for quantity in quantities {
            if quantity.unit != unit {
                return Err(QuantityError::UnitMismatch { left: unit, right: quantity.unit });
            }
            // Can't overflow before 2^64 quantities; checked regardless.
            total =
                total.checked_add(i128::from(quantity.micros)).ok_or(QuantityError::Overflow)?;
        }
        let micros = i64::try_from(total).map_err(|_| QuantityError::Overflow)?;
        Ok(Quantity { micros, unit })
    }

    /// Converts to `unit`, rounding once to the nearest millionth with `mode`.
    ///
    /// The conversion is computed exactly from the units' definitions, then rounded:
    /// 1 kg is 2.2046226218… lb, so it converts to 2.204623 lb rounding to the nearest.
    ///
    /// # Errors
    /// [`QuantityError::DimensionMismatch`] if `unit` measures something else,
    /// [`QuantityError::Overflow`] if the result is out of range.
    pub fn convert_to(self, unit: Unit, mode: RoundingMode) -> Result<Quantity, QuantityError> {
        if self.unit.dimension() != unit.dimension() {
            return Err(QuantityError::DimensionMismatch { from: self.unit, to: unit });
        }
        if self.unit == unit {
            return Ok(self);
        }
        // micros × (from_mantissa × 10^-from_scale) / (to_mantissa × 10^-to_scale)
        //   = micros × (from_mantissa × 10^to_scale) / (to_mantissa × 10^from_scale)
        let (from_mantissa, from_scale) = self.unit.definition();
        let (to_mantissa, to_scale) = unit.definition();
        let micros = pow10(to_scale)
            .and_then(|to_power| u128::from(from_mantissa).checked_mul(u128::from(to_power)))
            .zip(pow10(from_scale))
            .and_then(|(numerator, from_power)| {
                mode.round_ratio(self.micros, false, numerator, [to_mantissa, from_power])
            })
            .ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, unit })
    }

    /// Converts to `unit` exactly.
    ///
    /// # Errors
    /// [`QuantityError::InexactConversion`] if the result would need more than six decimal
    /// places (1 fl oz is 29.5735295625 ml), [`QuantityError::DimensionMismatch`] if `unit`
    /// measures something else, [`QuantityError::Overflow`] if the result is out of range.
    pub fn convert_exact(self, unit: Unit) -> Result<Quantity, QuantityError> {
        let down = self.convert_to(unit, RoundingMode::Floor)?;
        let up = self.convert_to(unit, RoundingMode::Ceiling)?;
        if down == up {
            Ok(down)
        } else {
            Err(QuantityError::InexactConversion { quantity: self, to: unit })
        }
    }

    /// Rounds to `decimal_places` decimal places (at most six) with `mode`: 1.23456 kg to two
    /// places is 1.23 kg rounding toward zero.
    ///
    /// # Errors
    /// [`QuantityError::Overflow`] if the rounded quantity is out of range.
    pub fn round_to_places(
        self,
        decimal_places: u32,
        mode: RoundingMode,
    ) -> Result<Quantity, QuantityError> {
        let dropped = Quantity::DECIMAL_PLACES.saturating_sub(decimal_places);
        let increment = pow10(dropped)
            .and_then(|power| u32::try_from(power).ok())
            .and_then(NonZeroU32::new)
            .ok_or(QuantityError::Overflow)?;
        let micros =
            mode.round_to_increment(self.micros, increment).ok_or(QuantityError::Overflow)?;
        Ok(Quantity { micros, ..self })
    }

    /// The quantity as a decimal, exactly and without trailing zeros: 1.5 kg → `1.5`.
    pub fn to_decimal(self) -> Decimal {
        fixed_point::to_decimal(self.micros, Quantity::DECIMAL_PLACES).normalize()
    }

    /// A quantity from a decimal, rounded to the nearest millionth with `mode`.
    ///
    /// # Errors
    /// [`QuantityError::Overflow`] if the quantity is out of range.
    pub fn from_decimal(
        value: Decimal,
        unit: Unit,
        mode: RoundingMode,
    ) -> Result<Quantity, QuantityError> {
        fixed_point::from_decimal(value, Quantity::DECIMAL_PLACES, mode)
            .map(|micros| Quantity { micros, unit })
            .map_err(|_| QuantityError::Overflow)
    }

    /// A quantity from a decimal that must be exactly representable.
    ///
    /// # Errors
    /// [`QuantityError::PrecisionExceeded`] if `value` has more than six significant decimal
    /// places, otherwise [`QuantityError::Overflow`] if it is out of range.
    pub fn from_decimal_exact(value: Decimal, unit: Unit) -> Result<Quantity, QuantityError> {
        match fixed_point::from_decimal_exact(value, Quantity::DECIMAL_PLACES) {
            Ok(micros) => Ok(Quantity { micros, unit }),
            Err(FixedPointError::PrecisionExceeded) => {
                Err(QuantityError::PrecisionExceeded { value, unit })
            }
            Err(FixedPointError::Overflow | FixedPointError::InvalidFormat) => {
                Err(QuantityError::Overflow)
            }
        }
    }

    /// Parses a quantity of `unit`, such as `"1.235"` or `"-0.5"`.
    ///
    /// The format is strict, as for [`Money::parse`](crate::Money::parse): an optional `-`, at
    /// least one digit, and optionally a `.` followed by one to six digits.
    ///
    /// # Errors
    /// [`QuantityError::InvalidFormat`] if the text doesn't match the format,
    /// [`QuantityError::Overflow`] if the quantity is out of range.
    pub fn parse(text: &str, unit: Unit) -> Result<Quantity, QuantityError> {
        match fixed_point::parse(text, Quantity::DECIMAL_PLACES) {
            Ok(micros) => Ok(Quantity { micros, unit }),
            Err(FixedPointError::Overflow) => Err(QuantityError::Overflow),
            Err(FixedPointError::InvalidFormat | FixedPointError::PrecisionExceeded) => {
                Err(QuantityError::InvalidFormat { text: text.to_owned(), unit })
            }
        }
    }

    fn same_unit(self, other: Quantity) -> Result<Unit, QuantityError> {
        if self.unit == other.unit {
            Ok(self.unit)
        } else {
            Err(QuantityError::UnitMismatch { left: self.unit, right: other.unit })
        }
    }
}

/// 10^exponent, if it fits in a u64.
fn pow10(exponent: u32) -> Option<u64> {
    10_u64.checked_pow(exponent)
}

impl fmt::Display for Quantity {
    /// Canonical, locale-independent form: `1.235 kg`, `-0.5 lb`, `3 each`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_decimal(), self.unit)
    }
}

impl fmt::Debug for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Quantity({self})")
    }
}

/// Errors from quantity arithmetic, conversion and parsing.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum QuantityError {
    /// Two quantities in different units were combined. Convert one of them first.
    #[error("unit mismatch: {left} and {right}")]
    UnitMismatch {
        /// The left-hand unit.
        left: Unit,
        /// The right-hand unit.
        right: Unit,
    },
    /// A conversion between units of different dimensions, such as grams to litres.
    #[error("cannot convert {from} to {to}: they measure different things")]
    DimensionMismatch {
        /// The unit converted from.
        from: Unit,
        /// The unit converted to.
        to: Unit,
    },
    /// An exact conversion whose result would need more than six decimal places.
    #[error("{quantity} is not a whole number of millionths of {to}")]
    InexactConversion {
        /// The quantity being converted.
        quantity: Quantity,
        /// The unit converted to.
        to: Unit,
    },
    /// The result doesn't fit in the representable range.
    #[error("quantity out of range")]
    Overflow,
    /// A value had more than six significant decimal places.
    #[error("{value} has more decimal places than a quantity of {unit} allows")]
    PrecisionExceeded {
        /// The value that couldn't be represented exactly.
        value: Decimal,
        /// The unit it was meant for.
        unit: Unit,
    },
    /// Text didn't match the strict quantity format.
    #[error("invalid {unit} quantity {text:?}")]
    InvalidFormat {
        /// The rejected text.
        text: String,
        /// The unit it was parsed for.
        unit: Unit,
    },
    /// No unit has this code.
    #[error("unknown unit code {0:?}")]
    UnknownUnit(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qty(text: &str, unit: Unit) -> Quantity {
        Quantity::parse(text, unit).unwrap()
    }

    fn whole(value: i64, unit: Unit) -> Quantity {
        Quantity::from_whole(value, unit).unwrap()
    }

    #[test]
    fn unit_codes_are_unique_stable_and_round_trip() {
        for (index, unit) in Unit::ALL.iter().enumerate() {
            let code = unit.code();
            assert!(
                code.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
                "{code:?} must be lowercase ASCII"
            );
            assert_eq!(Unit::from_code(code), Ok(*unit));
            let duplicate = Unit::ALL.iter().skip(index + 1).find(|other| other.code() == code);
            assert_eq!(duplicate, None, "duplicate code {code:?}");
        }
        assert_eq!(Unit::from_code("KG"), Err(QuantityError::UnknownUnit("KG".to_owned())));
        assert_eq!(Unit::from_code("gal"), Err(QuantityError::UnknownUnit("gal".to_owned())));
        // Codes are stored in events: these must never change.
        let pinned = [
            (Unit::Each, "each"),
            (Unit::Kilogram, "kg"),
            (Unit::Pound, "lb"),
            (Unit::UsFluidOunce, "fl_oz_us"),
            (Unit::ImperialGallon, "gal_imp"),
            (Unit::SquareFoot, "ft2"),
            (Unit::Hour, "h"),
        ];
        for (unit, code) in pinned {
            assert_eq!(unit.code(), code);
        }
    }

    #[test]
    fn every_dimension_has_a_base_unit() {
        for dimension in Dimension::ALL {
            let base = dimension.base_unit();
            assert_eq!(base.dimension(), dimension);
            assert_eq!(base.size_in_base_unit(), Decimal::ONE);
        }
        for unit in Unit::ALL {
            assert!(unit.size_in_base_unit() > Decimal::ZERO, "{unit}");
        }
    }

    /// The table is checked against the defining relationships between units, not against
    /// itself: each line is an exact identity.
    #[test]
    fn unit_definitions_satisfy_their_defining_relationships() {
        use Unit::*;
        let identities = [
            ("1", Dozen, "12", Each),
            ("1", Gram, "1000", Milligram),
            ("1", Kilogram, "1000", Gram),
            ("1", Tonne, "1000", Kilogram),
            ("1", Carat, "200", Milligram),
            ("1", Pound, "453.59237", Gram),
            ("1", Pound, "16", Ounce),
            ("1", ShortTon, "2000", Pound),
            ("1", TroyOunce, "31103.4768", Milligram),
            ("1", TroyOunce, "20", Pennyweight),
            ("1", Litre, "1000", Millilitre),
            ("1", Litre, "100", Centilitre),
            ("1", Litre, "10", Decilitre),
            ("1", CubicMetre, "1000", Litre),
            ("1", UsGallon, "231", CubicInch),
            ("1", UsGallon, "4", UsQuart),
            ("1", UsQuart, "2", UsPint),
            ("1", UsPint, "16", UsFluidOunce),
            ("1", ImperialGallon, "4.54609", Litre),
            ("1", ImperialGallon, "4", ImperialQuart),
            ("1", ImperialQuart, "2", ImperialPint),
            ("1", ImperialPint, "20", ImperialFluidOunce),
            ("1", CubicInch, "16.387064", Millilitre),
            ("1", CubicFoot, "1728", CubicInch),
            ("1", CubicYard, "27", CubicFoot),
            ("1", Centimetre, "10", Millimetre),
            ("1", Metre, "100", Centimetre),
            ("1", Kilometre, "1000", Metre),
            ("1", Inch, "2.54", Centimetre),
            ("1", Foot, "12", Inch),
            ("1", Yard, "3", Foot),
            ("1", Mile, "5280", Foot),
            ("1", SquareCentimetre, "100", SquareMillimetre),
            ("1", SquareMetre, "10000", SquareCentimetre),
            ("1", SquareInch, "6.4516", SquareCentimetre),
            ("1", SquareFoot, "144", SquareInch),
            ("1", SquareYard, "9", SquareFoot),
            ("1", Second, "1000", Millisecond),
            ("1", Minute, "60", Second),
            ("1", Hour, "60", Minute),
            ("1", Day, "24", Hour),
        ];
        for (left_value, left_unit, right_value, right_unit) in identities {
            let left = qty(left_value, left_unit);
            let right = qty(right_value, right_unit);
            assert_eq!(left.convert_exact(right_unit), Ok(right), "{left} = {right}");
            assert_eq!(right.convert_exact(left_unit), Ok(left), "{right} = {left}");
        }
        // Every unit appears in at least one identity.
        for unit in Unit::ALL {
            let mentioned =
                identities.iter().any(|(_, left, _, right)| left == unit || right == unit);
            assert!(mentioned, "{unit} has no defining relationship");
        }
    }

    #[test]
    fn conversions_round_once() {
        // Exact values from the definitions, rounded to millionths:
        // 1 kg = 1000/453.59237 lb = 2.20462262184877…
        let kilo = whole(1, Unit::Kilogram);
        assert_eq!(
            kilo.convert_to(Unit::Pound, RoundingMode::HalfEven),
            Ok(qty("2.204623", Unit::Pound))
        );
        assert_eq!(
            kilo.convert_to(Unit::Pound, RoundingMode::TowardZero),
            Ok(qty("2.204622", Unit::Pound))
        );
        // 1 g = 0.0352739619495804… oz
        let gram = whole(1, Unit::Gram);
        assert_eq!(
            gram.convert_to(Unit::Ounce, RoundingMode::HalfEven),
            Ok(qty("0.035274", Unit::Ounce))
        );
        // 100 ml = 3.38140227018429… US fl oz
        let hundred_ml = whole(100, Unit::Millilitre);
        assert_eq!(
            hundred_ml.convert_to(Unit::UsFluidOunce, RoundingMode::HalfEven),
            Ok(qty("3.381402", Unit::UsFluidOunce))
        );
        // 1 lb = 0.45359237 kg
        let pound = whole(1, Unit::Pound);
        assert_eq!(
            pound.convert_to(Unit::Kilogram, RoundingMode::HalfEven),
            Ok(qty("0.453592", Unit::Kilogram))
        );
        assert_eq!(
            pound.convert_to(Unit::Kilogram, RoundingMode::Ceiling),
            Ok(qty("0.453593", Unit::Kilogram))
        );
        // Negative quantities (returns) convert symmetrically.
        let returned = whole(-1, Unit::Kilogram);
        assert_eq!(
            returned.convert_to(Unit::Pound, RoundingMode::HalfEven),
            Ok(qty("-2.204623", Unit::Pound))
        );
        // 1 US fl oz is 29.5735295625 ml: too precise for an exact conversion.
        let ounce = whole(1, Unit::UsFluidOunce);
        assert_eq!(
            ounce.convert_exact(Unit::Millilitre),
            Err(QuantityError::InexactConversion { quantity: ounce, to: Unit::Millilitre })
        );
        assert_eq!(
            ounce.convert_to(Unit::Millilitre, RoundingMode::HalfEven),
            Ok(qty("29.57353", Unit::Millilitre))
        );
    }

    #[test]
    fn conversion_errors() {
        let kilo = whole(1, Unit::Kilogram);
        assert_eq!(
            kilo.convert_to(Unit::Litre, RoundingMode::HalfEven),
            Err(QuantityError::DimensionMismatch { from: Unit::Kilogram, to: Unit::Litre })
        );
        let most = Quantity::from_micros(i64::MAX, Unit::Tonne);
        assert_eq!(
            most.convert_to(Unit::Milligram, RoundingMode::HalfEven),
            Err(QuantityError::Overflow)
        );
        assert_eq!(most.convert_to(Unit::Tonne, RoundingMode::HalfEven), Ok(most));
    }

    #[test]
    fn arithmetic_is_checked_and_units_never_mix() {
        let a = qty("1.25", Unit::Kilogram);
        let b = qty("0.5", Unit::Kilogram);
        assert_eq!(a.checked_add(b), Ok(qty("1.75", Unit::Kilogram)));
        assert_eq!(a.checked_sub(b), Ok(qty("0.75", Unit::Kilogram)));
        assert_eq!(a.checked_mul(3), Ok(qty("3.75", Unit::Kilogram)));
        assert_eq!(a.checked_neg(), Ok(qty("-1.25", Unit::Kilogram)));
        assert_eq!(a.compare(b), Ok(Ordering::Greater));
        let pounds = qty("1", Unit::Pound);
        let mismatch = QuantityError::UnitMismatch { left: Unit::Kilogram, right: Unit::Pound };
        assert_eq!(a.checked_add(pounds), Err(mismatch.clone()));
        assert_eq!(a.checked_sub(pounds), Err(mismatch.clone()));
        assert_eq!(a.compare(pounds), Err(mismatch.clone()));
        assert_eq!(Quantity::sum(Unit::Kilogram, [a, pounds]), Err(mismatch));
        let max = Quantity::from_micros(i64::MAX, Unit::Gram);
        assert_eq!(max.checked_add(qty("0.000001", Unit::Gram)), Err(QuantityError::Overflow));
        assert_eq!(
            Quantity::from_whole(9_223_372_036_855, Unit::Each),
            Err(QuantityError::Overflow)
        );
    }

    #[test]
    fn multiplying_by_a_decimal_rounds_once() {
        let weight = qty("1.235", Unit::Kilogram);
        let half: Decimal = "0.5".parse().unwrap();
        assert_eq!(
            weight.mul_decimal(half, RoundingMode::HalfEven),
            Ok(qty("0.6175", Unit::Kilogram))
        );
        let third: Decimal = "0.3333333333333333333333333333".parse().unwrap();
        assert_eq!(
            weight.mul_decimal(third, RoundingMode::HalfEven),
            Ok(qty("0.411667", Unit::Kilogram))
        );
    }

    #[test]
    fn sum_is_exact_in_any_order() {
        let max = Quantity::from_micros(i64::MAX, Unit::Each);
        let one = Quantity::from_micros(1, Unit::Each);
        let minus_one = Quantity::from_micros(-1, Unit::Each);
        assert_eq!(Quantity::sum(Unit::Each, [max, one, minus_one]), Ok(max));
        assert_eq!(Quantity::sum(Unit::Each, [minus_one, one, max]), Ok(max));
        assert_eq!(Quantity::sum(Unit::Each, []), Ok(Quantity::zero(Unit::Each)));
    }

    #[test]
    fn rounding_to_places() {
        let weight = qty("1.234567", Unit::Kilogram);
        assert_eq!(
            weight.round_to_places(3, RoundingMode::HalfEven),
            Ok(qty("1.235", Unit::Kilogram))
        );
        assert_eq!(weight.round_to_places(0, RoundingMode::Floor), Ok(qty("1", Unit::Kilogram)));
        assert_eq!(weight.round_to_places(6, RoundingMode::Floor), Ok(weight));
        assert_eq!(weight.round_to_places(9, RoundingMode::Floor), Ok(weight));
    }

    #[test]
    fn display_parse_and_decimals() {
        assert_eq!(qty("1.235", Unit::Kilogram).to_string(), "1.235 kg");
        assert_eq!(qty("-0.5", Unit::Pound).to_string(), "-0.5 lb");
        assert_eq!(qty("3", Unit::Each).to_string(), "3 each");
        assert_eq!(qty("0.000", Unit::Gram).to_string(), "0 g");
        assert_eq!(format!("{:?}", qty("2", Unit::Hour)), "Quantity(2 h)");
        assert_eq!(qty("9223372036854.775807", Unit::Gram).micros(), i64::MAX);
        assert_eq!(qty("-9223372036854.775808", Unit::Gram).micros(), i64::MIN);
        for text in ["", "1.2345678", "+1", "1e3", "1,5", " 1", "1."] {
            assert!(
                matches!(
                    Quantity::parse(text, Unit::Gram),
                    Err(QuantityError::InvalidFormat { .. })
                ),
                "{text:?}"
            );
        }
        let value: Decimal = "1.2345675".parse().unwrap();
        assert_eq!(
            Quantity::from_decimal(value, Unit::Litre, RoundingMode::HalfEven),
            Ok(qty("1.234568", Unit::Litre))
        );
        assert_eq!(
            Quantity::from_decimal_exact(value, Unit::Litre),
            Err(QuantityError::PrecisionExceeded { value, unit: Unit::Litre })
        );
        assert_eq!(qty("1.5", Unit::Litre).to_decimal(), "1.5".parse::<Decimal>().unwrap());
    }
}
