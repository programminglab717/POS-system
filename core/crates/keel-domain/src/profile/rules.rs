//! The pricing rules, as a profile writes them: `keel-pricing`'s [`Rules`].

use std::collections::BTreeSet;

use keel_events::cbor::Value;
use keel_pricing::{Dining, Rules, Tax, TaxCategory, TaxRounding, TaxScope};
use keel_types::{Decimal, Id, Rate, RoundingMode};

use super::{MAX_CATEGORIES, MAX_TAXES, ProfileError};
use crate::codec::{Field, Fields, IdSet, Name, Record};

/// Rules keys.
mod key {
    pub(super) const EXTENSION: u64 = 1;
    pub(super) const DISCOUNTS: u64 = 2;
    pub(super) const TAXES: u64 = 3;
    pub(super) const TAX_ROUNDING: u64 = 4;

    pub(super) const TAX_ID: u64 = 1;
    pub(super) const TAX_NAME: u64 = 2;
    pub(super) const TAX_RATE: u64 = 3;
    pub(super) const TAX_CATEGORIES: u64 = 4;
    pub(super) const TAX_DINING: u64 = 5;
}

/// A rounding mode's code.
pub(super) const fn mode_code(mode: RoundingMode) -> u64 {
    match mode {
        RoundingMode::HalfAwayFromZero => 0,
        RoundingMode::HalfEven => 1,
        RoundingMode::HalfTowardZero => 2,
        RoundingMode::AwayFromZero => 3,
        RoundingMode::TowardZero => 4,
        RoundingMode::Ceiling => 5,
        RoundingMode::Floor => 6,
    }
}

/// A rounding mode, by its code.
pub(super) fn mode_value(mode: RoundingMode) -> Value {
    Value::Unsigned(mode_code(mode))
}

/// The rounding mode a code stands for.
pub(super) fn mode_from(value: &Value) -> Option<RoundingMode> {
    Some(match value.as_u64()? {
        0 => RoundingMode::HalfAwayFromZero,
        1 => RoundingMode::HalfEven,
        2 => RoundingMode::HalfTowardZero,
        3 => RoundingMode::AwayFromZero,
        4 => RoundingMode::TowardZero,
        5 => RoundingMode::Ceiling,
        6 => RoundingMode::Floor,
        _ => return None,
    })
}

/// The rules' encoding, which their version hashes.
pub(super) fn to_value(rules: &Rules) -> Value {
    let scope = match rules.tax_rounding.scope {
        TaxScope::Line => 0,
        TaxScope::Document => 1,
    };
    Record::default()
        .field(key::EXTENSION, &ModeField(rules.extension))
        .field(key::DISCOUNTS, &ModeField(rules.discounts))
        .field(key::TAXES, &rules.taxes.iter().cloned().map(TaxField).collect::<Vec<_>>())
        .field(
            key::TAX_ROUNDING,
            &RawField(Value::Array(vec![
                Value::Unsigned(scope),
                mode_value(rules.tax_rounding.mode),
            ])),
        )
        .build()
}

/// The rules `value` encodes, or `None` if it isn't their encoding.
pub(super) fn from_value(value: &Value) -> Option<Rules> {
    let mut fields = Fields::read(value).ok()?;
    let extension = fields.required::<ModeField>(key::EXTENSION, "extension rounding").ok()?.0;
    let discounts = fields.required::<ModeField>(key::DISCOUNTS, "discount rounding").ok()?.0;
    let taxes: Vec<TaxField> = fields.required(key::TAXES, "taxes").ok()?;
    let rounding = fields.required::<RawField>(key::TAX_ROUNDING, "tax rounding").ok()?.0;
    fields.finish().ok()?;
    let [scope, mode] = rounding.as_array()? else { return None };
    let scope = match scope.as_u64()? {
        0 => TaxScope::Line,
        1 => TaxScope::Document,
        _ => return None,
    };
    Some(Rules {
        extension,
        discounts,
        taxes: taxes.into_iter().map(|tax| tax.0).collect(),
        tax_rounding: TaxRounding { scope, mode: mode_from(mode)? },
    })
}

/// Checks the rules: within the limit of taxes, each identified once, named, at a rate of zero
/// or more, and applying to between one and [`MAX_CATEGORIES`] tax categories, each once, in the
/// ascending order of their identifiers' bytes, as sets of identifiers are written.
pub(super) fn check(rules: &Rules) -> Result<(), ProfileError> {
    if rules.taxes.len() > MAX_TAXES {
        return Err(ProfileError::Invalid("more taxes than allowed"));
    }
    let mut ids = BTreeSet::new();
    for tax in &rules.taxes {
        if !ids.insert(tax.id) {
            return Err(ProfileError::Duplicate("taxes"));
        }
        if Name::new(&tax.name).is_err() {
            return Err(ProfileError::Invalid("a tax without a valid name"));
        }
        if tax.rate.as_fraction() < Decimal::ZERO {
            return Err(ProfileError::Invalid("a negative tax rate"));
        }
        if tax.categories.is_empty() {
            return Err(ProfileError::Invalid("a tax with no tax categories"));
        }
        if tax.categories.len() > MAX_CATEGORIES {
            return Err(ProfileError::Invalid("a tax with more tax categories than allowed"));
        }
        let ascending = tax.categories.windows(2).all(|pair| match pair {
            [left, right] => left.to_bytes() < right.to_bytes(),
            _ => true,
        });
        if !ascending {
            return Err(ProfileError::Invalid(
                "a tax's categories not each once, in ascending order",
            ));
        }
    }
    Ok(())
}

/// A rounding mode, by its code.
struct ModeField(RoundingMode);

