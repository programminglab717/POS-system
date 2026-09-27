//! Property tests: timestamps round-trip and sort, HLCs and IDs only ever increase.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

use core::time::Duration;

use keel_types::{Hlc, HlcClock, HlcError, Id, IdGenerator, SeededEntropy, Timestamp};
use proptest::prelude::*;

const DRIFT_MS: u64 = 60_000;

fn any_timestamp() -> impl Strategy<Value = Timestamp> {
    (Timestamp::MIN.as_micros()..=Timestamp::MAX.as_micros())
        .prop_map(|micros| Timestamp::from_micros(micros).unwrap())
}

/// Physical clock readings around 2026 that wander, repeat and jump backwards, as device
/// clocks do, including before 1970.
fn clock_readings() -> impl Strategy<Value = Vec<i64>> {
    let step = prop_oneof![
        3 => 0_i64..=5,                       // the same or the next few milliseconds
        2 => -5_i64..=0,                      // small corrections backwards
        1 => -3_600_000_i64..=3_600_000,      // jumps of up to an hour either way
        1 => Just(-1_790_000_000_000_i64),    // a reset to 1970 (and before)
    ];
    (1_790_000_000_000_i64..=1_790_000_100_000, prop::collection::vec(step, 1..60)).prop_map(
        |(start, steps)| {
            steps
                .into_iter()
                .scan(start, |now, step| {
                    *now = (*now + step).max(-10_000);
                    Some(*now)
                })
                .collect()
        },
    )
}

/// One step for an HLC clock: a local event, or a message from another device whose wall time
/// is offset from ours (sometimes beyond the drift limit), with any counter value.
#[derive(Clone, Debug)]
enum Step {
    Tick,
    Observe { remote_offset_ms: i64, remote_logical: u16 },
}

fn hlc_steps(len: usize) -> impl Strategy<Value = Vec<Step>> {
    let step = prop_oneof![
        Just(Step::Tick),
        (-120_000_i64..=120_000, any::<u16>()).prop_map(|(remote_offset_ms, remote_logical)| {
            Step::Observe { remote_offset_ms, remote_logical }
        }),
        (Just(0_i64), prop_oneof![Just(u16::MAX), Just(u16::MAX - 1)]).prop_map(
            |(remote_offset_ms, remote_logical)| Step::Observe { remote_offset_ms, remote_logical }
        ),
    ];
    prop::collection::vec(step, len)
}

fn physical(millis: i64) -> u64 {
    u64::try_from(millis).unwrap_or(0).min(Hlc::MAX_WALL_MS)
}

proptest! {
    /// Display and parse round-trip, and the text sorts exactly like the timestamps.
    #[test]
    fn timestamps_round_trip_and_sort_as_text(a in any_timestamp(), b in any_timestamp()) {
        let (text_a, text_b) = (a.to_string(), b.to_string());
        prop_assert_eq!(text_a.parse::<Timestamp>(), Ok(a));
        prop_assert_eq!(text_a.len(), 27);
        prop_assert_eq!(text_a.cmp(&text_b), a.cmp(&b));
        let millis = a.as_millis();
        prop_assert!(millis * 1_000 <= a.as_micros() && a.as_micros() < (millis + 1) * 1_000);
    }

    /// Adding then subtracting a duration returns the same timestamp.
    #[test]
    fn duration_arithmetic_is_reversible(start in any_timestamp(), micros in 0_u64..=1_000_000_000_000) {
        let duration = Duration::from_micros(micros);
        if let Some(later) = start.checked_add(duration) {
            prop_assert_eq!(later.checked_sub(duration), Some(start));
            prop_assert_eq!(later.duration_since(start), Some(duration));
        }
    }

    /// Whatever the clocks do, an HLC clock's values strictly increase, never fall behind
    /// physical time, move past every remote HLC they accept, never run ahead of the latest
    /// time seen except by a full counter, and rejections change nothing.
    #[test]
    fn hlc_invariants_hold_for_any_history(
        readings in clock_readings(),
        steps in hlc_steps(60),
    ) {
        let mut clock = HlcClock::new(Duration::from_millis(DRIFT_MS));
        let mut latest_seen = 0_u64;
        for (&now_ms, step) in readings.iter().zip(&steps) {
            let now = Timestamp::from_millis(now_ms).unwrap();
            let before = clock.last();
            let pt = physical(now_ms);
            let (result, remote) = match *step {
                Step::Tick => (clock.tick(now), None),
                Step::Observe { remote_offset_ms, remote_logical } => {
                    let remote_wall = physical(now_ms + remote_offset_ms);
                    let remote = Hlc::new(remote_wall, remote_logical).unwrap();
                    (clock.observe(remote, now), Some(remote))
                }
            };
            match result {
                Ok(next) => {
                    prop_assert!(next > before, "{:?} after {:?}", next, before);
                    prop_assert_eq!(clock.last(), next);
                    prop_assert!(next.wall_ms() >= pt);
                    if let Some(remote) = remote {
                        prop_assert!(next > remote);
                        prop_assert!(remote.wall_ms() <= pt + DRIFT_MS);
                        latest_seen = latest_seen.max(remote.wall_ms());
                    }
                    latest_seen = latest_seen.max(pt);
                    if next.wall_ms() > latest_seen {
                        // Only a full counter moves past the latest time seen, by one ms.
                        prop_assert_eq!(next.wall_ms(), latest_seen + 1);
                        prop_assert_eq!(next.logical(), 0);
                        latest_seen = next.wall_ms();
                    }
                }
                Err(HlcError::ClockDrift { remote: rejected, ahead_ms, max_forward_drift_ms }) => {
                    prop_assert_eq!(Some(rejected), remote);
                    prop_assert!(rejected.wall_ms() > pt + DRIFT_MS);
                    prop_assert_eq!(ahead_ms, rejected.wall_ms() - pt);
                    prop_assert_eq!(max_forward_drift_ms, DRIFT_MS);
                    prop_assert_eq!(clock.last(), before, "a rejection changes nothing");
                }
                Err(error) => prop_assert!(false, "unexpected {:?}", error),
            }
        }
    }

    /// IDs from one generator strictly increase and never predate the clock reading, whatever
    /// the clock does; they round-trip through text and bytes.
    #[test]
    fn ids_strictly_increase_for_any_history(readings in clock_readings(), seed in any::<u64>()) {
        struct Sale;
        let mut generator = IdGenerator::new(SeededEntropy::new(seed));
        let mut previous: Option<Id<Sale>> = None;
        for &now_ms in &readings {
            let id: Id<Sale> = generator.generate(Timestamp::from_millis(now_ms).unwrap()).unwrap();
            prop_assert_eq!(id.as_uuid().get_version_num(), 7);
            prop_assert!(id.timestamp_ms() >= physical(now_ms));
            if let Some(previous) = previous {
                prop_assert!(id > previous);
                prop_assert!(id.timestamp_ms() >= previous.timestamp_ms());
            }
            prop_assert_eq!(id.to_string().parse::<Id<Sale>>(), Ok(id));
            prop_assert_eq!(Id::<Sale>::from_bytes(id.to_bytes()), Ok(id));
            previous = Some(id);
        }
    }
}
