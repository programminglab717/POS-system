//! Property tests: event bodies round-trip, are written exactly as the format table says, have
//! exactly one accepted encoding, and signed events verify, with any change to them caught.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_events::cbor::{self, Map, Value};
use keel_events::envelope::{Actor, EventBody};
use keel_events::event::{SignedEvent, UnverifiedEvent};
use keel_events::hash::EventHash;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_types::{Id, SeededEntropy};
use proptest::prelude::*;
use support::{any_body, any_mutation, any_value, boundary_u64, mutate, uuid_v7_bytes};

/// The body as the format table in the envelope's documentation describes it, built with an
/// independent CBOR implementation.
fn documented_encoding(body: &EventBody) -> ciborium::Value {
    use ciborium::Value as C;
    let int = |n: u64| C::Integer(n.into());
    let bytes = |bytes: &[u8]| C::Bytes(bytes.to_vec());
    let id = |uuid: [u8; 16]| C::Bytes(uuid.to_vec());
    let actor = match &body.actor {
        Actor::TeamMember(member) => C::Array(vec![int(0), id(member.to_bytes())]),
        Actor::Customer(customer) => C::Array(vec![int(1), id(customer.to_bytes())]),
        Actor::Integration(integration) => C::Array(vec![int(2), id(integration.to_bytes())]),
        Actor::Extension(extension) => C::Array(vec![int(3), id(extension.to_bytes())]),
        Actor::System(component) => C::Array(vec![int(4), C::Text(component.to_string())]),
    };
    let date = &body.business_date;
    let mut rows = vec![
        (0, int(1)),
        (1, id(body.event_id.to_bytes())),
        (2, id(body.location.to_bytes())),
        (3, C::Text(body.stream.kind.to_string())),
        (4, id(body.stream.id.to_bytes())),
        (5, C::Text(body.schema.name.to_string())),
        (6, int(u64::from(body.schema.version.get()))),
        (7, id(body.origin_device.to_bytes())),
        (8, int(body.origin_seq.get())),
        (9, int((body.hlc.wall_ms() << 16) | u64::from(body.hlc.logical()))),
        (10, C::Text(format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()))),
        (11, actor),
    ];
    let optional = [
        (12, body.approval.map(Id::to_bytes)),
        (13, body.causation.map(Id::to_bytes)),
        (14, body.correlation.map(Id::to_bytes)),
    ];
    rows.extend(optional.into_iter().filter_map(|(key, uuid)| Some((key, id(uuid?)))));
    rows.push((15, bytes(body.payload.as_bytes())));
    rows.push((16, bytes(body.prev_hash.as_bytes())));
    C::Map(rows.into_iter().map(|(key, value)| (int(key), value)).collect())
}

/// Values like the ones envelope fields hold, valid or nearly so, for any field.
fn near_miss_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any_value(),
        near_miss_id(),
        near_miss_name(),
        near_miss_date(),
        near_miss_unsigned(),
        near_miss_actor(),
        near_miss_bytes(),
    ]
}

fn near_miss_id() -> impl Strategy<Value = Value> {
    prop_oneof![
        6 => uuid_v7_bytes().prop_map(|id| Value::Bytes(id.to_vec())),
        2 => any::<[u8; 16]>().prop_map(|bytes| Value::Bytes(bytes.to_vec())),
        2 => prop::collection::vec(any::<u8>(), 15..=17).prop_map(Value::Bytes),
        1 => Just(Value::Null),
    ]
}

/// Identifiers and dotted names, straddling the length limits (32 and 64), some with one bad
/// character inserted.
fn near_miss_name() -> impl Strategy<Value = Value> {
    let valid =
        prop_oneof!["[a-z][a-z0-9_]{0,40}", "[a-z][a-z0-9_]{0,12}(\\.[a-z][a-z0-9_]{0,12}){0,6}",];
    let bad_character = prop::sample::select(vec!['A', 'Z', '.', '_', '-', ' ', '0', 'é']);
    prop_oneof![
        valid.clone().prop_map(Value::Text),
        (valid, any::<prop::sample::Index>(), bad_character).prop_map(|(name, at, character)| {
            let mut chars: Vec<char> = name.chars().collect();
            chars.insert(at.index(chars.len() + 1), character);
            Value::Text(chars.into_iter().collect())
        }),
        // Dotted names, some with empty segments.
        "\\.?[a-z][a-z0-9_]{0,8}(\\.\\.?[a-z][a-z0-9_]{0,8}){0,3}\\.?".prop_map(Value::Text),
        "[a-zA-Z0-9_.-]{0,70}".prop_map(Value::Text),
    ]
}

fn near_miss_date() -> impl Strategy<Value = Value> {
    prop_oneof![
        "[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])",
        "[ +0]?[0-9]{4}-(0?[1-9]|1[0-2])-(0?[1-9]|[12][0-9]|3[01])[ Z]?",
        "[0-9]{4}-[0-9]{2}-[0-9]{2}",
    ]
    .prop_map(Value::Text)
}

