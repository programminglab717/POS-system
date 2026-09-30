//! Known answers for sequencing records: how a record numbers events, each rule a record must
//! keep, reading payloads strictly, and the pinned payload.

use keel_events::cbor::{Map, Value};
use keel_events::envelope::{SchemaName, SchemaRef};

use super::*;
use crate::codec::PayloadError;
use crate::schema::{DecodeError, DomainEvent};

/// One past the largest number a record may hold.
const PAST_MAX: u64 = MAX_NUMBER + 1;

fn device(n: u64) -> Id<Device> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

/// A hash whose bytes step up from `first`: all different, so a byte lost or moved shows.
fn hash(first: u8) -> EventHash {
    EventHash::from_bytes(core::array::from_fn(|k| first.wrapping_add(u8::try_from(k).unwrap())))
}

/// Device `n`'s events from `from` to `to`, the last hashing to bytes stepping from `from`'s
/// low byte.
fn run(n: u64, from: u64, to: u64) -> Run {
    Run { device: device(n), from, to, last: hash(from.to_le_bytes()[0]) }
}

fn invalid(rule: &'static str) -> Result<Assigned, PayloadError> {
    Err(PayloadError::Invalid(rule))
}

/// Decodes the payload of a record built without its rules being checked.
fn decode(epoch: u64, first: u64, runs: Vec<Run>) -> Result<SequenceEvent, DecodeError> {
    let unchecked = SequenceEvent::Assigned(Assigned { epoch, first, runs });
    SequenceEvent::from_value(&ASSIGNED.to_ref().unwrap(), &unchecked.to_value())
}

#[test]
fn a_record_numbers_its_runs_in_order() {
    let record = Assigned::new(1, 5, vec![run(1, 1, 3), run(2, 1, 1), run(1, 4, 5)]).unwrap();
    assert_eq!(record.count(), 6);
    assert_eq!(record.last(), 10);
    let numbered: Vec<(u64, u64, u64)> =
        record.numbered().map(|(number, run)| (number, run.from, run.to)).collect();
    assert_eq!(numbered, [(5, 1, 3), (8, 1, 1), (9, 4, 5)]);
    for (device_n, position, number) in [(1, 1, 5), (1, 3, 7), (2, 1, 8), (1, 4, 9), (1, 5, 10)] {
        assert_eq!(record.number_of(device(device_n), position), Some(number));
    }
    // What the record doesn't cover.
    for (device_n, position) in [(1, 0), (1, 6), (2, 2), (3, 1)] {
        assert_eq!(record.number_of(device(device_n), position), None);
    }
    let one = run(2, 7, 7);
    assert_eq!(one.count(), 1);
    assert!(one.covers(device(2), 7));
    assert!(!one.covers(device(2), 6) && !one.covers(device(2), 8) && !one.covers(device(1), 7));
}

#[test]
fn a_device_may_start_anywhere_but_its_runs_follow_on() {
    // A record covers each device's log from wherever the one before it left off.
    let record = Assigned::new(1, 1, vec![run(1, 40, 41), run(2, 7, 7), run(1, 42, 42)]).unwrap();
    assert_eq!(record.last(), 4);
    // A gap, an overlap and a step back between one device's runs.
    for later in [run(1, 43, 44), run(1, 41, 43), run(1, 1, 2)] {
        let runs = vec![run(1, 40, 41), run(2, 7, 7), later];
        assert_eq!(Assigned::new(1, 1, runs), invalid("runs that don't follow on"));
    }
    // One device's runs side by side are one run, and must be written as one.
    let runs = vec![run(1, 1, 2), run(1, 3, 4)];
    assert_eq!(Assigned::new(1, 1, runs), invalid("runs of one device side by side"));
    let runs = vec![run(2, 1, 1), run(1, 1, 2), run(1, 3, 4)];
    assert_eq!(Assigned::new(1, 1, runs), invalid("runs of one device side by side"));
}

