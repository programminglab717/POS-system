//! Payload codecs: how domain values are written in event payloads.
//!
//! A payload is one canonical CBOR map with small unsigned integer keys, like the event envelope.
//! Each schema version documents its keys, and a released key never changes meaning. Values use
//! these encodings:
//!
//! | Value | Encoding |
//! |---|---|
//! | identifier | 16-byte byte string: a UUIDv7 |
//! | currency | text string: the ISO 4217 code, such as `"USD"` |
//! | money | `[minor units, currency code]`, such as `[350, "USD"]` |
//! | quantity | `[millionths, unit code]`, such as `[1500000, "kg"]` |
//! | name, note | text string of bounded length, without control characters |
//! | reason code | text string: a lowercase identifier of up to 32 characters |
//! | processor reference | text string: 1 to 100 ASCII letters, digits and punctuation |
//! | code (an enumeration) | unsigned integer |
//! | small count | unsigned integer, from 1 |
//! | catalog version, rules version | 32-byte byte string: a content hash |
//! | set of identifiers | array of identifiers in ascending byte order, with no repeats |
//!
//! Optional fields are omitted when absent, never written as `null`. In a payload that changes
//! fields, an absent field is unchanged, and a field set to `null` is cleared. Decoding is strict:
//! unknown keys, missing fields, wrong types and invalid values are all rejected, so every payload
//! has exactly one encoding.

use core::fmt;
use core::num::{NonZeroU8, NonZeroU16};

use keel_events::cbor::{Map, Value};
use keel_types::{Currency, Id, Money, Quantity, Unit};

/// Why a payload doesn't match its schema.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PayloadError {
    /// The payload, or a nested record, isn't a map.
    #[error("expected a map")]
    NotAMap,
    /// A key the schema doesn't define.
    #[error("unknown field {0}")]
    UnknownField(String),
    /// A required field is missing.
    #[error("missing {0}")]
    Missing(&'static str),
    /// A field has the wrong type or an invalid value.
    #[error("invalid {0}")]
    Invalid(&'static str),
    /// A change that changes nothing.
    #[error("the change is empty")]
    EmptyChange,
}

/// A value with a payload encoding.
pub(crate) trait Field: Sized {
    /// The value's encoding.
    fn to_value(&self) -> Value;
    /// The value an encoding holds, or `None` if it isn't a valid encoding.
    fn from_value(value: &Value) -> Option<Self>;
}

/// A change to an optional field: set it to a value, or clear it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Change<T> {
    /// Set the field to this value.
    Set(T),
    /// Clear the field.
    Clear,
}

impl<T> Change<T> {
    /// The new value: `Some` to set, `None` to clear.
    pub fn into_option(self) -> Option<T> {
        match self {
            Change::Set(value) => Some(value),
            Change::Clear => None,
        }
    }

    /// The change as a reference.
    pub const fn as_ref(&self) -> Change<&T> {
        match self {
            Change::Set(value) => Change::Set(value),
            Change::Clear => Change::Clear,
        }
    }
}

/// Reads a payload map field by field, and checks that no field is left unread.
pub(crate) struct Fields<'a> {
    entries: Vec<(u64, &'a Value, bool)>,
}

impl<'a> Fields<'a> {
    /// The fields of `value`, which must be a map with unsigned integer keys.
    pub(crate) fn read(value: &'a Value) -> Result<Fields<'a>, PayloadError> {
        let map = value.as_map().ok_or(PayloadError::NotAMap)?;
        let entries = map
            .iter()
            .map(|(key, value)| {
                key.as_u64()
                    .map(|key| (key, value, false))
                    .ok_or_else(|| PayloadError::UnknownField(key.to_string()))
            })
            .collect::<Result<_, _>>()?;
        Ok(Fields { entries })
    }

    fn take(&mut self, key: u64) -> Option<&'a Value> {
        self.entries.iter_mut().find(|(entry_key, _, _)| *entry_key == key).map(|entry| {
            entry.2 = true;
            entry.1
        })
    }

    /// A required field.
    pub(crate) fn required<T: Field>(
        &mut self,
        key: u64,
        name: &'static str,
    ) -> Result<T, PayloadError> {
        let value = self.take(key).ok_or(PayloadError::Missing(name))?;
        T::from_value(value).ok_or(PayloadError::Invalid(name))
    }