fn near_miss_unsigned() -> impl Strategy<Value = Value> {
    prop_oneof![
        3 => (0_u64..3).prop_map(Value::Unsigned),
        1 => boundary_u64().prop_map(Value::Unsigned),
        1 => any::<u64>().prop_map(Value::Unsigned),
        1 => prop::sample::select(vec![u64::from(u32::MAX), 1 << 32, (1 << 32) + 1])
            .prop_map(Value::Unsigned),
        1 => (0_u64..3).prop_map(Value::Negative),
    ]
}

fn near_miss_actor() -> impl Strategy<Value = Value> {
    let kind = || (0_u64..6).prop_map(Value::Unsigned);
    prop_oneof![
        (kind(), near_miss_id()).prop_map(|(kind, id)| Value::Array(vec![kind, id])),
        (kind(), near_miss_name()).prop_map(|(kind, name)| Value::Array(vec![kind, name])),
        (kind(), near_miss_id(), any_value())
            .prop_map(|(kind, id, extra)| Value::Array(vec![kind, id, extra])),
        kind().prop_map(|kind| Value::Array(vec![kind])),
    ]
}

fn near_miss_bytes() -> impl Strategy<Value = Value> {
    prop_oneof![
        any_value().prop_map(|value| value.encode()),
        prop::collection::vec(any::<u8>(), 0..8),
        prop::collection::vec(any::<u8>(), 31..=33),
    ]
    .prop_map(Value::Bytes)
}

/// Values aimed at the field under `key`, mostly, or at any field.
fn near_miss_for(key: u64) -> BoxedStrategy<Value> {
    let aimed = match key {
        1 | 2 | 4 | 7 | 12..=14 => near_miss_id().boxed(),
        3 | 5 => near_miss_name().boxed(),
        10 => near_miss_date().boxed(),
        0 | 6 | 8 | 9 => near_miss_unsigned().boxed(),
        11 => near_miss_actor().boxed(),
        _ => near_miss_bytes().boxed(),
    };
    prop_oneof![3 => aimed, 1 => near_miss_value()].boxed()
}

/// A change to one entry of an encoded body's map.
#[derive(Clone, Debug)]
enum FieldChange {
    /// Set a key, replacing any entry it had.
    Set { key: Value, value: Value },
    /// Remove a key's entry.
    Remove { key: u64 },
}

fn any_field_change() -> impl Strategy<Value = FieldChange> {
    let aimed = (0_u64..=17).prop_flat_map(|key| {
        near_miss_for(key)
            .prop_map(move |value| FieldChange::Set { key: Value::Unsigned(key), value })
    });
    prop_oneof![
        8 => aimed,
        1 => (-18_i64..0, near_miss_value())
            .prop_map(|(key, value)| FieldChange::Set { key: Value::integer(key), value }),
        1 => (any_value(), near_miss_value()).prop_map(|(key, value)| FieldChange::Set { key, value }),
        2 => (0_u64..=16).prop_map(|key| FieldChange::Remove { key }),
    ]
}

/// Whether a body with `value` under `key` is valid, by the rules the envelope documents: an
/// independent model of them.
fn valid_for(key: u64, value: &Value) -> bool {
    match key {
        0 => *value == Value::Unsigned(1),
        1 | 2 | 4 | 7 | 12..=14 => is_uuid_v7(value),
        3 => value.as_text().is_some_and(|kind| is_identifier(kind, 32)),
        5 => value.as_text().is_some_and(|name| {
            name.len() <= 64 && name.split('.').all(|part| is_identifier(part, 64))
        }),
        6 => value.as_u64().is_some_and(|version| (1..=u64::from(u32::MAX)).contains(&version)),
        8 => value.as_u64().is_some_and(|seq| seq >= 1),
        9 => value.as_u64().is_some(),
        10 => value.as_text().is_some_and(is_date),
        11 => match value.as_array() {
            Some([kind, id]) if matches!(kind.as_u64(), Some(0..=3)) => is_uuid_v7(id),
            Some([kind, name]) if kind.as_u64() == Some(4) => {
                name.as_text().is_some_and(|name| is_identifier(name, 32))
            }
            _ => false,
        },
        15 => value.as_bytes().is_some_and(|bytes| cbor::decode(bytes).is_ok()),
        16 => value.as_bytes().is_some_and(|bytes| bytes.len() == 32),
        _ => false,
    }
}

fn is_uuid_v7(value: &Value) -> bool {
    value.as_bytes().is_some_and(|b| b.len() == 16 && b[6] >> 4 == 7 && b[8] >> 6 == 0b10)
}

/// At most `max_len` characters: a lowercase ASCII letter, then lowercase ASCII letters, digits
/// and underscores.
fn is_identifier(text: &str, max_len: usize) -> bool {
    let chars: Vec<char> = text.chars().collect();
    !chars.is_empty()
        && chars.len() <= max_len
        && chars[0].is_ascii_lowercase()
        && chars.iter().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

/// `YYYY-MM-DD`: a real date of the proleptic Gregorian calendar, from year 1 to 9999.
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [year, month, day] = parts[..] else { return false };
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    if !(digits(year, 4) && digits(month, 2) && digits(day, 2)) {
        return false;
    }
    let [year, month, day] = [year, month, day].map(|part| part.parse::<u32>().unwrap());
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year >= 1 && (1..=days_in_month).contains(&day)
}

