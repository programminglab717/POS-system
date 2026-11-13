//! Known answers for availability (ADR-0033): its payload, pinned; the fold, the latest change
//! standing; and the commands, which refuse what the fold would report.

use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Hlc, Id, Timestamp};

use super::*;
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::{Note, PayloadError, ReasonCode};
use crate::order::Reason;
use crate::schema::{DecodeError, DomainEvent};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn location() -> Id<Location> {
    id(0x100)
}

/// A time, `minutes` after 8:00 on 13 November 2026.
fn at(minutes: i64) -> Timestamp {
    let micros = minutes.checked_mul(60_000_000).and_then(|m| m.checked_add(1_794_560_400_000_000));
    Timestamp::from_micros(micros.unwrap()).unwrap()
}

fn changed(kind: ItemKind, available: bool, until: Option<Timestamp>) -> AvailabilityEvent {
    AvailabilityEvent::Changed(AvailabilityChanged { kind, available, until, reason: None })
}

/// Applies events to an item's availability, each with fresh metadata.
struct Script {
    availability: Availability,
    events: u64,
}

impl Script {
    fn new() -> Script {
        Script { availability: Availability::new(id(0x401)), events: 0 }
    }

    fn apply_at(
        &mut self,
        device: u64,
        location: Id<Location>,
        event: &AvailabilityEvent,
    ) -> Id<Event> {
        self.events = self.events.checked_add(1).unwrap();
        let meta = EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location,
            origin_device: id::<Device>(device),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_794_560_400_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-11-13".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        };
        self.availability.apply(&meta, event);
        meta.event_id
    }

    fn apply(&mut self, event: &AvailabilityEvent) -> Id<Event> {
        self.apply_at(0xD1, location(), event)
    }

    fn kinds(&self) -> Vec<(Id<Event>, ConflictKind)> {
        self.availability
            .conflicts()
            .iter()
            .map(|conflict| (conflict.event, conflict.kind))
            .collect()
    }
}

#[test]
fn the_latest_change_stands_and_an_86_ends_at_its_time() {
    let mut script = Script::new();
    assert!(!script.availability.sold_out_at(at(0)), "on sale until it is changed");
    // 86'd until 9:00, by one device; another puts it back; the first 86s it again.
    script.apply(&changed(ItemKind::Variant, false, Some(at(60))));
    assert!(script.availability.sold_out_at(at(59)));
    assert!(!script.availability.sold_out_at(at(60)), "back on sale at its time, with no event");
    let back = script.apply_at(0xD2, location(), &changed(ItemKind::Variant, true, None));
    assert!(!script.availability.sold_out_at(at(10)));
    assert_eq!(script.availability.latest().unwrap().event, back);
    assert_eq!(script.availability.latest().unwrap().device, id(0xD2));
    script.apply(&changed(ItemKind::Variant, false, None));
    assert!(script.availability.sold_out_at(at(100_000)), "until it is put back");
    assert!(script.kinds().is_empty());
}

#[test]
fn what_doesnt_fit_is_reported() {
    let mut script = Script::new();
    script.apply(&changed(ItemKind::Variant, false, None));
    let elsewhere = script.apply_at(0xD1, id(0x101), &changed(ItemKind::Variant, true, None));
    assert!(script.availability.sold_out_at(at(0)), "a change elsewhere doesn't apply");
    let other = script.apply(&changed(ItemKind::Modifier, true, None));
    assert!(!script.availability.sold_out_at(at(0)), "a change of the other kind applies");
    assert_eq!(
        script.kinds(),
        [(elsewhere, ConflictKind::WrongLocation), (other, ConflictKind::KindChanged)]
    );
}

