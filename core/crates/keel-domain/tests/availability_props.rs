//! Property tests for availability (ADR-0033), against independent models:
//! - the fold, given any changes in any order, from any devices and two locations, leaves the
//!   item as a model predicts: the latest change at its first location standing, and the
//!   conflicts reported. The model reads the events as queries over their positions;
//! - whether the item is sold out at any time is what the standing change says of that time;
//! - a device's commands, faulty ones included, are refused exactly when a model of the rules
//!   refuses them, for the same reason, and accepted ones apply with no conflict.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_domain::aggregate::{Aggregate, EventMeta};
use keel_domain::availability::{
    Availability, AvailabilityChanged, AvailabilityCommand, AvailabilityError, AvailabilityEvent,
    ConflictKind, ItemKind,
};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Hlc, Id, Timestamp};
use proptest::prelude::*;
use support::id;

fn location() -> Id<Location> {
    id(0x100)
}

/// A time, `minutes` after 8:00 on 13 November 2026.
fn at(minutes: i64) -> Timestamp {
    Timestamp::from_micros(1_794_560_400_000_000 + minutes * 60_000_000).unwrap()
}

fn meta_at(device: u64, seq: u64, location: Id<Location>) -> EventMeta {
    EventMeta {
        event_id: id((device << 24) + seq),
        location,
        origin_device: id::<Device>(device),
        origin_seq: seq.try_into().unwrap(),
        hlc: Hlc::new(1_000 + seq, 0).unwrap(),
        business_date: "2026-11-13".parse().unwrap(),
        actor: Actor::TeamMember(id(0x501)),
        approval: None,
    }
}

/// A change among few times, so that they collide: an 86 until one of them or until put back,
/// or an item put back; of a variant, mostly.
fn any_change() -> impl Strategy<Value = AvailabilityChanged> {
    let kind = prop_oneof![9 => Just(ItemKind::Variant), 1 => Just(ItemKind::Modifier)];
    let until = prop::option::of((0_i64..6).prop_map(|n| at(n * 30)));
    (kind, any::<bool>(), until).prop_map(|(kind, available, until)| AvailabilityChanged {
        kind,
        available,
        until: if available { None } else { until },
        reason: None,
    })
}

/// What the fold model predicts: the change that stands, and the conflicts.
#[derive(Debug, PartialEq, Eq)]
struct Expected {
    latest: Option<(AvailabilityChanged, Id<Event>)>,
    conflicts: Vec<(Id<Event>, ConflictKind)>,
}

fn expected(events: &[(EventMeta, AvailabilityChanged)]) -> Expected {
    let Some((first, _)) = events.first() else {
        return Expected { latest: None, conflicts: Vec::new() };
    };
    let home = first.location;
    let applying: Vec<usize> =
        (0..events.len()).filter(|&i| events[i].0.location == home).collect();
    let mut conflicts = Vec::new();
    for (i, (meta, changed)) in events.iter().enumerate() {
        if meta.location != home {
            conflicts.push((meta.event_id, ConflictKind::WrongLocation));
            continue;
        }
        // The change applied just before it, at the item's location.
        let before = applying.iter().copied().take_while(|&j| j < i).last();
        if before.is_some_and(|j| events[j].1.kind != changed.kind) {
            conflicts.push((meta.event_id, ConflictKind::KindChanged));
        }
    }
    let latest = applying.last().map(|&i| (events[i].1.clone(), events[i].0.event_id));
    Expected { latest, conflicts }
}

fn actual(availability: &Availability) -> Expected {
    Expected {
        latest: availability.latest().map(|latest| (latest.changed.clone(), latest.event)),
        conflicts: availability.conflicts().iter().map(|c| (c.event, c.kind)).collect(),
    }
}

proptest! {
    /// Any changes fold as the model predicts, and the item is sold out exactly when the change
    /// that stands says so of the time.
    #[test]
    fn the_fold_matches_the_model(
        changes in prop::collection::vec((1_u64..=3, prop::bool::weighted(0.1), any_change()), 0..20),
        times in prop::collection::vec(-10_i64..200, 1..8),
    ) {
        let events: Vec<(EventMeta, AvailabilityChanged)> = (1..)
            .zip(changes)
            .map(|(seq, (device, elsewhere, changed))| {
                let at = if elsewhere { id(0x101) } else { location() };
                (meta_at(device, seq, at), changed)
            })
            .collect();
        let mut availability = Availability::new(id(0x401));
        for (meta, changed) in &events {
            let event = AvailabilityEvent::Changed(changed.clone());
            let (schema, payload) = event.encode().unwrap();
            prop_assert_eq!(&AvailabilityEvent::decode(&schema, &payload).unwrap(), &event);
            availability.apply(meta, &event);
        }
        let model = expected(&events);
        prop_assert_eq!(actual(&availability), expected(&events));
        for minutes in times {
            let now = at(minutes);
            let sold_out = model.latest.as_ref().is_some_and(|(changed, _)| {
                !changed.available && changed.until.is_none_or(|until| now.as_micros() < until.as_micros())
            });
            prop_assert_eq!(availability.sold_out_at(now), sold_out);
        }
    }

    /// A device's commands are refused exactly as the model refuses them, and accepted ones
    /// apply with no conflict.
    #[test]
    fn commands_do_what_the_model_says(
        steps in prop::collection::vec((any_change(), any::<bool>(), 0_i64..200, prop::bool::weighted(0.1)), 1..20),
    ) {
        let mut availability = Availability::new(id(0x401));
        // The model: the location and kind it first took, and the change that stands.
        let mut home: Option<Id<Location>> = None;
        let mut latest: Option<AvailabilityChanged> = None;
        let mut clock = 0;
        for (seq, (mut changed, back_with_until, minutes, elsewhere)) in (1..).zip(steps) {
            // Time goes forward, and a faulty step puts an item back until a time.
            clock = clock.max(minutes);
            let now = at(clock);
            if changed.available && back_with_until {
                changed.until = Some(at(clock + 30));
            }
            let at_place = if elsewhere { id(0x101) } else { location() };
            let sold_out = latest.as_ref().is_some_and(|latest| {
                !latest.available && latest.until.is_none_or(|until| now.as_micros() < until.as_micros())
            });
            let refusal = if changed.available && changed.until.is_some() {
                Some("invalid")
            } else if home.is_some_and(|home| home != at_place) {
                Some("location")
            } else if latest.as_ref().is_some_and(|latest| latest.kind != changed.kind) {
                Some("kind")
            } else if changed.until.is_some_and(|until| until.as_micros() <= now.as_micros()) {
                Some("past")
            } else if (changed.available && !sold_out)
                || (!changed.available
                    && sold_out
                    && latest.as_ref().map(|latest| latest.until) == Some(changed.until))
            {
                Some("same")
            } else {
                None
            };
            let decided = availability.decide(at_place, now, AvailabilityCommand::Change(changed.clone()));
            let got = decided.as_ref().err().map(|error| match error {
                AvailabilityError::Invalid(_) => "invalid",
                AvailabilityError::WrongLocation => "location",
                AvailabilityError::WrongKind => "kind",
                AvailabilityError::UntilPast => "past",
                AvailabilityError::NoChange => "same",
                other => panic!("no command here is refused with {other:?}"),
            });
            prop_assert_eq!(got, refusal);
            if let Ok(event) = decided {
                let before = availability.conflicts().len();
                availability.apply(&meta_at(1, seq, at_place), &event);
                prop_assert_eq!(availability.conflicts().len(), before);
                home = Some(at_place);
                latest = Some(changed);
            }
        }
    }
}
