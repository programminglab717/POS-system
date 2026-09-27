//! Property tests: in every time zone, with any cutoff, each instant belongs to exactly one
//! business date, and business dates never go backwards.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

use std::sync::OnceLock;

use keel_types::{BusinessDate, BusinessDayPolicy, Timestamp};
use proptest::prelude::*;

/// Every time zone in the bundled IANA database.
fn zone_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<String> =
            jiff::tz::db().available().map(|name| name.as_str().to_owned()).collect();
        names.sort();
        assert!(names.len() > 300, "the bundled database should hold every IANA zone");
        names
    })
}

/// Cutoffs: any minute of the day, weighted toward the small hours, where transitions and
/// real-world cutoffs cluster.
fn any_cutoff() -> impl Strategy<Value = (u8, u8)> {
    let hour = prop_oneof![3 => 0_u8..=5, 1 => 0_u8..=23];
    let minute = prop_oneof![3 => prop::sample::select(vec![0_u8, 15, 30, 45]), 1 => 0_u8..=59];
    (hour, minute)
}

/// An instant in a zone's history, usually within a day and a half of one of its transitions:
/// where business days have unusual lengths.
fn instant_near_transition(zone: &str, year: i16, pick: usize, offset_s: i64) -> Timestamp {
    let time_zone = jiff::tz::TimeZone::get(zone).unwrap();
    let start = jiff::civil::date(year, 1, 1).to_zoned(time_zone.clone()).unwrap().timestamp();
    let transitions: Vec<jiff::Timestamp> =
        time_zone.following(start).take(4).map(|transition| transition.timestamp()).collect();
    // Zones without transitions (UTC, fixed offsets) use the start of the year.
    let anchor = transitions.get(pick % transitions.len().max(1)).copied().unwrap_or(start);
    Timestamp::from_micros(anchor.as_microsecond() + offset_s * 1_000_000).unwrap()
}

proptest! {
    #[test]
    fn every_instant_belongs_to_exactly_one_business_date(
        zone_index in any::<usize>(),
        (cutoff_hour, cutoff_minute) in any_cutoff(),
        year in 1970_i16..=2037,
        pick in any::<usize>(),
        offset_s in -129_600_i64..=129_600,
        later_s in prop_oneof![0_i64..=7_200, 0_i64..=172_800],
    ) {
        let zone = &zone_names()[zone_index % zone_names().len()];
        let policy = BusinessDayPolicy::new(zone, cutoff_hour, cutoff_minute).unwrap();
        let instant = instant_near_transition(zone, year, pick, offset_s);

        // Containment: the instant lies in its business date's window.
        let date = policy.business_date_of(instant).unwrap();
        let (start, end) = policy.window(date).unwrap();
        prop_assert!(start <= instant && instant < end, "{instant} not in {date}: [{start}, {end})");

        // Starts never decrease, so windows tile time without overlapping.
        let previous_start = policy.start_of(date.previous().unwrap()).unwrap();
        prop_assert!(previous_start <= start);
        prop_assert_eq!(end, policy.start_of(date.next().unwrap()).unwrap());

        // A business date contains its own start.
        prop_assert_eq!(policy.business_date_of(start).unwrap(), date);

        // Monotonic: a later instant never has an earlier business date.
        let later = Timestamp::from_micros(instant.as_micros() + later_s * 1_000_000).unwrap();
        let later_date: BusinessDate = policy.business_date_of(later).unwrap();
        prop_assert!(later_date >= date, "{later} is on {later_date}, before {date}");
    }
}

/// Every zone, every transition from 1970 to 2037, several cutoffs (including ones inside
/// skipped and repeated hours, and around midnight), and instants just around each transition.
/// Slow in debug builds, so it runs with `cargo test -- --ignored` (CI does).
#[test]
#[ignore = "exhaustive sweep: run with --ignored"]
fn exhaustive_sweep_of_every_transition() {
    let from = jiff::civil::date(1970, 1, 1).to_zoned(jiff::tz::TimeZone::UTC).unwrap().timestamp();
    let until =
        jiff::civil::date(2038, 1, 1).to_zoned(jiff::tz::TimeZone::UTC).unwrap().timestamp();
    let cutoffs = [(0, 0), (0, 30), (1, 30), (2, 0), (2, 30), (3, 0), (4, 0), (23, 30)];
    let offsets_s = [-86_400, -3_601, -3_600, -1_800, -1, 0, 1, 1_800, 3_599, 3_600, 86_400];
    let mut checked = 0_u64;
    for zone in zone_names() {
        let time_zone = jiff::tz::TimeZone::get(zone).unwrap();
        let transitions: Vec<jiff::Timestamp> = time_zone
            .following(from)
            .map(|transition| transition.timestamp())
            .take_while(|&timestamp| timestamp < until)
            .collect();
        for (cutoff_hour, cutoff_minute) in cutoffs {
            let policy = BusinessDayPolicy::new(zone, cutoff_hour, cutoff_minute).unwrap();
            let mut last_date: Option<BusinessDate> = None;
            for transition in &transitions {
                for offset_s in offsets_s {
                    let micros = transition.as_microsecond() + offset_s * 1_000_000;
                    let instant = Timestamp::from_micros(micros).unwrap();
                    let date = policy.business_date_of(instant).unwrap();
                    let (start, end) = policy.window(date).unwrap();
                    assert!(
                        start <= instant && instant < end,
                        "{zone} {cutoff_hour:02}:{cutoff_minute:02}: {instant} not in {date}"
                    );
                    if offset_s == offsets_s[0] {
                        last_date = None; // a new transition: offsets restart earlier in time
                    }
                    if let Some(last) = last_date {
                        assert!(date >= last, "{zone}: {instant} went back to {date}");
                    }
                    last_date = Some(date);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 100_000, "only {checked} instants checked");
}
