//! Property tests for stock (ADR-0036), against an independent model:
//! - the levels, from any movements, counts and flows, are what an exact replay of them in time
//!   order gives: each count, in the order of its as-of time, setting what is on hand, and
//!   everything else adding to it, an item's stock starting at its first event when it has no
//!   count, and everything in another unit than its first event's left out; they fail only when
//!   a sum could overflow;
//! - the fold gives the same entries, and the same levels, whatever order its events arrive in,
//!   in one unit.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_domain::aggregate::{Aggregate, EventMeta};
use keel_domain::stock::{
    EntryKind, Flow, Movement, Stock, StockCounted, StockEvent, StockMoved, levels,
};
use keel_events::envelope::{Actor, Device, Location};
use keel_types::{Hlc, Id, Quantity, Timestamp, Unit};
use num_bigint::BigInt;
use proptest::prelude::*;
use support::id;

fn location() -> Id<Location> {
    id(0x100)
}

/// A time, `minute` after 8:00 on 13 November 2026.
fn at(minute: i64) -> Timestamp {
    Timestamp::from_micros(1_794_560_400_000_000 + minute * 60_000_000).unwrap()
}

/// A stock event, recorded at a minute.
#[derive(Clone, Debug)]
struct Recorded {
    minute: i64,
    event: StockEvent,
}

/// A quantity: mostly a few of each, sometimes in another unit, sometimes near what an `i64` of
/// millionths holds, so sums overflow.
fn any_quantity(min: i64) -> impl Strategy<Value = Quantity> {
    let unit = prop_oneof![7 => Just(Unit::Each), 1 => Just(Unit::Kilogram)];
    let micros = prop_oneof![
        9 => (min..20).prop_map(|n| n * 1_000_000),
        1 => i64::MAX / 4..=i64::MAX / 2,
    ];
    (micros, unit).prop_map(|(micros, unit)| Quantity::from_micros(micros, unit))
}

fn any_recorded() -> impl Strategy<Value = Recorded> {
    let movement = prop::sample::select(Movement::ALL.to_vec());
    prop_oneof![
        3 => (0_i64..30, movement, any_quantity(1), any::<bool>()).prop_map(|(minute, movement, quantity, down)| {
            let quantity = if movement == Movement::Adjusted && down {
                Quantity::from_micros(-quantity.micros(), quantity.unit())
            } else {
                quantity
            };
            Recorded { minute, event: StockEvent::Moved(StockMoved { movement, quantity, reason: None }) }
        }),
        // A count as of a time at or before it was recorded, ties included.
        1 => (0_i64..30, 0_i64..5, any_quantity(0)).prop_map(|(minute, back, quantity)| Recorded {
            minute,
            event: StockEvent::Counted(StockCounted { quantity, as_of: at(minute - back), note: None }),
        }),
    ]
}

fn any_flow() -> impl Strategy<Value = Flow> {
    (-5_i64..35, any_quantity(1), any::<bool>()).prop_map(|(minute, quantity, sold)| {
        let delta = if sold {
            Quantity::from_micros(-quantity.micros(), quantity.unit())
        } else {
            quantity
        };
        Flow { at: at(minute), delta }
    })
}

/// Folds `recorded`, in the order given, each with its own event id by its index in `ids`.
fn fold(recorded: &[(usize, Recorded)]) -> Stock {
    let mut stock = Stock::new(id(0x401));
    for (n, each) in recorded {
        let n = u64::try_from(*n).unwrap();
        let wall = u64::try_from(at(each.minute).as_micros() / 1_000).unwrap();
        let meta = EventMeta {
            event_id: id(0xE000 + n),
            location: location(),
            origin_device: id::<Device>(0xD1),
            origin_seq: (n + 1).try_into().unwrap(),
            hlc: Hlc::new(wall, u16::try_from(n).unwrap()).unwrap(),
            business_date: "2026-11-13".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        };
        stock.apply(&meta, &each.event);
    }
    stock
}

/// `recorded`, its quantity in `each`.
fn in_each(mut recorded: Recorded) -> Recorded {
    match &mut recorded.event {
        StockEvent::Moved(moved) => {
            moved.quantity = Quantity::from_micros(moved.quantity.micros(), Unit::Each);
        }
        StockEvent::Counted(counted) => {
            counted.quantity = Quantity::from_micros(counted.quantity.micros(), Unit::Each);
        }
    }
    recorded
}

fn quantity_of(event: &StockEvent) -> Quantity {
    match event {
        StockEvent::Moved(moved) => moved.quantity,
        StockEvent::Counted(counted) => counted.quantity,
    }
}

