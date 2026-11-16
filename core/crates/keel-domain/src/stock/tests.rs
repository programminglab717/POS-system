//! Known answers for stock (ADR-0036): its payloads, pinned; the fold, which keeps what applies;
//! the levels, from the latest count as of its time; and the commands, which refuse what the fold
//! would report.

use keel_events::envelope::{Actor, Device, Event, Location, SchemaName};
use keel_types::{Hlc, Id, Quantity, Timestamp, Unit};

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

fn each(n: i64) -> Quantity {
    Quantity::from_whole(n, Unit::Each).unwrap()
}

/// A time, `minutes` after 8:00 on 13 November 2026.
fn at(minutes: i64) -> Timestamp {
    let micros = minutes.checked_mul(60_000_000).and_then(|m| m.checked_add(1_794_560_400_000_000));
    Timestamp::from_micros(micros.unwrap()).unwrap()
}

fn moved(movement: Movement, quantity: Quantity) -> StockEvent {
    StockEvent::Moved(StockMoved { movement, quantity, reason: None })
}

fn counted(quantity: Quantity, as_of: Timestamp) -> StockEvent {
    StockEvent::Counted(StockCounted { quantity, as_of, note: None })
}

/// Applies events to an item's stock, each recorded at a minute after 8:00.
struct Script {
    stock: Stock,
    events: u64,
}

impl Script {
    fn new() -> Script {
        Script { stock: Stock::new(id(0x401)), events: 0 }
    }

    fn apply_at(&mut self, minute: i64, place: Id<Location>, event: &StockEvent) -> Id<Event> {
        self.events = self.events.checked_add(1).unwrap();
        let wall = u64::try_from(at(minute).as_micros() / 1_000).unwrap();
        let meta = EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location: place,
            origin_device: id::<Device>(0xD1),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(wall, u16::try_from(self.events).unwrap()).unwrap(),
            business_date: "2026-11-13".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        };
        self.stock.apply(&meta, event);
        meta.event_id
    }

    fn apply(&mut self, minute: i64, event: &StockEvent) -> Id<Event> {
        self.apply_at(minute, location(), event)
    }

    fn levels(&self, flows: &[Flow]) -> Levels {
        levels(Unit::Each, self.stock.entries(), flows).unwrap()
    }
}

fn sold(minute: i64, n: i64) -> Flow {
    Flow { at: at(minute), delta: each(n).checked_neg().unwrap() }
}

#[test]
fn on_hand_is_what_was_received_less_what_sold_since() {
    let mut script = Script::new();
    assert!(!script.stock.is_tracked());
    // Sales before its stock's first event don't count: it wasn't tracked.
    script.apply(10, &moved(Movement::Received, each(12)));
    let flows = [sold(5, 3), sold(20, 2), Flow { at: at(30), delta: each(1) }];
    let levels = script.levels(&flows);
    assert_eq!((levels.on_hand, levels.reserved, levels.available), (each(11), each(0), each(11)));
    script.apply(40, &moved(Movement::Wasted, each(1)));
    script.apply(41, &moved(Movement::Adjusted, each(-2)));
    script.apply(42, &moved(Movement::Reserved, each(3)));
    let levels = script.levels(&flows);
    assert_eq!((levels.on_hand, levels.reserved, levels.available), (each(8), each(3), each(5)));
    script.apply(43, &moved(Movement::Released, each(1)));
    assert_eq!(script.levels(&flows).reserved, each(2));
    // More sold than there was: below zero, never refused.
    assert_eq!(script.levels(&[sold(50, 20)]).on_hand, each(-11));
}

#[test]
fn a_count_as_of_a_time_keeps_what_moved_after_it() {
    let mut script = Script::new();
    script.apply(0, &moved(Movement::Received, each(20)));
    // Counted at 9:05 as of 9:00: 15 there then. What sold at 9:02 still comes off; what sold at
    // 8:50 was already gone when counted.
    let count = script.apply(65, &counted(each(15), at(60)));
    script.apply(30, &moved(Movement::Received, each(6)));
    let levels = script.levels(&[sold(50, 4), sold(62, 1)]);
    assert_eq!(levels.on_hand, each(14));
    assert_eq!(levels.counted.map(|count| count.event), Some(count));
    // An earlier count, recorded later, doesn't replace a later one.
    script.apply(70, &counted(each(99), at(55)));
    assert_eq!(script.levels(&[sold(62, 1)]).on_hand, each(14));
    // A later count does.
    script.apply(80, &counted(each(3), at(75)));
    assert_eq!(script.levels(&[sold(62, 1), sold(76, 1)]).on_hand, each(2));
    // Reserved isn't counted: it stays as it was.
    script.apply(81, &moved(Movement::Reserved, each(1)));
    script.apply(85, &counted(each(10), at(84)));
    assert_eq!(script.levels(&[]).reserved, each(1));
}

#[test]
fn what_doesnt_fit_is_reported() {
    let mut script = Script::new();
    script.apply(0, &moved(Movement::Received, each(5)));
    let elsewhere = script.apply_at(1, id(0x101), &moved(Movement::Received, each(5)));
    let kilos = Quantity::from_micros(1_000_000, Unit::Kilogram);
    let wrong = script.apply(2, &moved(Movement::Received, kilos));
    let over = script.apply(3, &moved(Movement::Released, each(1)));
    let kinds: Vec<(Id<Event>, ConflictKind)> =
        script.stock.conflicts().iter().map(|c| (c.event, c.kind)).collect();
    assert_eq!(
        kinds,
        [
            (elsewhere, ConflictKind::WrongLocation),
            (wrong, ConflictKind::WrongUnit),
            (over, ConflictKind::OverReleased)
        ]
    );
    assert_eq!(script.stock.unit(), Some(Unit::Each));
    assert_eq!(script.levels(&[]).on_hand, each(5));
    assert_eq!(script.stock.reserved().unwrap(), each(-1), "the release applied");
    // A sale in another unit leaves on hand as it was.
    assert_eq!(script.levels(&[Flow { at: at(5), delta: kilos }]).on_hand, each(5));
}

