//! The pricing rules, as a profile writes them: `keel-pricing`'s [`Rules`].

use std::collections::BTreeSet;

use keel_events::cbor::Value;
use keel_pricing::{Dining, Rules, Tax, TaxRounding, TaxScope};
use keel_types::{Decimal, Id, Rate, RoundingMode};

use super::{MAX_TAXES, ProfileError};
use crate::codec::{Field, Fields, Name, Record};

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

/// A rounding mode's code: its position in [`RoundingMode::ALL`].
pub(super) fn mode_value(mode: RoundingMode) -> Value {
    let code = RoundingMode::ALL.iter().position(|candidate| *candidate == mode).unwrap_or(0);
    Value::Unsigned(u64::try_from(code).unwrap_or(0))
}

/// The rounding mode a code stands for.
pub(super) fn mode_from(value: &Value) -> Option<RoundingMode> {
    RoundingMode::ALL.get(usize::try_from(value.as_u64()?).ok()?).copied()
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
/// or more, and applying to at least one tax category, each once.
pub(super) fn check(rules: &Rules) -> Result<(), ProfileError> {
    if rules.taxes.len() > MAX_TAXES {
        return Err(ProfileError::Invalid("more taxes than allowed"));
    }
    let mut ids = BTreeSet::new();
    for tax in &rules.taxes {
        if !ids.insert(tax.id) {
            return Err(ProfileError::Duplicate("tax"));
        }
        if Name::new(&tax.name).is_err() {
            return Err(ProfileError::Invalid("a tax without a valid name"));
        }
        if tax.rate.as_fraction().is_sign_negative() {
            return Err(ProfileError::Invalid("a negative tax rate"));
        }
        let categories: BTreeSet<_> = tax.categories.iter().collect();
        if categories.is_empty() || categories.len() != tax.categories.len() {
            return Err(ProfileError::Invalid("a tax with no tax categories, or one twice"));
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
        let categories = fields.required(key::TAX_CATEGORIES, "tax categories").ok()?;
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
            categories,
            dining,
        }))
    }
}

/// A rate's text: its decimal fraction, with no trailing zeros, such as `0.08875`.
fn rate_text(rate: Rate) -> String {
    rate.as_fraction().normalize().to_string()
}

/// The rate `text` writes, if it is a rate's text exactly: digits, perhaps a point and more
/// digits, with no sign, no leading zero before another digit, and no trailing zero after the
/// point, so that every rate has one text.
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
    text.parse::<Decimal>().ok().map(Rate::from_fraction)
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
    }

    #[test]
    fn rounding_modes_are_coded_by_position() {
        for (code, mode) in RoundingMode::ALL.iter().enumerate() {
            let value = mode_value(*mode);
            assert_eq!(value, Value::Unsigned(u64::try_from(code).unwrap()));
            assert_eq!(mode_from(&value), Some(*mode));
        }
        assert_eq!(mode_from(&Value::Unsigned(7)), None);
    }
}