/// The body's encoding with `change` applied, re-encoded canonically.
fn changed(body: &EventBody, change: &FieldChange) -> Vec<u8> {
    let mut entries =
        cbor::decode(&body.encode()).unwrap().as_map().unwrap().clone().into_entries();
    match change {
        FieldChange::Set { key, value } => {
            entries.retain(|(existing, _)| existing != key);
            entries.push((key.clone(), value.clone()));
        }
        FieldChange::Remove { key } => {
            entries.retain(|(existing, _)| existing.as_u64() != Some(*key));
        }
    }
    Value::Map(Map::from_entries(entries).unwrap()).encode()
}

fn any_algorithm() -> impl Strategy<Value = SignatureAlgorithm> {
    prop_oneof![Just(SignatureAlgorithm::Es256), Just(SignatureAlgorithm::EdDsa)]
}

fn signer(algorithm: SignatureAlgorithm, seed: u64) -> SoftwareSigner {
    SoftwareSigner::generate(algorithm, &mut SeededEntropy::new(seed)).unwrap()
}

proptest! {
    /// Every body survives encoding and decoding.
    #[test]
    fn bodies_round_trip(body in any_body()) {
        prop_assert_eq!(EventBody::decode(&body.encode()), Ok(body));
    }

    /// Bodies are written exactly as the documented format table says, as read by an
    /// independent CBOR implementation.
    #[test]
    fn bodies_follow_the_documented_format(body in any_body()) {
        let encoded = body.encode();
        let read: ciborium::Value = ciborium::de::from_reader(encoded.as_slice()).unwrap();
        prop_assert_eq!(read, documented_encoding(&body));
    }

    /// Changing, removing or adding a field: the result is accepted exactly when the documented
    /// rules allow it, and what is accepted re-encodes to exactly the same bytes (one encoding
    /// per body).
    #[test]
    fn changed_fields_are_accepted_exactly_when_valid(
        body in any_body(),
        change in any_field_change(),
    ) {
        let bytes = changed(&body, &change);
        let valid = match &change {
            FieldChange::Set { key, value } => key.as_u64().is_some_and(|key| valid_for(key, value)),
            FieldChange::Remove { key } => matches!(key, 12..=14),
        };
        match EventBody::decode(&bytes) {
            Ok(decoded) => {
                prop_assert!(valid, "accepted an invalid change");
                prop_assert_eq!(decoded.encode(), bytes);
            }
            Err(error) => prop_assert!(!valid, "rejected a valid change: {}", error),
        }
    }

    /// The same, for small changes to the bytes; the decoder never panics.
    #[test]
    fn changed_bytes_are_rejected_or_canonical(body in any_body(), mutation in any_mutation()) {
        let bytes = mutate(body.encode(), &mutation);
        if let Ok(decoded) = EventBody::decode(&bytes) {
            prop_assert_eq!(decoded.encode(), bytes);
        }
    }

    /// Signed events verify with their signer's key, and are identified by the hash of their
    /// body alone: signing the same body with another key gives the same hash.
    #[test]
    fn signed_events_verify_and_are_identified_by_their_body(
        body in any_body(),
        algorithm in any_algorithm(),
        seed in any::<u64>(),
        other_algorithm in any_algorithm(),
    ) {
        let other = SignedEvent::sign(body.clone(), &signer(other_algorithm, seed ^ 1)).unwrap();
        let signer = signer(algorithm, seed);
        let event = SignedEvent::sign(body.clone(), &signer).unwrap();
        prop_assert_eq!(event.hash(), EventHash::of(&body.encode()));
        prop_assert_eq!(event.key_id(), signer.public_key().key_id());
        prop_assert_eq!(other.hash(), event.hash());

        let received = UnverifiedEvent::decode(&event.to_bytes()).unwrap();
        prop_assert_eq!(received.unverified_body(), &body);
        prop_assert_eq!(received.verify(signer.public_key()), Ok(event));
    }

    /// Any small change to a signed event's bytes is caught: they no longer decode, or no longer
    /// verify.
    #[test]
    fn any_change_to_a_signed_event_is_caught(
        body in any_body(),
        algorithm in any_algorithm(),
        seed in any::<u64>(),
        mutation in any_mutation(),
    ) {
        let signer = signer(algorithm, seed);
        let bytes = SignedEvent::sign(body, &signer).unwrap().to_bytes();
        let mutated = mutate(bytes.clone(), &mutation);
        let verified =
            UnverifiedEvent::decode(&mutated).map(|event| event.verify(signer.public_key()));
        prop_assert_eq!(matches!(verified, Ok(Ok(_))), mutated == bytes, "{:?} went unnoticed", mutation);
    }
}