#[test]
fn commands_refuse_what_the_fold_would_report() {
    let mut script = Script::new();
    let now = at(10);
    let decide = |script: &Script, command| script.stock.decide(location(), now, command);
    script.apply(
        0,
        &decide(
            &script,
            StockCommand::Move(StockMoved {
                movement: Movement::Received,
                quantity: each(4),
                reason: None,
            }),
        )
        .unwrap(),
    );
    let move_of =
        |movement, quantity| StockCommand::Move(StockMoved { movement, quantity, reason: None });
    assert_eq!(
        script.stock.decide(id(0x101), now, move_of(Movement::Received, each(1))),
        Err(StockError::WrongLocation)
    );
    let kilos = Quantity::from_micros(1, Unit::Kilogram);
    assert_eq!(decide(&script, move_of(Movement::Received, kilos)), Err(StockError::WrongUnit));
    assert_eq!(
        decide(&script, move_of(Movement::Released, each(1))),
        Err(StockError::BeyondReserved)
    );
    let invalid = Err(StockError::Invalid(PayloadError::Invalid("quantity")));
    assert_eq!(decide(&script, move_of(Movement::Received, each(0))), invalid);
    assert_eq!(decide(&script, move_of(Movement::Wasted, each(-1))), invalid);
    assert_eq!(decide(&script, move_of(Movement::Adjusted, each(0))), invalid);
    assert!(decide(&script, move_of(Movement::Adjusted, each(-3))).is_ok());
    let count = |quantity, as_of| StockCommand::Count(StockCounted { quantity, as_of, note: None });
    assert_eq!(decide(&script, count(each(3), at(11))), Err(StockError::CountedLater));
    assert_eq!(decide(&script, count(each(-1), at(9))), invalid);
    assert!(decide(&script, count(each(0), now)).is_ok());
    script.apply(1, &decide(&script, move_of(Movement::Reserved, each(2))).unwrap());
    assert!(decide(&script, move_of(Movement::Released, each(2))).is_ok());
    // A billion is the most at once, either way: found in review.
    let most = i64::try_from(super::MAX_QUANTITY).unwrap();
    let past = Quantity::from_micros(most + 1, Unit::Each);
    assert_eq!(decide(&script, move_of(Movement::Received, past)), Err(StockError::TooMuch));
    let down = Quantity::from_micros(-most - 1, Unit::Each);
    assert_eq!(decide(&script, move_of(Movement::Adjusted, down)), Err(StockError::TooMuch));
    assert!(
        decide(&script, move_of(Movement::Received, Quantity::from_micros(most, Unit::Each)))
            .is_ok()
    );
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The stock payloads, pinned forever: Python's `cbor2` encoded each from the key tables
/// (ADR-0036). If this test fails, a payload format changed, and stored events would no longer
/// decode.
#[test]
fn the_payload_formats_are_pinned() {
    let pinned = [
        (
            StockEvent::Moved(StockMoved {
                movement: Movement::Received,
                quantity: each(12),
                reason: Some(Reason {
                    code: ReasonCode::new("delivery").unwrap(),
                    note: Some(Note::new("Morning bread").unwrap()),
                }),
            }),
            "stock.moved",
            "a4010002821a00b71b006465616368036864656c6976657279046d4d6f726e696e67206272656164",
        ),
        (moved(Movement::Adjusted, each(-2)), "stock.moved", "a2010102823a001e847f6465616368"),
        (
            StockEvent::Counted(StockCounted {
                quantity: Quantity::from_micros(7_500_000, Unit::Kilogram),
                as_of: at(0),
                note: Some(Note::new("Back room").unwrap()),
            }),
            "stock.counted",
            "a301821a007270e0626b67021b00066024a72b840003694261636b20726f6f6d",
        ),
    ];
    for (event, name, hex) in pinned {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!((schema.name.as_str(), schema.version.get()), (name, 1));
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice(), "{name}");
        assert_eq!(StockEvent::decode(&schema, &payload), Ok(event), "{name}");
    }
    assert_eq!(StockEvent::SCHEMAS.len(), 2);
    let unknown = keel_events::envelope::SchemaRef {
        name: SchemaName::new("stock.transferred").unwrap(),
        version: 1.try_into().unwrap(),
    };
    assert_eq!(
        StockEvent::from_value(&unknown, &keel_events::cbor::Value::Null),
        Err(DecodeError::UnknownSchema)
    );
}

#[test]
fn a_note_comes_with_a_reason() {
    let event = moved(Movement::Received, each(1));
    let (schema, _) = event.encode().unwrap();
    let mut entries: Vec<(keel_events::cbor::Value, keel_events::cbor::Value)> =
        event.to_value().as_map().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    entries.push((keel_events::cbor::Value::Unsigned(4), keel_events::cbor::Value::from("x")));
    let payload =
        keel_events::cbor::Value::Map(keel_events::cbor::Map::from_entries(entries).unwrap());
    assert_eq!(
        StockEvent::from_value(&schema, &payload),
        Err(DecodeError::Malformed(PayloadError::Invalid("note")))
    );
}