#[test]
fn commands_refuse_what_the_fold_would_report() {
    let mut script = Script::new();
    let change = |available, until| {
        AvailabilityCommand::Change(AvailabilityChanged {
            kind: ItemKind::Variant,
            available,
            until,
            reason: None,
        })
    };
    let mut run = |command: AvailabilityCommand, now: Timestamp, location: Id<Location>| {
        let event = script.availability.decide(location, now, command)?;
        let before = script.availability.conflicts().len();
        script.apply_at(0xD1, location, &event);
        assert_eq!(script.availability.conflicts().len(), before);
        Ok::<(), AvailabilityError>(())
    };
    // On sale already.
    assert_eq!(run(change(true, None), at(0), location()), Err(AvailabilityError::NoChange));
    assert_eq!(
        run(change(true, Some(at(60))), at(0), location()),
        Err(AvailabilityError::Invalid(PayloadError::Invalid("until")))
    );
    assert_eq!(
        run(change(false, Some(at(0))), at(0), location()),
        Err(AvailabilityError::UntilPast)
    );
    run(change(false, Some(at(60))), at(0), location()).unwrap();
    assert_eq!(
        run(change(false, Some(at(60))), at(1), location()),
        Err(AvailabilityError::NoChange)
    );
    // Longer, or until put back, changes it.
    run(change(false, Some(at(120))), at(1), location()).unwrap();
    assert_eq!(run(change(true, None), at(2), id(0x101)), Err(AvailabilityError::WrongLocation));
    let modifier = AvailabilityCommand::Change(AvailabilityChanged {
        kind: ItemKind::Modifier,
        available: true,
        until: None,
        reason: None,
    });
    assert_eq!(run(modifier, at(2), location()), Err(AvailabilityError::WrongKind));
    // Once its time passed, it is on sale: putting it back changes nothing.
    assert_eq!(run(change(true, None), at(120), location()), Err(AvailabilityError::NoChange));
    run(change(true, None), at(3), location()).unwrap();
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The availability payload, pinned forever: Python's `cbor2` encoded each example from the key
/// table (ADR-0033). If this test fails, the payload format changed, and stored events would no
/// longer decode.
#[test]
fn the_payload_format_is_pinned() {
    let sold_out = AvailabilityEvent::Changed(AvailabilityChanged {
        kind: ItemKind::Variant,
        available: false,
        until: Some(at(0)),
        reason: Some(Reason {
            code: ReasonCode::new("ran_out").unwrap(),
            note: Some(Note::new("last one sold").unwrap()),
        }),
    });
    let pinned = [
        (
            sold_out,
            "a5010002f4031b00066024a72b8400046772616e5f6f7574056d6c617374206f6e6520736f6c64",
        ),
        (changed(ItemKind::Modifier, true, None), "a2010102f5"),
    ];
    assert_eq!(AvailabilityEvent::SCHEMAS.len(), 1);
    for (event, hex) in pinned {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), "availability.changed");
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice(), "{hex}");
        assert_eq!(AvailabilityEvent::decode(&schema, &payload), Ok(event));
    }
}

#[test]
fn the_payload_keeps_its_rules() {
    let decode = |event: &AvailabilityEvent| {
        let schema = event.schema().to_ref().unwrap();
        AvailabilityEvent::from_value(&schema, &event.to_value())
    };
    assert_eq!(
        decode(&changed(ItemKind::Variant, true, Some(at(0)))),
        Err(DecodeError::Malformed(PayloadError::Invalid("until")))
    );
    // A note with no reason.
    let schema = changed(ItemKind::Variant, false, None).schema().to_ref().unwrap();
    let mut entries =
        changed(ItemKind::Variant, false, None).to_value().as_map().unwrap().clone().into_entries();
    entries.push((keel_events::cbor::Value::Unsigned(5), keel_events::cbor::Value::from("why")));
    let map = keel_events::cbor::Value::Map(keel_events::cbor::Map::from_entries(entries).unwrap());
    assert_eq!(
        AvailabilityEvent::from_value(&schema, &map),
        Err(DecodeError::Malformed(PayloadError::Invalid("note")))
    );
}
