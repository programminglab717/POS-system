//! Canonical CBOR: the byte-exact encoding of everything Keel hashes and signs.
//!
//! Keel encodes events with a strict subset of CBOR (RFC 8949): unsigned and negative integers,
//! byte strings, text strings, arrays, maps, booleans and null. Floating point, tags, undefined,
//! other simple values and indefinite lengths are not part of the subset.
//!
//! Every value is written in the *core deterministic encoding* of RFC 8949 §4.2.1: every integer
//! and length in its shortest form, and map entries sorted by the bytes of their encoded keys.
//!
//! The decoder is just as strict. It accepts only canonical encodings, so each value has exactly
//! one valid encoding, and re-encoding a decoded value reproduces the input byte for byte. That
//! makes hashes and signatures reproducible, and means no event can be re-encoded into a
//! different but equivalent form.

use core::fmt;

/// The deepest nesting of arrays and maps that [`decode`] accepts. Deeper values can be encoded
/// but not decoded; keep payloads shallow.
pub const MAX_DEPTH: usize = 32;

/// A value in Keel's CBOR subset.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    /// An unsigned integer, 0 to 2^64 − 1.
    Unsigned(u64),
    /// The negative integer `-1 - n`, for `n` from 0 to 2^64 − 1: −1 is `Negative(0)`.
    Negative(u64),
    /// A byte string.
    Bytes(Vec<u8>),
    /// A text string (always valid UTF-8).
    Text(String),
    /// An array.
    Array(Vec<Value>),
    /// A map, with its entries in canonical order.
    Map(Map),
    /// `false` or `true`.
    Bool(bool),
    /// `null`.
    Null,
}

impl Value {
    /// The integer `n`: unsigned if `n >= 0`, negative otherwise.
    pub fn integer(n: i64) -> Value {
        // CBOR stores a negative integer n as -1 - n, which is exactly its bitwise complement.
        match u64::try_from(n) {
            Ok(unsigned) => Value::Unsigned(unsigned),
            Err(_) => Value::Negative(u64::try_from(!n).unwrap_or(u64::MAX)),
        }
    }

    /// The value as an unsigned integer, if it is one.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Unsigned(n) => Some(*n),
            _ => None,
        }
    }

    /// The value as an `i64`, if it is an integer in the `i64` range.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Unsigned(n) => i64::try_from(*n).ok(),
            Value::Negative(n) => i64::try_from(*n).ok().map(|n| !n),
            _ => None,
        }
    }

    /// The value as a byte string, if it is one.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// The value as a text string, if it is one.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The value as an array, if it is one.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The value as a map, if it is one.
    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Value::Map(map) => Some(map),
            _ => None,
        }
    }

    /// The value as a boolean, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The canonical encoding of the value.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    /// Appends the canonical encoding of the value to `out`.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Value::Unsigned(n) => write_head(out, Major::Unsigned, *n),
            Value::Negative(n) => write_head(out, Major::Negative, *n),
            Value::Bytes(bytes) => {
                write_head(out, Major::Bytes, length(bytes.len()));
                out.extend_from_slice(bytes);
            }
            Value::Text(text) => {
                write_head(out, Major::Text, length(text.len()));
                out.extend_from_slice(text.as_bytes());
            }
            Value::Array(items) => {
                write_head(out, Major::Array, length(items.len()));
                for item in items {
                    item.encode_into(out);
                }
            }
            Value::Map(map) => {
                write_head(out, Major::Map, length(map.len()));
                for (key, value) in map.iter() {
                    key.encode_into(out);
                    value.encode_into(out);
                }
            }
            Value::Bool(false) => out.push(FALSE),
            Value::Bool(true) => out.push(TRUE),
            Value::Null => out.push(NULL),
        }
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Value {
        Value::Unsigned(n)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Value {
        Value::integer(n)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::Bool(value)
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::Text(text.to_owned())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::Text(text)
    }
}

impl From<&[u8]> for Value {
    fn from(bytes: &[u8]) -> Value {
        Value::Bytes(bytes.to_vec())
    }
}