    /// An optional field.
    pub(crate) fn optional<T: Field>(
        &mut self,
        key: u64,
        name: &'static str,
    ) -> Result<Option<T>, PayloadError> {
        self.take(key)
            .map(|value| T::from_value(value).ok_or(PayloadError::Invalid(name)))
            .transpose()
    }

    /// An optional change to a field that can be cleared: absent is unchanged, `null` clears.
    pub(crate) fn change<T: Field>(
        &mut self,
        key: u64,
        name: &'static str,
    ) -> Result<Option<Change<T>>, PayloadError> {
        self.take(key)
            .map(|value| match value {
                Value::Null => Ok(Change::Clear),
                value => T::from_value(value).map(Change::Set).ok_or(PayloadError::Invalid(name)),
            })
            .transpose()
    }

    /// Checks that every field was read.
    pub(crate) fn finish(self) -> Result<(), PayloadError> {
        match self.entries.iter().find(|(_, _, read)| !read) {
            Some((key, _, _)) => Err(PayloadError::UnknownField(key.to_string())),
            None => Ok(()),
        }
    }
}

/// Builds a payload map.
#[derive(Default)]
pub(crate) struct Record {
    entries: Vec<(Value, Value)>,
}

impl Record {
    /// A field.
    pub(crate) fn field(mut self, key: u64, value: &impl Field) -> Record {
        self.entries.push((Value::Unsigned(key), value.to_value()));
        self
    }

    /// An optional field, omitted when absent.
    pub(crate) fn optional<T: Field>(self, key: u64, value: Option<&T>) -> Record {
        match value {
            Some(value) => self.field(key, value),
            None => self,
        }
    }

    /// An optional change, omitted when absent; `null` when it clears the field.
    pub(crate) fn change<T: Field>(mut self, key: u64, change: Option<&Change<T>>) -> Record {
        match change {
            Some(Change::Set(value)) => self.field(key, value),
            Some(Change::Clear) => {
                self.entries.push((Value::Unsigned(key), Value::Null));
                self
            }
            None => self,
        }
    }

    /// The finished map.
    pub(crate) fn build(self) -> Value {
        // Callers use each key once, so this can't fail.
        Value::Map(Map::from_entries(self.entries).unwrap_or_default())
    }
}

impl<T> Field for Id<T> {
    fn to_value(&self) -> Value {
        Value::Bytes(self.to_bytes().to_vec())
    }

    fn from_value(value: &Value) -> Option<Id<T>> {
        let bytes = <[u8; 16]>::try_from(value.as_bytes()?).ok()?;
        Id::from_bytes(bytes).ok()
    }
}

impl Field for Money {
    fn to_value(&self) -> Value {
        Value::Array(vec![Value::integer(self.minor()), Value::from(self.currency().code())])
    }

    fn from_value(value: &Value) -> Option<Money> {
        let [minor, currency] = value.as_array()? else { return None };
        let currency = Currency::from_code(currency.as_text()?).ok()?;
        Some(Money::from_minor(minor.as_i64()?, currency))
    }
}

impl Field for Currency {
    fn to_value(&self) -> Value {
        Value::from(self.code())
    }

    fn from_value(value: &Value) -> Option<Currency> {
        Currency::from_code(value.as_text()?).ok()
    }
}

impl Field for Quantity {
    fn to_value(&self) -> Value {
        Value::Array(vec![Value::integer(self.micros()), Value::from(self.unit().code())])
    }

    fn from_value(value: &Value) -> Option<Quantity> {
        let [micros, unit] = value.as_array()? else { return None };
        let unit = Unit::from_code(unit.as_text()?).ok()?;
        Some(Quantity::from_micros(micros.as_i64()?, unit))
    }
}

impl Field for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_value(value: &Value) -> Option<bool> {
        value.as_bool()
    }
}

impl Field for NonZeroU8 {
    fn to_value(&self) -> Value {
        Value::Unsigned(u64::from(self.get()))
    }

    fn from_value(value: &Value) -> Option<NonZeroU8> {
        value.as_u64().and_then(|n| u8::try_from(n).ok()).and_then(NonZeroU8::new)
    }
}

