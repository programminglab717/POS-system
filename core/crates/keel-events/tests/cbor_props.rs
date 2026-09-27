//! Property tests: the canonical CBOR codec round-trips every value, accepts exactly one encoding
//! per value, never panics on hostile input, and agrees with an independent implementation.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_events::cbor::{self, CborErrorKind, Map, Value};
use proptest::prelude::*;
use support::{any_mutation, any_value, map_of, mutate};

/// The independent implementation's reading of CBOR bytes, in our data model.
fn ciborium_reading(bytes: &[u8]) -> Option<Value> {
    fn convert(value: ciborium::Value) -> Option<Value> {
        Some(match value {
            ciborium::Value::Integer(integer) => {
                let n = i128::from(integer);
                if n >= 0 {
                    Value::Unsigned(u64::try_from(n).ok()?)
                } else {
                    Value::Negative(u64::try_from(-1 - n).ok()?)
                }
            }
            ciborium::Value::Bytes(bytes) => Value::Bytes(bytes),
            ciborium::Value::Text(text) => Value::Text(text),
            ciborium::Value::Array(items) => {
                Value::Array(items.into_iter().map(convert).collect::<Option<_>>()?)
            }
            ciborium::Value::Map(entries) => {
                let entries = entries
                    .into_iter()
                    .map(|(key, value)| Some((convert(key)?, convert(value)?)))
                    .collect::<Option<Vec<_>>>()?;
                Value::Map(Map::from_entries(entries).ok()?)
            }
            ciborium::Value::Bool(value) => Value::Bool(value),
            ciborium::Value::Null => Value::Null,
            _ => return None,
        })
    }
    convert(ciborium::de::from_reader(bytes).ok()?)
}

/// Maps with at least two entries, filtered in the strategy rather than rejected in the test.
fn maps_with_two_or_more_entries() -> impl Strategy<Value = Map> {
    prop::collection::vec((any_value(), any_value()), 2..6).prop_map(map_of).prop_filter_map(
        "at least two distinct keys",
        |value| match value {
            Value::Map(map) if map.len() >= 2 => Some(map),
            _ => None,
        },
    )
}

proptest! {
    /// Every value survives encoding and decoding.
    #[test]
    fn round_trip(value in any_value()) {
        let encoded = value.encode();
        prop_assert_eq!(cbor::decode(&encoded), Ok(value));
    }

    /// Our encodings are valid CBOR that an independent implementation reads the same way.
    #[test]
    fn independent_decoder_agrees(value in any_value()) {
        prop_assert_eq!(ciborium_reading(&value.encode()), Some(value));
    }

    /// One encoding per value: whenever the decoder accepts bytes, even bytes one mutation away
    /// from a valid encoding, re-encoding reproduces them exactly, and an independent decoder
    /// reads the same value. Otherwise it returns an error; it never panics.
    #[test]
    fn accepted_bytes_are_exactly_canonical(value in any_value(), mutation in any_mutation()) {
        let bytes = mutate(value.encode(), &mutation);
        if let Ok(decoded) = cbor::decode(&bytes) {
            prop_assert_eq!(decoded.encode(), bytes.clone());
            prop_assert_eq!(ciborium_reading(&bytes), Some(decoded));
        }
    }

    /// Arbitrary bytes never make the decoder panic, and anything accepted is canonical.
    #[test]
    fn arbitrary_bytes_are_handled(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
        if let Ok(decoded) = cbor::decode(&bytes) {
            prop_assert_eq!(decoded.encode(), bytes);
        }
    }

    /// Map entries are encoded in ascending key order, and swapping two adjacent entries in an
    /// encoding is rejected.
    #[test]
    fn map_order_is_enforced(map in maps_with_two_or_more_entries()) {
        let keys: Vec<Vec<u8>> = map.iter().map(|(key, _)| key.encode()).collect();
        prop_assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));

        // Re-encode with the first two entries swapped.
        let mut swapped = Value::Map(Map::new()).encode();
        swapped[0] |= u8::try_from(map.len()).unwrap(); // map header with the right count
        let entries: Vec<(&Value, &Value)> = map.iter().collect();
        for (key, value) in [entries[1], entries[0]].into_iter().chain(entries[2..].iter().copied()) {
            key.encode_into(&mut swapped);
            value.encode_into(&mut swapped);
        }
        let error = cbor::decode(&swapped).unwrap_err();
        prop_assert_eq!(error.kind, CborErrorKind::UnsortedKeys);
    }
}

