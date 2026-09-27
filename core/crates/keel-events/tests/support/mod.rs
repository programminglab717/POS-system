//! Shared support for property tests: strategies for CBOR values and event bodies, and small
//! mutations of encodings.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use core::num::{NonZeroU32, NonZeroU64};

use keel_events::cbor::{Map, Value};
use keel_events::envelope::{
    Actor, Aggregate, Cause, Component, Correlation, Customer, Device, Event, EventBody, Extension,
    Integration, Location, Payload, SchemaName, SchemaRef, StreamKind, StreamRef, TeamMember,
};
use keel_events::hash::EventHash;
use keel_events::log::EventDraft;
use keel_types::{BusinessDate, Hlc, Id, Timestamp};
use proptest::prelude::*;

/// Integers around every boundary where the encoded head changes size.
pub(crate) fn boundary_u64() -> impl Strategy<Value = u64> {
    prop::sample::select(vec![
        0,
        1,
        23,
        24,
        255,
        256,
        65_535,
        65_536,
        4_294_967_295,
        4_294_967_296,
        u64::MAX - 1,
        u64::MAX,
    ])
}

/// A map from arbitrary entries, keeping the first of any entries with equal keys.
pub(crate) fn map_of(entries: Vec<(Value, Value)>) -> Value {
    let mut seen = std::collections::HashSet::new();
    let unique = entries.into_iter().filter(|(key, _)| seen.insert(key.encode()));
    Value::Map(Map::from_entries(unique).unwrap())
}

pub(crate) fn any_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<u64>().prop_map(Value::Unsigned),
        any::<u64>().prop_map(Value::Negative),
        boundary_u64().prop_map(Value::Unsigned),
        boundary_u64().prop_map(Value::Negative),
        (0_u64..=30).prop_map(Value::Unsigned),
        prop::collection::vec(any::<u8>(), 0..40).prop_map(Value::Bytes),
        prop::collection::vec(any::<u8>(), 20..30).prop_map(Value::Bytes),
        ".{0,20}".prop_map(Value::Text),
        any::<bool>().prop_map(Value::Bool),
        Just(Value::Null),
    ];
    leaf.prop_recursive(4, 64, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
            prop::collection::vec((inner.clone(), inner), 0..8).prop_map(map_of),
        ]
    })
}

/// A small change to an encoding: the edge of validity, where decoders go wrong.
#[derive(Clone, Debug)]
pub(crate) enum Mutation {
    FlipBit { at: usize, bit: u8 },
    Insert { at: usize, byte: u8 },
    Delete { at: usize },
    Replace { at: usize, byte: u8 },
    Truncate { at: usize },
}

pub(crate) fn any_mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (any::<usize>(), 0_u8..8).prop_map(|(at, bit)| Mutation::FlipBit { at, bit }),
        (any::<usize>(), any::<u8>()).prop_map(|(at, byte)| Mutation::Insert { at, byte }),
        any::<usize>().prop_map(|at| Mutation::Delete { at }),
        (any::<usize>(), any::<u8>()).prop_map(|(at, byte)| Mutation::Replace { at, byte }),
        any::<usize>().prop_map(|at| Mutation::Truncate { at }),
    ]
}

pub(crate) fn mutate(mut bytes: Vec<u8>, mutation: &Mutation) -> Vec<u8> {
    let len = bytes.len();
    match *mutation {
        Mutation::FlipBit { at, bit } if len > 0 => bytes[at % len] ^= 1 << bit,
        Mutation::Insert { at, byte } => bytes.insert(at % (len + 1), byte),
        Mutation::Delete { at } if len > 0 => {
            bytes.remove(at % len);
        }
        Mutation::Replace { at, byte } if len > 0 => bytes[at % len] = byte,
        Mutation::Truncate { at } => bytes.truncate(at % (len + 1)),
        _ => {}
    }
    bytes
}

/// Sixteen random bytes with the version 7 and RFC 9562 variant bits set: a valid UUIDv7.
pub(crate) fn uuid_v7_bytes() -> impl Strategy<Value = [u8; 16]> {
    any::<[u8; 16]>().prop_map(|mut bytes| {
        bytes[6] = 0x70 | (bytes[6] & 0x0F);
        bytes[8] = 0x80 | (bytes[8] & 0x3F);
        bytes
    })
}

pub(crate) fn any_id<T>() -> impl Strategy<Value = Id<T>> {
    uuid_v7_bytes().prop_map(|bytes| Id::from_bytes(bytes).unwrap())
}

/// Identifiers of up to `max_len` characters, favouring short ones and the longest allowed.
fn identifier(max_len: usize) -> impl Strategy<Value = String> {
    let rest = max_len - 1;
    let pattern = |repeat: String| {
        proptest::string::string_regex(&format!("[a-z][a-z0-9_]{{{repeat}}}")).unwrap()
    };
    prop_oneof![3 => pattern(format!("0,{}", rest.min(12))), 1 => pattern(rest.to_string())]
}

pub(crate) fn any_stream_kind() -> impl Strategy<Value = StreamKind> {
    identifier(32).prop_map(|kind| StreamKind::new(&kind).unwrap())
}