impl From<Vec<u8>> for Value {
    fn from(bytes: Vec<u8>) -> Value {
        Value::Bytes(bytes)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Value {
        Value::Array(items)
    }
}

impl From<Map> for Value {
    fn from(map: Map) -> Value {
        Value::Map(map)
    }
}

impl fmt::Display for Value {
    /// A debugging view in the style of CBOR diagnostic notation: `{1: h'00ff', 2: [-1, "a"]}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Unsigned(n) => write!(f, "{n}"),
            Value::Negative(n) => write!(f, "-{}", u128::from(*n).saturating_add(1)),
            Value::Bytes(bytes) => {
                f.write_str("h'")?;
                for byte in bytes {
                    write!(f, "{byte:02x}")?;
                }
                f.write_str("'")
            }
            Value::Text(text) => write!(f, "{text:?}"),
            Value::Array(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Value::Map(map) => {
                f.write_str("{")?;
                for (index, (key, value)) in map.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key}: {value}")?;
                }
                f.write_str("}")
            }
            Value::Bool(value) => write!(f, "{value}"),
            Value::Null => f.write_str("null"),
        }
    }
}

/// A CBOR map whose entries are always in canonical order: sorted by the bytes of their encoded
/// keys, with no key repeated.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Map {
    entries: Vec<(Value, Value)>,
}

impl Map {
    /// An empty map.
    pub const fn new() -> Map {
        Map { entries: Vec::new() }
    }

    /// A map of `entries`, in any order.
    ///
    /// # Errors
    /// [`DuplicateKey`] if two entries have equal keys.
    pub fn from_entries<I>(entries: I) -> Result<Map, DuplicateKey>
    where
        I: IntoIterator<Item = (Value, Value)>,
    {
        let mut keyed: Vec<(Vec<u8>, Value, Value)> =
            entries.into_iter().map(|(key, value)| (key.encode(), key, value)).collect();
        keyed.sort_by(|left, right| left.0.cmp(&right.0));
        if let Some([(_, key, _), _]) =
            keyed.windows(2).find(|pair| matches!(pair, [left, right] if left.0 == right.0))
        {
            return Err(DuplicateKey(key.clone()));
        }
        Ok(Map { entries: keyed.into_iter().map(|(_, key, value)| (key, value)).collect() })
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value stored under `key`.
    pub fn get(&self, key: &Value) -> Option<&Value> {
        self.entries.iter().find(|(entry_key, _)| entry_key == key).map(|(_, value)| value)
    }

    /// The entries, in canonical order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&Value, &Value)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }

    /// The entries, in canonical order.
    pub fn into_entries(self) -> Vec<(Value, Value)> {
        self.entries
    }
}

/// Two map entries had equal keys.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("duplicate map key {0}")]
pub struct DuplicateKey(pub Value);

/// Decodes exactly one canonical CBOR data item, which must fill `bytes` completely.
///
/// # Errors
/// [`CborError`] if the input isn't a single, canonically encoded item of Keel's subset. The
/// error's offset is the position of the offending byte.
pub fn decode(bytes: &[u8]) -> Result<Value, CborError> {
    let mut reader = Reader { input: bytes, position: 0 };
    let value = reader.value(0)?;
    if reader.position == bytes.len() {
        Ok(value)
    } else {
        Err(error_at(reader.position, CborErrorKind::TrailingBytes))
    }
}

/// Why bytes aren't canonical CBOR in Keel's subset, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{kind} (at byte {offset})")]
pub struct CborError {
    /// What was wrong.
    pub kind: CborErrorKind,
    /// The position of the offending byte in the input.
    pub offset: usize,
}

/// The ways bytes can fail to be canonical CBOR in Keel's subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CborErrorKind {
    /// The input ended inside a data item, or a length exceeds what remains.
    #[error("input ends inside a data item")]
    Truncated,
    /// Bytes followed the data item.
    #[error("bytes after the data item")]
    TrailingBytes,
    /// An integer or length wasn't in its shortest form.
    #[error("integer or length not in its shortest form")]
    NotShortest,
    /// An indefinite-length item, or a stray "break" byte.
    #[error("indefinite lengths are not allowed")]
    IndefiniteLength,
    /// Additional information 28 to 30, which CBOR reserves.
    #[error("reserved additional information")]
    Reserved,
    /// A tag.
    #[error("tags are not allowed")]
    Tag,
    /// Floating point, undefined, or a simple value other than false, true and null.
    #[error("floating point and simple values other than false, true and null are not allowed")]
    UnsupportedSimpleValue,
    /// A text string that isn't valid UTF-8.
    #[error("text is not valid UTF-8")]
    InvalidUtf8,
    /// Map keys not in ascending order of their encoded bytes.
    #[error("map keys are not in canonical order")]
    UnsortedKeys,
    /// A map key that repeats the previous one.
    #[error("duplicate map key")]
    DuplicateKey,
    /// Arrays and maps nested deeper than [`MAX_DEPTH`].
    #[error("nesting deeper than {MAX_DEPTH} levels")]
    TooDeep,
}

