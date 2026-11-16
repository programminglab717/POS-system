//! Property tests for stock (ADR-0036), against an independent model:
//! - the levels, from any movements, counts and flows, are what a replay of them in time order
//!   gives: each count, in the order of its as-of time, setting what is on hand, and everything
//!   else adding to it, an item's stock starting at its first event when it has no count;
//! - the fold gives the same entries, and the same levels, whatever order its events arrive in.

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

fn any_recorded() -> impl Strategy<Value = Recorded> {
    let movement = prop::sample::select(Movement::ALL.to_vec());
    prop_oneof![
        3 => (0_i64..30, movement, 1_i64..20, any::<bool>()).prop_map(|(minute, movement, n, down)| {
            let n = if movement == Movement::Adjusted && down { -n } else { n };
            let quantity = Quantity::from_whole(n, Unit::Each).unwrap();
            Recorded { minute, event: StockEvent::Moved(StockMoved { movement, quantity, reason: None }) }
        }),
        // A count as of a time at or before it was recorded, ties included.
        1 => (0_i64..30, 0_i64..5, 0_i64..20).prop_map(|(minute, back, n)| Recorded {
            minute,
            event: StockEvent::Counted(StockCounted {
                quantity: Quantity::from_whole(n, Unit::Each).unwrap(),
                as_of: at(minute - back),
                note: None,
            }),
        }),
    ]
}

fn any_flow() -> impl Strategy<Value = Flow> {
    (-5_i64..35, -4_i64..=4).prop_filter("a flow moves something", |(_, n)| *n != 0).prop_map(
        |(minute, n)| Flow { at: at(minute), delta: Quantity::from_whole(n, Unit::Each).unwrap() },
    )
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

/// The model: what is on hand, reserved and available, by replaying everything in time order.
/// A count is placed at its as-of time, after what moved at that time, and the latest of several
/// at one time, by when it was recorded, last; it sets on hand. With no count, what flowed
/// before the first event doesn't count.
fn model(recorded: &[(usize, Recorded)], flows: &[Flow]) -> (i64, i64, i64) {
    #[derive(Clone, Copy)]
    enum Step {
        Add(i64),
        Set(i64),
    }
    let whole = |quantity: Quantity| quantity.micros() / 1_000_000;
    // (time, order within a time, step): moves and flows first at a time, then counts.
    let mut timeline: Vec<(i64, (u8, i64, usize), Step)> = Vec::new();
    let mut reserved = 0;
    let first = recorded.iter().map(|(_, each)| each.minute).min();
    let counts = recorded.iter().any(|(_, each)| matches!(each.event, StockEvent::Counted(_)));
    for (n, each) in recorded {
        let time = at(each.minute).as_micros();
        match &each.event {
            StockEvent::Moved(moved) => {
                let q = whole(moved.quantity);
                match moved.movement {
                    Movement::Received | Movement::Adjusted => {
                        timeline.push((time, (0, 0, *n), Step::Add(q)));
                    }
                    Movement::Wasted => timeline.push((time, (0, 0, *n), Step::Add(-q))),
                    Movement::Reserved => reserved += q,
                    Movement::Released => reserved -= q,
                }
            }
            StockEvent::Counted(counted) => timeline.push((
                counted.as_of.as_micros(),
                (1, time, *n),
                Step::Set(whole(counted.quantity)),
            )),
        }
    }
    for flow in flows {
        let started = counts || first.is_some_and(|first| flow.at >= at(first));
        if started && first.is_some() {
            timeline.push((flow.at.as_micros(), (0, 0, usize::MAX), Step::Add(whole(flow.delta))));
        }
    }
    timeline.sort_by_key(|(time, order, _)| (*time, *order));
    let mut on_hand = 0;
    for (_, _, step) in timeline {
        match step {
            Step::Add(q) => on_hand += q,
            Step::Set(q) => on_hand = q,
        }
    }
    (on_hand, reserved, on_hand - reserved)
}

proptest! {
    /// The levels are the model's.
    #[test]
    fn the_levels_are_a_replay_in_time_order(
        recorded in prop::collection::vec(any_recorded(), 0..14),
        flows in prop::collection::vec(any_flow(), 0..10),
    ) {
        let indexed: Vec<(usize, Recorded)> = recorded.into_iter().enumerate().collect();
        let stock = fold(&indexed);
        let got = levels(Unit::Each, stock.entries(), &flows).unwrap();
        let whole = |quantity: Quantity| quantity.micros() / 1_000_000;
        prop_assert_eq!(
            (whole(got.on_hand), whole(got.reserved), whole(got.available)),
            model(&indexed, &flows)
        );
    }

    /// The order the events arrive in changes nothing.
    #[test]
    fn the_fold_is_the_same_in_any_order(
        recorded in prop::collection::vec(any_recorded(), 0..14),
        flows in prop::collection::vec(any_flow(), 0..6),
        shuffle in any::<prop::sample::Index>(),
    ) {
        let indexed: Vec<(usize, Recorded)> = recorded.into_iter().enumerate().collect();
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
        prop_assert_eq!(
            levels(Unit::Each, a.entries(), &flows).unwrap(),
            levels(Unit::Each, b.entries(), &flows).unwrap()
        );
        let negative_count = a.entries().iter().any(|entry| {
            matches!(entry.kind, EntryKind::Counted { quantity, .. } if quantity.is_negative())
        });
        prop_assert!(!negative_count);
    }
}