/// Ways to encode a value validly but not canonically (or, for a duplicated entry, not validly
/// in Keel's subset). Applied at one node of the value, found by its position in a pre-order walk.
#[derive(Clone, Copy, Debug)]
enum Deviation {
    /// Write the node's head in a wider form than necessary; `steps` picks how much wider.
    WidenHead { steps: u8 },
    /// Write a string, array or map with indefinite length (strings as two chunks).
    Indefinite,
    /// Write a map's first two entries in the opposite order.
    SwapEntries,
    /// Write a map's first entry twice.
    DuplicateEntry,
}

fn any_deviation() -> impl Strategy<Value = Deviation> {
    prop_oneof![
        (1_u8..=4).prop_map(|steps| Deviation::WidenHead { steps }),
        Just(Deviation::Indefinite),
        Just(Deviation::SwapEntries),
        Just(Deviation::DuplicateEntry),
    ]
}

/// An encoder independent of the one under test, able to deviate from canonical form at one node.
struct DeviantEncoder {
    target: usize,
    deviation: Deviation,
    visited: usize,
    applied: bool,
    out: Vec<u8>,
}

impl DeviantEncoder {
    /// Writes a head with the argument in `width` bytes (0 means inside the initial byte).
    fn head(&mut self, major: u8, argument: u64, width: usize) {
        let info = match width {
            0 => u8::try_from(argument).unwrap(),
            1 => 24,
            2 => 25,
            4 => 26,
            _ => 27,
        };
        self.out.push(major << 5 | info);
        let bytes = argument.to_be_bytes();
        self.out.extend_from_slice(&bytes[8 - width.min(8)..]);
    }

    fn shortest_width(argument: u64) -> usize {
        match argument {
            0..=23 => 0,
            24..=0xFF => 1,
            0x100..=0xFFFF => 2,
            0x1_0000..=0xFFFF_FFFF => 4,
            _ => 8,
        }
    }

    fn write(&mut self, value: &Value) {
        let here = self.visited == self.target;
        self.visited += 1;
        let deviation = if here { Some(self.deviation) } else { None };
        let (major, argument) = match value {
            Value::Unsigned(n) => (0, *n),
            Value::Negative(n) => (1, *n),
            Value::Bytes(bytes) => (2, len(bytes.len())),
            Value::Text(text) => (3, len(text.len())),
            Value::Array(items) => (4, len(items.len())),
            Value::Map(map) => (5, len(map.len())),
            Value::Bool(false) => return self.out.push(0xF4),
            Value::Bool(true) => return self.out.push(0xF5),
            Value::Null => return self.out.push(0xF6),
        };
        let shortest = Self::shortest_width(argument);
        let widths = [0, 1, 2, 4, 8];
        let mut width = shortest;
        match deviation {
            Some(Deviation::WidenHead { steps }) if shortest < 8 => {
                let position = widths.iter().position(|&w| w == shortest).unwrap();
                width = widths[(position + usize::from(steps)).min(4)];
                self.applied = true;
            }
            Some(Deviation::Indefinite) if matches!(value, Value::Bytes(_) | Value::Text(_)) => {
                self.applied = true;
                // Two chunks, split on a character boundary for text.
                let (bytes, split): (&[u8], usize) = match value {
                    Value::Text(text) => {
                        let middle = (0..=text.len() / 2).rev().find(|&i| text.is_char_boundary(i));
                        (text.as_bytes(), middle.unwrap_or(0))
                    }
                    Value::Bytes(bytes) => (bytes, bytes.len() / 2),
                    _ => (&[], 0),
                };
                self.out.push(major << 5 | 31);
                for chunk in [&bytes[..split], &bytes[split..]] {
                    self.head(major, len(chunk.len()), Self::shortest_width(len(chunk.len())));
                    self.out.extend_from_slice(chunk);
                }
                return self.out.push(0xFF);
            }
            Some(Deviation::Indefinite) if matches!(value, Value::Array(_) | Value::Map(_)) => {
                self.applied = true;
                self.out.push(major << 5 | 31);
                self.children(value, None);
                return self.out.push(0xFF);
            }
            Some(Deviation::SwapEntries) if matches!(value, Value::Map(map) if map.len() >= 2) => {
                self.applied = true;
                self.head(major, argument, shortest);
                return self.children(value, Some(Deviation::SwapEntries));
            }
            Some(Deviation::DuplicateEntry) if matches!(value, Value::Map(map) if !map.is_empty()) =>
            {
                self.applied = true;
                self.head(major, argument + 1, Self::shortest_width(argument + 1));
                return self.children(value, Some(Deviation::DuplicateEntry));
            }
            _ => {}
        }
        self.head(major, argument, width);
        match value {
            Value::Bytes(bytes) => self.out.extend_from_slice(bytes),
            Value::Text(text) => self.out.extend_from_slice(text.as_bytes()),
            Value::Array(_) | Value::Map(_) => self.children(value, None),
            _ => {}
        }
    }