impl Field for NonZeroU16 {
    fn to_value(&self) -> Value {
        Value::Unsigned(u64::from(self.get()))
    }

    fn from_value(value: &Value) -> Option<NonZeroU16> {
        value.as_u64().and_then(|n| u16::try_from(n).ok()).and_then(NonZeroU16::new)
    }
}

impl<T: Field> Field for Vec<T> {
    fn to_value(&self) -> Value {
        Value::Array(self.iter().map(Field::to_value).collect())
    }

    fn from_value(value: &Value) -> Option<Vec<T>> {
        value.as_array()?.iter().map(T::from_value).collect()
    }
}

/// Declares a text type: validated on construction, encoded as a text string.
macro_rules! text_type {
    ($(#[$meta:meta])* $name:ident, $what:literal, $valid:expr) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// The text, validated.
            ///
            /// # Errors
            #[doc = concat!("[`PayloadError::Invalid`] if the text isn't a valid ", $what, ".")]
            pub fn new(text: &str) -> Result<$name, PayloadError> {
                let valid: fn(&str) -> bool = $valid;
                if valid(text) { Ok($name(text.to_owned())) } else { Err(PayloadError::Invalid($what)) }
            }

            /// The text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:?})", stringify!($name), self.0)
            }
        }

        impl Field for $name {
            fn to_value(&self) -> Value {
                Value::from(self.as_str())
            }

            fn from_value(value: &Value) -> Option<$name> {
                $name::new(value.as_text()?).ok()
            }
        }
    };
}

text_type!(
    /// A display name, such as an item's or a modifier's: 1 to 200 characters, with no control
    /// characters.
    Name,
    "name",
    |text| is_text(text, 200, false)
);

text_type!(
    /// A free-text note, such as kitchen instructions: 1 to 500 characters, with no control
    /// characters other than line feeds.
    Note,
    "note",
    |text| is_text(text, 500, true)
);

text_type!(
    /// A reason code, such as `kitchen_error`: 1 to 32 characters, a lowercase letter then
    /// lowercase letters, digits and underscores. Merchants define their own codes; reports group
    /// by them.
    ReasonCode,
    "reason code",
    is_identifier
);

text_type!(
    /// A payment processor's or terminal's reference for a payment, such as its transaction
    /// identifier: 1 to 100 ASCII letters, digits and punctuation, with no spaces.
    ProcessorRef,
    "processor reference",
    |text| (1..=100).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_graphic())
);

/// Text of 1 to `max` characters with no control characters, except line feeds where allowed.
/// Control characters could drive printers and displays, so they are never stored.
fn is_text(text: &str, max: usize, line_feeds: bool) -> bool {
    !text.is_empty()
        && text.chars().count() <= max
        && text.chars().all(|c| !c.is_control() || (line_feeds && c == '\n'))
}

/// 1 to 32 characters: a lowercase letter, then lowercase letters, digits and underscores.
fn is_identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    text.len() <= 32
        && bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Declares a version of published reference data, identified by its 32-byte content hash.
macro_rules! version_type {
    ($(#[$meta:meta])* $name:ident, $what:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
            #[doc = concat!("A ", $what, " from its 32-byte hash.")]
            pub const fn from_bytes(bytes: [u8; 32]) -> $name {
                $name(bytes)
            }

            /// The 32-byte hash.
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "("))?;
                self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))?;
                f.write_str(")")
            }
        }

        impl Field for $name {
            fn to_value(&self) -> Value {
                Value::from(self.0.as_slice())
            }

            fn from_value(value: &Value) -> Option<$name> {
                <[u8; 32]>::try_from(value.as_bytes()?).ok().map($name)
            }
        }
    };
}

version_type!(
    /// A published catalog version: the content hash that identifies it.
    CatalogVersion,
    "catalog version"
);

version_type!(
    /// A published version of a location's pricing rules (its taxes and rounding): the content
    /// hash that identifies it. A check's snapshot records the version it was priced with.
    RulesVersion,
    "rules version"
);

/// A non-empty set of identifiers, kept in ascending order, so it has one encoding.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct IdSet<T>(Vec<Id<T>>);