pub(crate) fn any_component() -> impl Strategy<Value = Component> {
    identifier(32).prop_map(|name| Component::new(&name).unwrap())
}

/// Schema names: dotted paths, and names of exactly the maximum length, 64 characters.
pub(crate) fn any_schema_name() -> impl Strategy<Value = SchemaName> {
    prop_oneof![
        3 => prop::collection::vec("[a-z][a-z0-9_]{0,12}", 1..5).prop_map(|parts| parts.join(".")),
        1 => "[a-z][a-z0-9_]{63}",
        1 => "[a-z][a-z0-9_]{30}\\.[a-z][a-z0-9_]{31}",
    ]
    .prop_map(|name| SchemaName::new(&name).unwrap())
}

pub(crate) fn any_schema() -> impl Strategy<Value = SchemaRef> {
    let version = prop_oneof![1..=u32::MAX, Just(1), Just(u32::MAX)];
    (any_schema_name(), version)
        .prop_map(|(name, version)| SchemaRef { name, version: NonZeroU32::new(version).unwrap() })
}

pub(crate) fn any_origin_seq() -> impl Strategy<Value = NonZeroU64> {
    prop_oneof![1..=u64::MAX, 1_u64..100, Just(u64::MAX)]
        .prop_map(|seq| NonZeroU64::new(seq).unwrap())
}

pub(crate) fn any_hlc() -> impl Strategy<Value = Hlc> {
    prop_oneof![any::<u64>(), boundary_u64()].prop_map(Hlc::from_u64)
}

/// Dates across the whole range, including its ends and a leap day.
pub(crate) fn any_business_date() -> impl Strategy<Value = BusinessDate> {
    prop_oneof![
        8 => (1_i16..=9999, 1_i8..=12, 1_i8..=31),
        1 => prop::sample::select(vec![(1, 1, 1), (9999, 12, 31), (2024, 2, 29)]),
    ]
    .prop_filter_map("a real date", |(year, month, day)| BusinessDate::new(year, month, day).ok())
}

pub(crate) fn any_actor() -> impl Strategy<Value = Actor> {
    prop_oneof![
        any_id::<TeamMember>().prop_map(Actor::TeamMember),
        any_id::<Customer>().prop_map(Actor::Customer),
        any_id::<Integration>().prop_map(Actor::Integration),
        any_id::<Extension>().prop_map(Actor::Extension),
        any_component().prop_map(Actor::System),
    ]
}

pub(crate) fn any_prev_hash() -> impl Strategy<Value = EventHash> {
    prop_oneof![Just(EventHash::ZERO), any::<[u8; 32]>().prop_map(EventHash::from_bytes)]
}

pub(crate) fn any_body() -> impl Strategy<Value = EventBody> {
    let what = (
        any_id::<Event>(),
        any_id::<Location>(),
        (any_stream_kind(), any_id::<Aggregate>()),
        any_schema(),
    );
    let when = (any_id::<Device>(), any_origin_seq(), any_hlc(), any_business_date(), any_actor());
    let links = (
        prop::option::of(any_id::<Event>()),
        prop::option::of(any_id::<Cause>()),
        prop::option::of(any_id::<Correlation>()),
        any_value().prop_map(|value| Payload::new(&value).unwrap()),
        any_prev_hash(),
    );
    (what, when, links).prop_map(
        |(
            (event_id, location, (kind, id), schema),
            (origin_device, origin_seq, hlc, business_date, actor),
            (approval, causation, correlation, payload, prev_hash),
        )| EventBody {
            event_id,
            location,
            stream: StreamRef { kind, id },
            schema,
            origin_device,
            origin_seq,
            hlc,
            business_date,
            actor,
            approval,
            causation,
            correlation,
            payload,
            prev_hash,
        },
    )
}

pub(crate) fn any_draft() -> impl Strategy<Value = EventDraft> {
    let about =
        (any_stream_kind(), any_id::<Aggregate>(), any_schema(), any_business_date(), any_actor());
    let links = (
        prop::option::of(any_id::<Event>()),
        prop::option::of(any_id::<Cause>()),
        prop::option::of(any_id::<Correlation>()),
        any_value().prop_map(|value| Payload::new(&value).unwrap()),
    );
    (about, links).prop_map(
        |(
            (kind, id, schema, business_date, actor),
            (approval, causation, correlation, payload),
        )| {
            EventDraft {
                stream: StreamRef { kind, id },
                schema,
                business_date,
                actor,
                approval,
                causation,
                correlation,
                payload,
            }
        },
    )
}

/// `len` readings of a physical clock, starting in 2026: mostly moving forward a little, sometimes
/// standing still, jumping ahead a day, or going back up to a minute.
pub(crate) fn clock_readings(len: usize) -> impl Strategy<Value = Vec<Timestamp>> {
    let step = prop_oneof![
        6 => 0_i64..2_000,
        2 => Just(0_i64),
        1 => -60_000_i64..0,
        1 => 0_i64..86_400_000,
    ];
    prop::collection::vec(step, len).prop_map(|steps| {
        steps
            .into_iter()
            .scan(1_790_517_780_000_i64, |millis, step| {
                *millis += step;
                Some(Timestamp::from_millis(*millis).unwrap())
            })
            .collect()
    })
}