/// The model, in exact integers: what is on hand, reserved and available, in millionths of the
/// stock's unit, by replaying everything in time order. The unit is the first event's, and
/// everything in another unit is left out. A count is placed at its as-of time, after what moved
/// at that time, and the latest of several at one time, by when it was recorded, last; it sets
/// on hand. With no count, what flowed before the first event doesn't count. Also: the sum of
/// every quantity's size it read, which bounds every partial sum.
fn model(recorded: &[(usize, Recorded)], flows: &[Flow]) -> ((BigInt, BigInt, BigInt), BigInt) {
    #[derive(Clone)]
    enum Step {
        Add(BigInt),
        Set(BigInt),
    }
    let unit = recorded.first().map_or(Unit::Each, |(_, each)| quantity_of(&each.event).unit());
    let recorded: Vec<&(usize, Recorded)> =
        recorded.iter().filter(|(_, each)| quantity_of(&each.event).unit() == unit).collect();
    let flows: Vec<&Flow> = flows.iter().filter(|flow| flow.delta.unit() == unit).collect();
    let big = |quantity: Quantity| BigInt::from(quantity.micros());
    let mut size = BigInt::from(0);
    // (time, order within a time, step): moves and flows first at a time, then counts.
    let mut timeline: Vec<(i64, (u8, i64, usize), Step)> = Vec::new();
    let mut reserved = BigInt::from(0);
    let first = recorded.iter().map(|(_, each)| each.minute).min();
    let counts = recorded.iter().any(|(_, each)| matches!(each.event, StockEvent::Counted(_)));
    for (n, each) in &recorded {
        let time = at(each.minute).as_micros();
        let q = big(quantity_of(&each.event));
        size += BigInt::from(q.magnitude().clone());
        match &each.event {
            StockEvent::Moved(moved) => match moved.movement {
                Movement::Received | Movement::Adjusted => {
                    timeline.push((time, (0, 0, *n), Step::Add(q)));
                }
                Movement::Wasted => timeline.push((time, (0, 0, *n), Step::Add(-q))),
                Movement::Reserved => reserved += q,
                Movement::Released => reserved -= q,
            },
            StockEvent::Counted(counted) => {
                timeline.push((counted.as_of.as_micros(), (1, time, *n), Step::Set(q)));
            }
        }
    }
    for flow in &flows {
        let started = counts || first.is_some_and(|first| flow.at >= at(first));
        if started && first.is_some() {
            size += BigInt::from(big(flow.delta).magnitude().clone());
            timeline.push((flow.at.as_micros(), (0, 0, usize::MAX), Step::Add(big(flow.delta))));
        }
    }
    timeline.sort_by_key(|(time, order, _)| (*time, *order));
    let mut on_hand = BigInt::from(0);
    for (_, _, step) in timeline {
        match step {
            Step::Add(q) => on_hand += q,
            Step::Set(q) => on_hand = q,
        }
    }
    let available = &on_hand - &reserved;
    ((on_hand, reserved, available), size)
}

proptest! {
    /// The levels are the model's, exactly; they fail only if a sum could overflow, and never
    /// when every quantity read together fits.
    #[test]
    fn the_levels_are_a_replay_in_time_order(
        recorded in prop::collection::vec(any_recorded(), 0..14),
        flows in prop::collection::vec(any_flow(), 0..10),
    ) {
        let indexed: Vec<(usize, Recorded)> = recorded.into_iter().enumerate().collect();
        let stock = fold(&indexed);
        let unit = stock.unit().unwrap_or(Unit::Each);
        let (expected, size) = model(&indexed, &flows);
        match levels(unit, stock.entries(), &flows) {
            Ok(got) => {
                let big = |quantity: Quantity| BigInt::from(quantity.micros());
                prop_assert_eq!((big(got.on_hand), big(got.reserved), big(got.available)), expected);
            }
            Err(_) => prop_assert!(size > BigInt::from(i64::MAX), "failed though nothing could overflow"),
        }
    }

    /// The order the events arrive in changes nothing.
    #[test]
    fn the_fold_is_the_same_in_any_order(
        recorded in prop::collection::vec(any_recorded(), 0..14),
        flows in prop::collection::vec(any_flow(), 0..6),
        shuffle in any::<prop::sample::Index>(),
    ) {
        // In one unit: with several, the first event's decides, and the store folds in the log's
        // order, the same on every replica, so which is first is fixed.
        let indexed: Vec<(usize, Recorded)> = recorded.into_iter().map(in_each).enumerate().collect();
        let mut shuffled = indexed.clone();
        if !shuffled.is_empty() {
            let len = shuffled.len();
            shuffled.rotate_left(shuffle.index(len));
            shuffled.reverse();
        }
        let a = fold(&indexed);
        let b = fold(&shuffled);
        let key = |stock: &Stock| {
            let mut entries: Vec<_> = stock.entries().iter().map(|entry| (entry.event.to_bytes(), format!("{:?}", entry.kind))).collect();
            entries.sort();
            entries
        };
        prop_assert_eq!(key(&a), key(&b));
        // Where a sum overflows can depend on the order it adds in; where neither does, they agree.
        if let (Ok(first), Ok(second)) =
            (levels(Unit::Each, a.entries(), &flows), levels(Unit::Each, b.entries(), &flows))
        {
            prop_assert_eq!(first, second);
        }
        let negative_count = a.entries().iter().any(|entry| {
            matches!(entry.kind, EntryKind::Counted { quantity, .. } if quantity.is_negative())
        });
        prop_assert!(!negative_count);
    }
}