impl<T> IdSet<T> {
    /// The set of `ids`, in any order.
    ///
    /// # Errors
    /// [`PayloadError::Invalid`] if `ids` is empty or repeats an identifier.
    pub fn new(ids: impl IntoIterator<Item = Id<T>>) -> Result<IdSet<T>, PayloadError> {
        let mut ids: Vec<Id<T>> = ids.into_iter().collect();
        ids.sort_by_key(|id| id.to_bytes());
        let repeats = ids.windows(2).any(|pair| matches!(pair, [left, right] if left == right));
        if ids.is_empty() || repeats {
            return Err(PayloadError::Invalid("identifier set"));
        }
        Ok(IdSet(ids))
    }

    /// The identifiers, in ascending order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Id<T>> + '_ {
        self.0.iter().copied()
    }

    /// The number of identifiers.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set is empty. It never is: sets have at least one identifier.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T> fmt::Debug for IdSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
    }
}

impl<T> Field for IdSet<T> {
    fn to_value(&self) -> Value {
        Value::Array(self.0.iter().map(Field::to_value).collect())
    }

    fn from_value(value: &Value) -> Option<IdSet<T>> {
        let ids: Vec<Id<T>> = Field::from_value(value)?;
        // Only the ascending, repeat-free order is accepted.
        let ascending = ids.windows(2).all(|pair| match pair {
            [left, right] => left.to_bytes() < right.to_bytes(),
            _ => true,
        });
        (ascending && !ids.is_empty()).then_some(IdSet(ids))
    }
}