impl Field for ModeField {
    fn to_value(&self) -> Value {
        mode_value(self.0)
    }

    fn from_value(value: &Value) -> Option<ModeField> {
        mode_from(value).map(ModeField)
    }
}

/// Any value, read as it is.
struct RawField(Value);

impl Field for RawField {
    fn to_value(&self) -> Value {
        self.0.clone()
    }

    fn from_value(value: &Value) -> Option<RawField> {
        Some(RawField(value.clone()))
    }
}

/// A tax.
struct TaxField(Tax);

impl Field for TaxField {
    fn to_value(&self) -> Value {
        let tax = &self.0;
        let dining = tax.dining.map(|dining| match dining {
            Dining::OnPremises => 0_u64,
            Dining::ToGo => 1,
        });
        let record = Record::default()
            .field(key::TAX_ID, &tax.id)
            .field(key::TAX_NAME, &RawField(Value::from(tax.name.as_str())))
            .field(key::TAX_RATE, &RawField(Value::from(rate_text(tax.rate).as_str())))
            .field(key::TAX_CATEGORIES, &tax.categories);
        match dining {
            Some(code) => record.field(key::TAX_DINING, &RawField(Value::Unsigned(code))),
            None => record,
        }
        .build()
    }

    fn from_value(value: &Value) -> Option<TaxField> {
        let mut fields = Fields::read(value).ok()?;
        let id: Id<Tax> = fields.required(key::TAX_ID, "tax").ok()?;
        let name: Name = fields.required(key::TAX_NAME, "tax name").ok()?;
        let rate = fields.required::<RawField>(key::TAX_RATE, "tax rate").ok()?.0;
        let categories: IdSet<TaxCategory> =
            fields.required(key::TAX_CATEGORIES, "tax categories").ok()?;
        let dining = fields.optional::<RawField>(key::TAX_DINING, "dining").ok()?;
        fields.finish().ok()?;
        let dining = match dining.map(|raw| raw.0.as_u64()) {
            None => None,
            Some(Some(0)) => Some(Dining::OnPremises),
            Some(Some(1)) => Some(Dining::ToGo),
            Some(_) => return None,
        };
        Some(TaxField(Tax {
            id,
            name: name.as_str().to_owned(),
            rate: rate_from(rate.as_text()?)?,
            categories: categories.iter().collect(),
            dining,
        }))
    }
}

/// A rate's text: its decimal fraction, with no trailing zeros, such as `0.08875`, and zero, of
/// either sign, as `0`.
fn rate_text(rate: Rate) -> String {
    let fraction = rate.as_fraction();
    if fraction.is_zero() { "0".to_owned() } else { fraction.normalize().to_string() }
}

/// The rate `text` writes, if it is a rate's text exactly: digits, perhaps a point and more
/// digits, with no sign, no leading zero before another digit, and no trailing zero after the
/// point, and the very text of the rate it reads as, which a text with more digits than a rate
/// holds isn't. So every rate has one text, and every text one rate.
fn rate_from(text: &str) -> Option<Rate> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let canonical = !whole.is_empty()
        && digits(whole)
        && digits(fraction)
        && (whole == "0" || !whole.starts_with('0'))
        && (!text.contains('.') || (!fraction.is_empty() && !fraction.ends_with('0')));
    if !canonical {
        return None;
    }
    let rate = Rate::from_fraction(text.parse::<Decimal>().ok()?);
    (rate_text(rate) == text).then_some(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_have_one_text_each() {
        for (text, expected) in [("0", "0"), ("0.08875", "0.08875"), ("1", "1"), ("12.5", "12.5")] {
            let rate = rate_from(text).unwrap();
            assert_eq!(rate_text(rate), expected);
        }
        for bad in
            ["", ".5", "0.", "00.1", "01", "0.10", "1.0", "-0.1", "+0.1", "0.1e2", " 0.1", "0,1"]
        {
            assert_eq!(rate_from(bad), None, "{bad:?}");
        }
        // Texts with more digits than a rate holds would read as another rate's.
        for long in [
            "0.00000000000000000000000000001",
            "0.99999999999999999999999999999",
            "0.11111111111111111111111111111",
            "1.00000000000000000000000000001",
            "7922816251426433759354395033.55",
            "79228162514264337593543950336",
        ] {
            assert_eq!(rate_from(long), None, "{long:?}");
        }
        // As many as a rate holds read as themselves.
        for exact in ["0.0000000000000000000000000001", "79228162514264337593543950335"] {
            assert_eq!(rate_from(exact).map(rate_text).as_deref(), Some(exact));
        }
    }

    #[test]
    fn a_rate_of_negative_zero_is_written_as_zero() {
        let negative_zero = Rate::from_fraction(-Decimal::ZERO);
        assert_eq!(rate_text(negative_zero), "0");
        assert_eq!(rate_from("0"), Some(negative_zero));
    }

    #[test]
    fn rounding_modes_have_codes_of_their_own() {
        let codes = [
            (RoundingMode::HalfAwayFromZero, 0),
            (RoundingMode::HalfEven, 1),
            (RoundingMode::HalfTowardZero, 2),
            (RoundingMode::AwayFromZero, 3),
            (RoundingMode::TowardZero, 4),
            (RoundingMode::Ceiling, 5),
            (RoundingMode::Floor, 6),
        ];
        assert_eq!(codes.len(), RoundingMode::ALL.len());
        for (mode, code) in codes {
            assert_eq!(mode_value(mode), Value::Unsigned(code));
            assert_eq!(mode_from(&Value::Unsigned(code)), Some(mode));
        }
        assert_eq!(mode_from(&Value::Unsigned(7)), None);
        assert_eq!(mode_from(&Value::from("0")), None);
    }
}
