//! Tests for the aggregate framework's canonical order of provisional events: by hybrid logical
//! clock, then by device, then by position in the device's log.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::cmp::Ordering;

use keel_domain::aggregate::provisional_order;
use keel_events::cbor::{Map, Value};
use keel_events::envelope::{
    Actor, Device, EventBody, Payload, SchemaName, SchemaRef, StreamKind, StreamRef,
};
use keel_events::hash::EventHash;
use keel_types::{Hlc, Id};
use proptest::prelude::*;
use support::id;

/// Devices whose identifiers differ early or late in their bytes, so that comparing them by
/// their last digits alone would get some pairs wrong.
fn device(n: usize) -> Id<Device> {
    let uuids = [
        "0192f0c1-0000-7000-8000-000000000001",
        "0192f0c1-0000-7000-8000-000000000002",
        "0192f0c1-0000-7000-8000-000000000100",
        "0192f0c2-0000-7000-8000-000000000001",
    ];
    Id::parse(uuids[n % uuids.len()]).unwrap()
}

/// An event body with the given device, position and clock; everything else is fixed.
fn body(device: Id<Device>, seq: u64, hlc: Hlc) -> EventBody {
    EventBody {
        event_id: id(seq),
        location: id(0x100),
        stream: StreamRef { kind: StreamKind::new("order").unwrap(), id: id(0xA) },
        schema: SchemaRef {
            name: SchemaName::new("order.abandoned").unwrap(),
            version: 1.try_into().unwrap(),
        },
        origin_device: device,
        origin_seq: seq.try_into().unwrap(),
        hlc,
        business_date: "2026-09-28".parse().unwrap(),
        actor: Actor::TeamMember(id(0x300)),
        approval: None,
        causation: None,
        correlation: None,
        payload: Payload::new(&Value::Map(Map::new())).unwrap(),
        prev_hash: EventHash::ZERO,
    }
}

fn hlc(wall_ms: u64, logical: u16) -> Hlc {
    Hlc::new(wall_ms, logical).unwrap()
}

#[test]
fn provisional_events_order_by_clock_then_device_then_position() {
    let (first, second) = (device(0), device(3));
    // The clock comes first: its wall time, then its counter.
    let early = body(second, 9, hlc(1_000, 5));
    assert_eq!(provisional_order(&early, &body(first, 1, hlc(1_001, 0))), Ordering::Less);
    assert_eq!(provisional_order(&early, &body(first, 1, hlc(1_000, 6))), Ordering::Less);
    // At the same clock, the device, by its identifier's bytes.
    let tie = hlc(1_000, 0);
    assert_eq!(provisional_order(&body(first, 9, tie), &body(second, 1, tie)), Ordering::Less);
    assert_eq!(
        provisional_order(&body(device(2), 1, tie), &body(device(3), 1, tie)),
        Ordering::Less
    );
    // Then the position in the device's log.
    assert_eq!(provisional_order(&body(first, 2, tie), &body(first, 1, tie)), Ordering::Greater);
    assert_eq!(provisional_order(&body(first, 2, tie), &body(first, 2, tie)), Ordering::Equal);
}

/// A device, a position and a clock, from small ranges so that ties are common.
fn any_key() -> impl Strategy<Value = (usize, u64, u64, u16)> {
    (0_usize..4, 1_u64..4, 999_u64..1_002, 0_u16..3)
}

proptest! {
    /// The canonical order agrees with comparing the clock's wall time, then its counter, then
    /// the devices' identifiers byte by byte, then the positions.
    #[test]
    fn provisional_order_compares_clock_then_device_then_position(
        a in any_key(),
        b in any_key(),
    ) {
        let key = |(n, seq, wall_ms, logical): (usize, u64, u64, u16)| {
            (wall_ms, logical, device(n).to_bytes(), seq)
        };
        let body_of = |(n, seq, wall_ms, logical): (usize, u64, u64, u16)| {
            body(device(n), seq, hlc(wall_ms, logical))
        };
        prop_assert_eq!(provisional_order(&body_of(a), &body_of(b)), key(a).cmp(&key(b)));
    }
}