#[test]
fn every_number_is_from_1_to_the_largest_the_store_holds() {
    for epoch in [0, PAST_MAX, u64::MAX] {
        assert_eq!(Assigned::new(epoch, 1, vec![run(1, 1, 1)]), invalid("epoch"));
    }
    for first in [0, PAST_MAX, u64::MAX] {
        assert_eq!(Assigned::new(1, first, vec![run(1, 1, 1)]), invalid("first"));
    }
    for (from, to) in [(0, 1), (0, 0), (1, PAST_MAX), (PAST_MAX, PAST_MAX), (3, 2)] {
        assert_eq!(Assigned::new(1, 1, vec![run(1, from, to)]), invalid("run"));
    }
    let largest = Assigned::new(MAX_NUMBER, MAX_NUMBER, vec![run(3, MAX_NUMBER, MAX_NUMBER)]);
    assert_eq!(largest.map(|record| record.last()), Ok(MAX_NUMBER));
    let whole = Assigned::new(1, 1, vec![run(1, 1, MAX_NUMBER)]).unwrap();
    assert_eq!((whole.count(), whole.last()), (MAX_NUMBER, MAX_NUMBER));
}

#[test]
fn the_numbers_a_record_assigns_never_pass_the_largest() {
    const SECOND_LARGEST: u64 = MAX_NUMBER - 1;
    assert_eq!(Assigned::new(1, SECOND_LARGEST, vec![run(1, 1, 2)]).unwrap().last(), MAX_NUMBER);
    assert_eq!(Assigned::new(1, MAX_NUMBER, vec![run(1, 1, 2)]), invalid("last number"));
    let runs = vec![run(1, 1, MAX_NUMBER), run(2, 1, 1)];
    assert_eq!(Assigned::new(1, 1, runs), invalid("last number"));
    // So many events that counting them overflows.
    let runs = vec![run(1, 1, MAX_NUMBER), run(2, 1, MAX_NUMBER), run(3, 1, MAX_NUMBER)];
    assert_eq!(Assigned::new(1, 1, runs), invalid("last number"));
}

#[test]
fn a_record_holds_from_one_run_to_the_most() {
    const TOO_MANY: usize = MAX_RUNS + 1;
    assert_eq!(Assigned::new(1, 1, Vec::new()), invalid("runs"));
    // Two devices taking turns, one event each.
    let turns = |count: usize| -> Vec<Run> {
        (1_u64..).flat_map(|at| [run(1, at, at), run(2, at, at)]).take(count).collect()
    };
    let record = Assigned::new(1, 1, turns(MAX_RUNS)).unwrap();
    assert_eq!(record.count(), u64::try_from(MAX_RUNS).unwrap());
    assert_eq!(Assigned::new(1, 1, turns(TOO_MANY)), invalid("runs"));
}

#[test]
fn decoding_keeps_the_same_rules() {
    let refused = |epoch, first, runs, rule| {
        assert_eq!(
            decode(epoch, first, runs),
            Err(DecodeError::Malformed(PayloadError::Invalid(rule))),
            "{rule}"
        );
    };
    refused(0, 1, vec![run(1, 1, 1)], "epoch");
    refused(1, PAST_MAX, vec![run(1, 1, 1)], "first");
    refused(1, 1, Vec::new(), "runs");
    refused(1, 1, vec![run(1, 2, 1)], "run");
    refused(1, 1, vec![run(1, 0, 1)], "run");
    refused(1, 1, vec![run(1, 1, 2), run(1, 3, 3)], "runs of one device side by side");
    refused(1, 1, vec![run(1, 1, 2), run(2, 1, 1), run(1, 4, 4)], "runs that don't follow on");
    refused(1, MAX_NUMBER, vec![run(1, 1, 2)], "last number");
    let record = Assigned::new(1, 1, vec![run(1, 1, 2), run(2, 5, 5)]).unwrap();
    assert_eq!(decode(1, 1, record.runs.clone()), Ok(SequenceEvent::Assigned(record)));
}