    fn children(&mut self, value: &Value, map_deviation: Option<Deviation>) {
        match value {
            Value::Array(items) => items.iter().for_each(|item| self.write(item)),
            Value::Map(map) => {
                let mut entries: Vec<(&Value, &Value)> = map.iter().collect();
                match map_deviation {
                    Some(Deviation::SwapEntries) => entries.swap(0, 1),
                    Some(Deviation::DuplicateEntry) => entries.insert(1, entries[0]),
                    _ => {}
                }
                for (key, value) in entries {
                    self.write(key);
                    self.write(value);
                }
            }
            _ => {}
        }
    }
}

fn len(n: usize) -> u64 {
    u64::try_from(n).unwrap()
}

/// Whether `deviation` can be applied to this node.
fn applies_to(value: &Value, deviation: Deviation) -> bool {
    match deviation {
        Deviation::WidenHead { .. } => match value {
            Value::Unsigned(n) | Value::Negative(n) => *n <= 0xFFFF_FFFF,
            Value::Bool(_) | Value::Null => false,
            _ => true,
        },
        Deviation::Indefinite => {
            matches!(value, Value::Bytes(_) | Value::Text(_) | Value::Array(_) | Value::Map(_))
        }
        Deviation::SwapEntries => matches!(value, Value::Map(map) if map.len() >= 2),
        Deviation::DuplicateEntry => matches!(value, Value::Map(map) if !map.is_empty()),
    }
}

/// The pre-order positions of the nodes `deviation` can be applied to.
fn eligible_nodes(value: &Value, deviation: Deviation) -> Vec<usize> {
    fn walk(value: &Value, deviation: Deviation, next: &mut usize, found: &mut Vec<usize>) {
        if applies_to(value, deviation) {
            found.push(*next);
        }
        *next += 1;
        match value {
            Value::Array(items) => items.iter().for_each(|item| walk(item, deviation, next, found)),
            Value::Map(map) => map.iter().for_each(|(key, value)| {
                walk(key, deviation, next, found);
                walk(value, deviation, next, found);
            }),
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(value, deviation, &mut 0, &mut found);
    found
}

/// A value, a deviation, and a node of the value the deviation applies to.
fn deviant_case() -> impl Strategy<Value = (Value, Deviation, usize)> {
    (any_value(), any_deviation(), any::<usize>()).prop_filter_map(
        "the deviation applies somewhere in the value",
        |(value, deviation, pick)| {
            let nodes = eligible_nodes(&value, deviation);
            let target = *nodes.get(pick % nodes.len().max(1))?;
            Some((value, deviation, target))
        },
    )
}

proptest! {
    /// Every other encoding of a value is rejected: wider heads, indefinite lengths and
    /// reordered entries (which an independent decoder confirms are valid encodings of the same
    /// value), and duplicated entries.
    #[test]
    fn every_other_encoding_is_rejected((value, deviation, target) in deviant_case()) {
        let mut encoder =
            DeviantEncoder { target, deviation, visited: 0, applied: false, out: Vec::new() };
        encoder.write(&value);
        prop_assert!(encoder.applied, "{:?} not applied at node {}", deviation, target);
        {
            let bytes = encoder.out;
            let error = cbor::decode(&bytes).unwrap_err();
            let expected = match deviation {
                Deviation::WidenHead { .. } => CborErrorKind::NotShortest,
                Deviation::Indefinite => CborErrorKind::IndefiniteLength,
                Deviation::SwapEntries => CborErrorKind::UnsortedKeys,
                Deviation::DuplicateEntry => CborErrorKind::DuplicateKey,
            };
            prop_assert_eq!(error.kind, expected);
            if !matches!(deviation, Deviation::DuplicateEntry) {
                prop_assert_eq!(ciborium_reading(&bytes), Some(value));
            }
        }
    }
}