/// CBOR major types.
#[derive(Clone, Copy)]
enum Major {
    Unsigned,
    Negative,
    Bytes,
    Text,
    Array,
    Map,
}

impl Major {
    const fn bits(self) -> u8 {
        match self {
            Major::Unsigned => 0x00,
            Major::Negative => 0x20,
            Major::Bytes => 0x40,
            Major::Text => 0x60,
            Major::Array => 0x80,
            Major::Map => 0xA0,
        }
    }
}

const FALSE: u8 = 0xF4;
const TRUE: u8 = 0xF5;
const NULL: u8 = 0xF6;

/// Additional information announcing an argument in the next 1, 2, 4 or 8 bytes.
const ARGUMENT_1: u8 = 0x18;
const ARGUMENT_2: u8 = 0x19;
const ARGUMENT_4: u8 = 0x1A;
const ARGUMENT_8: u8 = 0x1B;
/// Additional information announcing an indefinite length.
const INDEFINITE: u8 = 0x1F;

/// A length as a CBOR argument. Lengths of in-memory data always fit in a `u64`.
fn length(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// Writes a data item's head: its major type and argument, in the shortest form.
fn write_head(out: &mut Vec<u8>, major: Major, argument: u64) {
    let major = major.bits();
    let [b0, b1, b2, b3, b4, b5, b6, b7] = argument.to_be_bytes();
    match argument {
        0..=23 => out.push(major | b7),
        24..=0xFF => out.extend_from_slice(&[major | ARGUMENT_1, b7]),
        0x100..=0xFFFF => out.extend_from_slice(&[major | ARGUMENT_2, b6, b7]),
        0x1_0000..=0xFFFF_FFFF => out.extend_from_slice(&[major | ARGUMENT_4, b4, b5, b6, b7]),
        _ => out.extend_from_slice(&[major | ARGUMENT_8, b0, b1, b2, b3, b4, b5, b6, b7]),
    }
}

fn error_at(offset: usize, kind: CborErrorKind) -> CborError {
    CborError { kind, offset }
}

/// A strict, canonical-only CBOR reader over a byte slice.
struct Reader<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.position)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CborError> {
        let slice = self
            .position
            .checked_add(count)
            .and_then(|end| self.input.get(self.position..end))
            .ok_or_else(|| error_at(self.position, CborErrorKind::Truncated))?;
        self.position = self.position.saturating_add(count);
        Ok(slice)
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], CborError> {
        let start = self.position;
        let slice = self.take(N)?;
        <[u8; N]>::try_from(slice).map_err(|_| error_at(start, CborErrorKind::Truncated))
    }

    /// Reads the argument that follows an initial byte with additional information `info`,
    /// insisting on the shortest form.
    fn argument(&mut self, info: u8, start: usize) -> Result<u64, CborError> {
        let not_shortest = error_at(start, CborErrorKind::NotShortest);
        match info {
            0..=23 => Ok(u64::from(info)),
            ARGUMENT_1 => {
                let [byte] = self.take_array::<1>()?;
                if byte < 24 { Err(not_shortest) } else { Ok(u64::from(byte)) }
            }
            ARGUMENT_2 => {
                let value = u16::from_be_bytes(self.take_array::<2>()?);
                if value <= 0xFF { Err(not_shortest) } else { Ok(u64::from(value)) }
            }
            ARGUMENT_4 => {
                let value = u32::from_be_bytes(self.take_array::<4>()?);
                if value <= 0xFFFF { Err(not_shortest) } else { Ok(u64::from(value)) }
            }
            ARGUMENT_8 => {
                let value = u64::from_be_bytes(self.take_array::<8>()?);
                if value <= 0xFFFF_FFFF { Err(not_shortest) } else { Ok(value) }
            }
            INDEFINITE => Err(error_at(start, CborErrorKind::IndefiniteLength)),
            _ => Err(error_at(start, CborErrorKind::Reserved)),
        }
    }

    /// A length or count, which must fit in what remains of the input: every byte of a string,
    /// and every element of an array or map, takes at least one byte. This check comes before
    /// any allocation, so a hostile length can't exhaust memory.
    fn count(
        &self,
        argument: u64,
        bytes_per_item: usize,
        start: usize,
    ) -> Result<usize, CborError> {
        usize::try_from(argument)
            .ok()
            .filter(|&count| {
                count.checked_mul(bytes_per_item).is_some_and(|needed| needed <= self.remaining())
            })
            .ok_or_else(|| error_at(start, CborErrorKind::Truncated))
    }

    fn value(&mut self, depth: usize) -> Result<Value, CborError> {
        let start = self.position;
        let [initial] = self.take_array::<1>()?;
        let major = initial.wrapping_shr(5);
        let info = initial & 0x1F;
        if major == 7 {
            return match initial {
                FALSE => Ok(Value::Bool(false)),
                TRUE => Ok(Value::Bool(true)),
                NULL => Ok(Value::Null),
                0xFF => Err(error_at(start, CborErrorKind::IndefiniteLength)),
                0xFC..=0xFE => Err(error_at(start, CborErrorKind::Reserved)),
                _ => Err(error_at(start, CborErrorKind::UnsupportedSimpleValue)),
            };
        }
        if major == 6 {
            return Err(error_at(start, CborErrorKind::Tag));
        }
        let argument = self.argument(info, start)?;
        match major {
            0 => Ok(Value::Unsigned(argument)),
            1 => Ok(Value::Negative(argument)),
            2 => {
                let len = self.count(argument, 1, start)?;
                Ok(Value::Bytes(self.take(len)?.to_vec()))
            }
            3 => {
                let len = self.count(argument, 1, start)?;
                let bytes = self.take(len)?.to_vec();
                String::from_utf8(bytes)
                    .map(Value::Text)
                    .map_err(|_| error_at(start, CborErrorKind::InvalidUtf8))
            }
            4 => {
                let child_depth = enter(depth, start)?;
                let count = self.count(argument, 1, start)?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.value(child_depth)?);
                }
                Ok(Value::Array(items))
            }
            5 => {
                let child_depth = enter(depth, start)?;
                let count = self.count(argument, 2, start)?;
                let mut entries = Vec::with_capacity(count);
                let mut previous_key: Option<&'a [u8]> = None;
                for _ in 0..count {
                    let key_start = self.position;
                    let key = self.value(child_depth)?;
                    let key_bytes = self
                        .input
                        .get(key_start..self.position)
                        .ok_or_else(|| error_at(key_start, CborErrorKind::Truncated))?;
                    if let Some(previous) = previous_key {
                        match key_bytes.cmp(previous) {
                            core::cmp::Ordering::Less => {
                                return Err(error_at(key_start, CborErrorKind::UnsortedKeys));
                            }
                            core::cmp::Ordering::Equal => {
                                return Err(error_at(key_start, CborErrorKind::DuplicateKey));
                            }
                            core::cmp::Ordering::Greater => {}
                        }
                    }
                    previous_key = Some(key_bytes);
                    let value = self.value(child_depth)?;
                    entries.push((key, value));
                }
                Ok(Value::Map(Map { entries }))
            }
            _ => Err(error_at(start, CborErrorKind::Reserved)),
        }
    }
}