#[test]
fn payloads_are_read_strictly() {
    let payload = SequenceEvent::Assigned(Assigned::new(2, 9, vec![run(1, 1, 1)]).unwrap())
        .to_value()
        .as_map()
        .unwrap()
        .clone()
        .into_entries();
    let schema = ASSIGNED.to_ref().unwrap();
    let with = |key: u64, value: Option<Value>| {
        let mut entries = payload.clone();
        entries.retain(|(existing, _)| existing.as_u64() != Some(key));
        entries.extend(value.map(|value| (Value::Unsigned(key), value)));
        SequenceEvent::from_value(&schema, &Value::Map(Map::from_entries(entries).unwrap()))
    };
    let malformed = |error| Err(DecodeError::Malformed(error));
    assert_eq!(
        with(4, Some(Value::Unsigned(1))),
        malformed(PayloadError::UnknownField("4".into()))
    );
    assert_eq!(with(1, None), malformed(PayloadError::Missing("epoch")));
    assert_eq!(with(2, None), malformed(PayloadError::Missing("first")));
    assert_eq!(with(3, None), malformed(PayloadError::Missing("runs")));
    assert_eq!(with(1, Some(Value::from("1"))), malformed(PayloadError::Invalid("epoch")));
    assert_eq!(with(2, Some(Value::integer(-1))), malformed(PayloadError::Invalid("first")));
    let good = run(1, 1, 1).to_value().as_array().unwrap().to_vec();
    let bad_runs = [
        Value::Map(Map::new()),
        Value::Array(vec![Value::Array(good[..3].to_vec())]),
        Value::Array(vec![Value::Array([&good[..], &[Value::Null]].concat())]),
        // A hash of 31 bytes, and a device identifier that isn't a UUIDv7.
        Value::Array(vec![Value::Array(vec![
            good[0].clone(),
            good[1].clone(),
            good[2].clone(),
            Value::Bytes(vec![0; 31]),
        ])]),
        Value::Array(vec![Value::Array(vec![
            Value::Bytes(vec![0; 16]),
            good[1].clone(),
            good[2].clone(),
            good[3].clone(),
        ])]),
        // A position below zero.
        Value::Array(vec![Value::Array(vec![
            good[0].clone(),
            Value::integer(-1),
            good[2].clone(),
            good[3].clone(),
        ])]),
    ];
    for runs in bad_runs {
        assert_eq!(
            with(3, Some(runs.clone())),
            malformed(PayloadError::Invalid("runs")),
            "{runs:?}"
        );
    }
    assert_eq!(
        SequenceEvent::from_value(&schema, &Value::Array(Vec::new())),
        malformed(PayloadError::NotAMap)
    );
}

#[test]
fn the_registry_lists_the_schema_once() {
    assert_eq!(SequenceEvent::SCHEMAS, &[ASSIGNED]);
    let reference = ASSIGNED.to_ref().unwrap();
    assert_eq!(reference.name.as_str(), "sequence.assigned");
    assert!(ASSIGNED.matches(&reference));
    let payload =
        SequenceEvent::Assigned(Assigned::new(1, 1, vec![run(1, 1, 1)]).unwrap()).to_value();
    let unknown = [("sequence.revoked", 1_u32), ("sequence.assigned", 2), ("order.created", 1)];
    for (name, version) in unknown {
        let schema = SchemaRef {
            name: SchemaName::new(name).unwrap(),
            version: version.try_into().unwrap(),
        };
        assert_eq!(SequenceEvent::from_value(&schema, &payload), Err(DecodeError::UnknownSchema));
    }
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The sequencing record's payload, pinned forever: an ordinary record, and one at the largest
/// numbers. Python's `cbor2` encoded each one itself from the documented key table, and
/// confirmed it is canonical. If this test fails, the payload format changed, and stored records
/// would no longer decode.
#[test]
fn the_payload_format_is_pinned() {
    let pinned = [
        (
            Assigned {
                epoch: 1,
                first: 1,
                runs: vec![
                    Run { device: device(1), from: 1, to: 3, last: hash(0xA3) },
                    Run { device: device(2), from: 1, to: 1, last: hash(0xB1) },
                    Run { device: device(1), from: 4, to: 5, last: hash(0xA5) },
                ],
            },
            "a301010201038384500192f0c100007000800000000000000101035820a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c284500192f0c100007000800000000000000201015820b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd084500192f0c100007000800000000000000104055820a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4",
        ),
        (
            Assigned {
                epoch: MAX_NUMBER,
                first: MAX_NUMBER,
                runs: vec![Run {
                    device: device(3),
                    from: MAX_NUMBER,
                    to: MAX_NUMBER,
                    last: hash(0xE0),
                }],
            },
            "a3011b7fffffffffffffff021b7fffffffffffffff038184500192f0c10000700080000000000000031b7fffffffffffffff1b7fffffffffffffff5820e0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff",
        ),
    ];
    for (record, hex) in pinned {
        let event = SequenceEvent::Assigned(record);
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), "sequence.assigned");
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice());
        assert_eq!(SequenceEvent::decode(&schema, &payload), Ok(event));
    }
}