/// Declares an enumeration with stable numeric codes, encoded as unsigned integers.
macro_rules! code_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident = $code:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis enum $name {
            $($(#[$variant_meta])* $variant,)+
        }

        impl $name {
            /// Every value, in code order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)+];

            /// The stable code written in payloads. Codes never change.
            pub const fn code(self) -> u64 {
                match self { $($name::$variant => $code,)+ }
            }

            /// The value with `code`, if there is one.
            pub const fn from_code(code: u64) -> Option<$name> {
                match code {
                    $($code => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl $crate::codec::Field for $name {
            fn to_value(&self) -> keel_events::cbor::Value {
                keel_events::cbor::Value::Unsigned(self.code())
            }

            fn from_value(value: &keel_events::cbor::Value) -> Option<$name> {
                $name::from_code(value.as_u64()?)
            }
        }
    };
}

pub(crate) use code_enum;

#[cfg(test)]
mod tests {
    use keel_types::Unit;

    use super::*;

    #[test]
    fn values_have_one_encoding() {
        let usd = Currency::from_code("USD").unwrap();
        let price = Money::from_minor(-350, usd);
        assert_eq!(price.to_value(), Value::Array(vec![Value::integer(-350), Value::from("USD")]));
        assert_eq!(Money::from_value(&price.to_value()), Some(price));
        let weight = Quantity::from_micros(1_500_000, Unit::Kilogram);
        assert_eq!(
            weight.to_value(),
            Value::Array(vec![Value::Unsigned(1_500_000), Value::from("kg")])
        );
        assert_eq!(Quantity::from_value(&weight.to_value()), Some(weight));

        for bad in [
            Value::Array(vec![Value::Unsigned(1), Value::from("usd")]),
            Value::Array(vec![Value::Unsigned(1), Value::from("XXX1")]),
            Value::Array(vec![Value::Unsigned(1)]),
            Value::Array(vec![Value::Unsigned(1), Value::from("USD"), Value::Null]),
            Value::Array(vec![Value::Unsigned(u64::MAX), Value::from("USD")]),
            Value::Array(vec![Value::from("1"), Value::from("USD")]),
            Value::Unsigned(1),
        ] {
            assert_eq!(Money::from_value(&bad), None, "{bad}");
        }
        assert_eq!(
            Quantity::from_value(&Value::Array(vec![Value::Unsigned(1), Value::from("KG")])),
            None
        );
    }

    #[test]
    fn text_follows_its_rules() {
        assert!(Name::new("Flat white").is_ok());
        assert!(Name::new(&"é".repeat(200)).is_ok());
        for bad in ["", "tab\there", "line\nfeed", "\u{1b}[1m", "\u{85}next line", &"a".repeat(201)]
        {
            assert!(Name::new(bad).is_err(), "{bad:?}");
        }
        assert!(Note::new("no salt\nextra lemon").is_ok());
        assert!(Note::new(&"a".repeat(500)).is_ok());
        for bad in ["", "\r\n", "bell\u{7}", &"a".repeat(501)] {
            assert!(Note::new(bad).is_err(), "{bad:?}");
        }
        for good in ["kitchen_error", "a", "wrong_item_2", &"a".repeat(32)] {
            assert!(ReasonCode::new(good).is_ok(), "{good}");
        }
        for bad in ["", "Kitchen", "2nd", "_x", "kitchen-error", &"a".repeat(33)] {
            assert!(ReasonCode::new(bad).is_err(), "{bad}");
        }
        for good in ["pi_3Mtw", "8815678901234567", "A-1/b.c:9", &"x".repeat(100)] {
            assert!(ProcessorRef::new(good).is_ok(), "{good}");
        }
        for bad in ["", "two words", "tab\there", "é", &"x".repeat(101)] {
            assert!(ProcessorRef::new(bad).is_err(), "{bad}");
        }
        assert_eq!(format!("{:?}", Name::new("Tea").unwrap()), "Name(\"Tea\")");
        assert_eq!(Name::new("Tea").unwrap().to_string(), "Tea");
    }

    #[test]
    fn id_sets_are_ascending_and_distinct() {
        let a: Id<()> = Id::parse("0192f0c1-0000-7000-8000-000000000001").unwrap();
        let b: Id<()> = Id::parse("0192f0c1-0000-7000-8000-000000000002").unwrap();
        let set = IdSet::new([b, a]).unwrap();
        assert_eq!(set.iter().collect::<Vec<_>>(), [a, b]);
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
        assert_eq!(IdSet::from_value(&set.to_value()), Some(set.clone()));
        assert!(IdSet::<()>::new([]).is_err());
        assert!(IdSet::new([a, a]).is_err());
        let descending = Value::Array(vec![b.to_value(), a.to_value()]);
        assert_eq!(IdSet::<()>::from_value(&descending), None);
        let repeated = Value::Array(vec![a.to_value(), a.to_value()]);
        assert_eq!(IdSet::<()>::from_value(&repeated), None);
        assert_eq!(IdSet::<()>::from_value(&Value::Array(Vec::new())), None);
    }

    #[test]
    fn fields_are_read_strictly() {
        let value = Record::default()
            .field(1, &NonZeroU8::MIN)
            .change(2, Some(&Change::<NonZeroU8>::Clear))
            .optional::<NonZeroU8>(3, None)
            .build();
        let mut fields = Fields::read(&value).unwrap();
        assert_eq!(fields.required::<NonZeroU8>(1, "one"), Ok(NonZeroU8::MIN));
        assert_eq!(fields.change::<NonZeroU8>(2, "two"), Ok(Some(Change::Clear)));
        assert_eq!(fields.optional::<NonZeroU8>(3, "three"), Ok(None));
        assert_eq!(fields.finish(), Ok(()));

        let mut fields = Fields::read(&value).unwrap();
        assert_eq!(fields.required::<NonZeroU8>(1, "one"), Ok(NonZeroU8::MIN));
        assert_eq!(fields.finish(), Err(PayloadError::UnknownField("2".to_owned())));
        let mut fields = Fields::read(&value).unwrap();
        assert_eq!(fields.required::<NonZeroU8>(4, "four"), Err(PayloadError::Missing("four")));
        assert_eq!(fields.required::<Money>(1, "one"), Err(PayloadError::Invalid("one")));
        assert!(matches!(Fields::read(&Value::Null), Err(PayloadError::NotAMap)));
        let text_key = Value::Map(Map::from_entries([(Value::from("a"), Value::Null)]).unwrap());
        assert!(matches!(Fields::read(&text_key), Err(PayloadError::UnknownField(_))));
        assert_eq!(NonZeroU8::from_value(&Value::Unsigned(256)), None);
        assert_eq!(NonZeroU16::from_value(&Value::Unsigned(0)), None);
        assert_eq!(Change::Set(3).into_option(), Some(3));
        assert_eq!(Change::<u8>::Clear.as_ref(), Change::Clear);
    }
}