/// Enters an array or map at `depth`, returning its children's depth.
fn enter(depth: usize, start: usize) -> Result<usize, CborError> {
    if depth >= MAX_DEPTH {
        return Err(error_at(start, CborErrorKind::TooDeep));
    }
    depth.checked_add(1).ok_or_else(|| error_at(start, CborErrorKind::TooDeep))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn map(entries: Vec<(Value, Value)>) -> Value {
        Value::Map(Map::from_entries(entries).unwrap())
    }

    fn text(value: &str) -> Value {
        Value::from(value)
    }

    fn int(value: i64) -> Value {
        Value::integer(value)
    }

    fn array(items: Vec<Value>) -> Value {
        Value::Array(items)
    }

    /// RFC 8949 Appendix A examples within Keel's subset; the encodings were confirmed with an
    /// independent implementation (Python's cbor2).
    #[test]
    fn rfc_8949_appendix_a() {
        let examples: Vec<(Value, &str)> = vec![
            (int(0), "00"),
            (int(1), "01"),
            (int(10), "0a"),
            (int(23), "17"),
            (int(24), "1818"),
            (int(25), "1819"),
            (int(100), "1864"),
            (int(1000), "1903e8"),
            (int(1_000_000), "1a000f4240"),
            (int(1_000_000_000_000), "1b000000e8d4a51000"),
            (Value::Unsigned(u64::MAX), "1bffffffffffffffff"),
            (Value::Negative(u64::MAX), "3bffffffffffffffff"),
            (int(-1), "20"),
            (int(-10), "29"),
            (int(-100), "3863"),
            (int(-1000), "3903e7"),
            (Value::Bool(false), "f4"),
            (Value::Bool(true), "f5"),
            (Value::Null, "f6"),
            (Value::Bytes(vec![]), "40"),
            (Value::Bytes(vec![1, 2, 3, 4]), "4401020304"),
            (text(""), "60"),
            (text("a"), "6161"),
            (text("IETF"), "6449455446"),
            (text("\"\\"), "62225c"),
            (text("\u{fc}"), "62c3bc"),
            (text("\u{6c34}"), "63e6b0b4"),
            (text("\u{10151}"), "64f0908591"),
            (array(vec![]), "80"),
            (array(vec![int(1), int(2), int(3)]), "83010203"),
            (
                array(vec![int(1), array(vec![int(2), int(3)]), array(vec![int(4), int(5)])]),
                "8301820203820405",
            ),
            (
                array((1..=25).map(int).collect()),
                "98190102030405060708090a0b0c0d0e0f101112131415161718181819",
            ),
            (map(vec![]), "a0"),
            (map(vec![(int(1), int(2)), (int(3), int(4))]), "a201020304"),
            (
                map(vec![(text("a"), int(1)), (text("b"), array(vec![int(2), int(3)]))]),
                "a26161016162820203",
            ),
            (array(vec![text("a"), map(vec![(text("b"), text("c"))])]), "826161a161626163"),
            (
                map(["a", "b", "c", "d", "e"]
                    .into_iter()
                    .map(|key| (text(key), text(&key.to_uppercase())))
                    .collect()),
                "a56161614161626142616361436164614461656145",
            ),
        ];
        for (value, expected) in examples {
            assert_eq!(value.encode(), hex(expected), "encode {value}");
            assert_eq!(decode(&hex(expected)), Ok(value.clone()), "decode {expected}");
        }
    }

    /// The key-ordering example of RFC 8949 §4.2.1: bytewise order of the encoded keys, which
    /// differs from the older "shorter first" rule (-1 sorts after 100).
    #[test]
    fn map_keys_sort_by_encoded_bytes() {
        let keys = vec![
            int(10),
            int(100),
            int(-1),
            text("z"),
            text("aa"),
            array(vec![int(100)]),
            array(vec![int(-1)]),
            Value::Bool(false),
        ];
        let mut shuffled: Vec<(Value, Value)> =
            keys.iter().rev().cloned().map(|key| (key, Value::Null)).collect();
        shuffled.rotate_left(3);
        let map = Map::from_entries(shuffled).unwrap();
        let sorted: Vec<Value> = map.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(sorted, keys);
        let encoded = Value::Map(map).encode();
        let expected = concat!(
            "a8", "0af6", "1864f6", "20f6", "617af6", "626161f6", "811864f6", "8120f6", "f4f6"
        );
        assert_eq!(encoded, hex(expected));
        assert_eq!(
            Map::from_entries(vec![(int(1), int(1)), (int(1), int(2))]),
            Err(DuplicateKey(int(1)))
        );
    }

    #[test]
    fn non_canonical_encodings_are_rejected() {
        use CborErrorKind::*;
        let cases: &[(&str, CborErrorKind, usize)] = &[
            ("1817", NotShortest, 0),               // 23 in two bytes
            ("190017", NotShortest, 0),             // 23 in three bytes
            ("1900ff", NotShortest, 0),             // 255 in three bytes
            ("1a0000ffff", NotShortest, 0),         // 65535 in five bytes
            ("1b00000000ffffffff", NotShortest, 0), // 2^32 - 1 in nine bytes
            ("3817", NotShortest, 0),               // -24 in two bytes
            ("5801ff", NotShortest, 0),             // a 1-byte string with a 2-byte length
            ("8200", Truncated, 0), // array of 2 with one element: rejected up front
            ("5f41ff", IndefiniteLength, 0), // indefinite-length byte string
            ("9f01ff", IndefiniteLength, 0), // indefinite-length array
            ("ff", IndefiniteLength, 0), // stray break
            ("1c", Reserved, 0),
            ("3d", Reserved, 0),
            ("c11a514b67b0", Tag, 0),              // tag 1 (epoch time)
            ("f7", UnsupportedSimpleValue, 0),     // undefined
            ("f0", UnsupportedSimpleValue, 0),     // simple(16)
            ("f820", UnsupportedSimpleValue, 0),   // simple(32)
            ("f93c00", UnsupportedSimpleValue, 0), // half-precision 1.0
            ("fb3ff0000000000000", UnsupportedSimpleValue, 0), // double 1.0
            ("62c328", InvalidUtf8, 0),
            ("a203040102", UnsortedKeys, 3), // {3: 4, 1: 2}
            ("a201020102", DuplicateKey, 3), // {1: 2, 1: 2}
            ("a2200a0a0b", UnsortedKeys, 3), // {-1: 10, 10: 11}: 0x0a sorts before 0x20
            ("0000", TrailingBytes, 1),
            ("", Truncated, 0),
            ("19", Truncated, 1),
            ("6361", Truncated, 0), // claims 3 bytes, has 1
        ];
        for &(input, kind, offset) in cases {
            assert_eq!(decode(&hex(input)), Err(CborError { kind, offset }), "{input}");
        }
        // A string whose claimed length exceeds the input is rejected before allocation.
        assert_eq!(
            decode(&hex("5bffffffffffffffff")),
            Err(CborError { kind: Truncated, offset: 0 })
        );
        assert_eq!(
            decode(&hex("9bffffffffffffffff")),
            Err(CborError { kind: Truncated, offset: 0 })
        );
        assert_eq!(decode(&hex("63616263")), Ok(text("abc")));
    }

    #[test]
    fn nesting_is_limited() {
        let mut value = int(0);
        for _ in 0..MAX_DEPTH {
            value = array(vec![value]);
        }
        assert_eq!(decode(&value.encode()), Ok(value.clone()));
        let too_deep = array(vec![value]);
        assert_eq!(
            decode(&too_deep.encode()),
            Err(CborError { kind: CborErrorKind::TooDeep, offset: MAX_DEPTH })
        );
    }

    #[test]
    fn integer_helpers() {
        assert_eq!(Value::integer(i64::MIN), Value::Negative(9_223_372_036_854_775_807));
        assert_eq!(Value::integer(i64::MIN).as_i64(), Some(i64::MIN));
        assert_eq!(Value::integer(-1), Value::Negative(0));
        assert_eq!(Value::Negative(u64::MAX).as_i64(), None);
        assert_eq!(Value::Unsigned(u64::MAX).as_i64(), None);
        assert_eq!(Value::Unsigned(7).as_u64(), Some(7));
        assert_eq!(Value::Negative(0).as_u64(), None);
    }

    #[test]
    fn diagnostic_display() {
        let value = map(vec![
            (int(1), Value::Bytes(vec![0, 255])),
            (int(2), array(vec![int(-1), text("a"), Value::Bool(true), Value::Null])),
            (int(3), Value::Negative(u64::MAX)),
        ]);
        assert_eq!(
            value.to_string(),
            "{1: h'00ff', 2: [-1, \"a\", true, null], 3: -18446744073709551616}"
        );
    }
}
