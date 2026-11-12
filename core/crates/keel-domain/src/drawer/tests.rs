//! Known answers for drawer sessions (ADR-0031): their payloads, pinned; the fold, which counts
//! every cash and reports what doesn't fit; and the commands, which refuse it.

use keel_events::cbor::Value;
use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Currency, Hlc, Id, Money};

use super::*;
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::{Note, PayloadError, ReasonCode};
use crate::order::Reason;
use crate::schema::{DecodeError, DomainEvent};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::USD)
}

fn eur(minor: i64) -> Money {
    Money::from_minor(minor, Currency::EUR)
}

fn location() -> Id<Location> {
    id(0x100)
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

fn moved(kind: Movement, amount: Money) -> DrawerEvent {
    DrawerEvent::CashMoved(CashMoved { kind, amount, reason: reason("supplier") })
}

fn taken(payment: u64, amount: Money) -> DrawerEvent {
    DrawerEvent::CashTaken(CashTaken { payment: id(payment), amount })
}

/// Applies events to a drawer, each with fresh metadata, from device 0xD1 unless told otherwise.
struct Script {
    drawer: Drawer,
    events: u64,
}

impl Script {
    fn new() -> Script {
        Script { drawer: Drawer::new(id(0xDA)), events: 0 }
    }

    fn opened(float: Money) -> Script {
        let mut script = Script::new();
        script.apply(&DrawerEvent::Opened { float });
        script
    }

    fn meta(&mut self, device: u64, location: Id<Location>) -> EventMeta {
        self.events = self.events.checked_add(1).unwrap();
        EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location,
            origin_device: id::<Device>(device),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_790_600_000_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-11-12".parse().unwrap(),
            actor: Actor::TeamMember(id(0x501)),
            approval: None,
        }
    }

    fn apply(&mut self, event: &DrawerEvent) -> Id<Event> {
        self.apply_by(0xD1, event)
    }

    fn apply_by(&mut self, device: u64, event: &DrawerEvent) -> Id<Event> {
        let meta = self.meta(device, location());
        self.drawer.apply(&meta, event);
        meta.event_id
    }

    fn kinds(&self) -> Vec<(Id<Event>, ConflictKind)> {
        self.drawer.conflicts().iter().map(|conflict| (conflict.event, conflict.kind)).collect()
    }
}

#[test]
fn a_drawer_counts_its_cash_from_its_float_to_its_close() {
    let mut script = Script::new();
    assert!(!script.drawer.is_open());
    assert_eq!(script.drawer.expected(), Ok(None));
    script.apply(&DrawerEvent::Opened { float: usd(15_000) });
    assert!(script.drawer.is_open());
    let info = script.drawer.info().unwrap();
    assert_eq!((info.device, info.float), (id(0xD1), usd(15_000)));
    // $150.00 to begin, $10.89 and $25.00 of sales, $20.00 paid in, $25.00 paid out, $100.00
    // dropped: $80.89 expected.
    script.apply(&taken(0xB1, usd(1_089)));
    script.apply(&taken(0xB2, usd(2_500)));
    script.apply(&moved(Movement::PaidIn, usd(2_000)));
    script.apply(&moved(Movement::PaidOut, usd(2_500)));
    script.apply(&moved(Movement::Drop, usd(10_000)));
    script.apply(&DrawerEvent::NoSale { reason: reason("change_for_customer") });
    let sums = *script.drawer.sums().unwrap();
    assert_eq!(
        sums,
        Sums { taken: usd(3_589), paid_in: usd(2_000), paid_out: usd(2_500), dropped: usd(10_000) }
    );
    assert_eq!(script.drawer.no_sales(), 1);
    assert_eq!(script.drawer.expected(), Ok(Some(usd(8_089))));
    assert_eq!(script.drawer.payments().len(), 2);
    // Counted $80.50: $0.39 short.
    script.apply(&DrawerEvent::Closed(DrawerClosed { counted: usd(8_050), expected: usd(8_089) }));
    assert!(!script.drawer.is_open());
    assert_eq!(script.drawer.closed().unwrap().variance(), Some(usd(-39)));
    assert_eq!(script.kinds(), []);
}

#[test]
fn what_doesnt_fit_is_reported_and_no_cash_goes_uncounted() {
    let mut script = Script::new();
    let before = script.apply(&taken(0xB1, usd(100)));
    script.apply(&DrawerEvent::Opened { float: usd(10_000) });
    let again = script.apply(&DrawerEvent::Opened { float: usd(5_000) });
    script.apply(&taken(0xB2, usd(500)));
    let twice = script.apply(&taken(0xB2, usd(500)));
    let foreign = script.apply(&taken(0xB3, eur(500)));
    let foreign_move = script.apply(&moved(Movement::PaidIn, eur(100)));
    // Another device's event applies, and is reported.
    let other = script.apply_by(0xD2, &moved(Movement::PaidOut, usd(200)));
    let meta = script.meta(0xD1, id(0x101));
    script.drawer.apply(&meta, &taken(0xB4, usd(700)));
    let elsewhere = meta.event_id;
    let closed = DrawerEvent::Closed(DrawerClosed { counted: usd(10_300), expected: usd(10_300) });
    script.apply(&closed);
    let reclosed = script.apply(&closed);
    // Cash after the close is counted, and reported.
    let late = script.apply(&taken(0xB5, usd(900)));
    assert_eq!(script.drawer.info().unwrap().float, usd(10_000));
    let sums = script.drawer.sums().unwrap();
    assert_eq!((sums.taken, sums.paid_in, sums.paid_out), (usd(1_400), usd(0), usd(200)));
    assert!(!script.drawer.payments().contains(&id(0xB3)));
    assert_eq!(script.drawer.closed().unwrap().counted, usd(10_300));
    assert_eq!(
        script.kinds(),
        [
            (before, ConflictKind::BeforeOpening),
            (again, ConflictKind::DuplicateOpening),
            (twice, ConflictKind::DuplicatePayment(id(0xB2))),
            (foreign, ConflictKind::CurrencyMismatch),
            (foreign_move, ConflictKind::CurrencyMismatch),
            (other, ConflictKind::NotOpener { by: id(0xD2) }),
            (elsewhere, ConflictKind::WrongLocation),
            (reclosed, ConflictKind::DuplicateClose),
            (late, ConflictKind::AfterClose),
        ]
    );
}

#[test]
fn a_close_in_another_currency_doesnt_apply() {
    let mut script = Script::opened(usd(1_000));
    let foreign = script
        .apply(&DrawerEvent::Closed(DrawerClosed { counted: eur(1_000), expected: eur(1_000) }));
    assert!(script.drawer.is_open());
    assert_eq!(script.kinds(), [(foreign, ConflictKind::CurrencyMismatch)]);
}

/// A device working its drawer through commands: every event it records applies with no
/// conflict.
struct Device1 {
    script: Script,
}

impl Device1 {
    fn run(&mut self, command: DrawerCommand) -> Result<(), DrawerError> {
        self.run_as(0xD1, location(), command)
    }

    fn run_as(
        &mut self,
        device: u64,
        location: Id<Location>,
        command: DrawerCommand,
    ) -> Result<(), DrawerError> {
        let event = self.script.drawer.decide(location, id(device), command)?;
        let before = self.script.drawer.conflicts().len();
        self.script.apply_by(device, &event);
        assert_eq!(
            self.script.drawer.conflicts().len(),
            before,
            "a valid command caused a conflict"
        );
        Ok(())
    }
}

#[test]
fn commands_refuse_what_the_fold_would_report() {
    let open = |float| DrawerCommand::Open { float, currency: Currency::USD };
    let mut device = Device1 { script: Script::new() };
    assert_eq!(device.run(DrawerCommand::NoSale(reason("x"))), Err(DrawerError::NotOpened));
    assert_eq!(device.run(open(eur(100))), Err(DrawerError::WrongCurrency));
    assert_eq!(
        device.run(open(usd(-1))),
        Err(DrawerError::Invalid(PayloadError::Invalid("float")))
    );
    device.run(open(usd(10_000))).unwrap();
    assert_eq!(device.run(open(usd(10_000))), Err(DrawerError::AlreadyOpened));
    let take =
        |payment, amount| DrawerCommand::TakeCash(CashTaken { payment: id(payment), amount });
    device.run(take(0xB1, usd(500))).unwrap();
    assert_eq!(device.run(take(0xB1, usd(500))), Err(DrawerError::AlreadyTaken));
    assert_eq!(device.run(take(0xB2, eur(500))), Err(DrawerError::WrongCurrency));
    assert_eq!(
        device.run(take(0xB2, usd(0))),
        Err(DrawerError::Invalid(PayloadError::Invalid("amount")))
    );
    let pay_out = |amount| {
        DrawerCommand::MoveCash(CashMoved { kind: Movement::PaidOut, amount, reason: reason("x") })
    };
    assert_eq!(
        device.run(pay_out(usd(0))),
        Err(DrawerError::Invalid(PayloadError::Invalid("amount")))
    );
    // Only its opener writes it, at its location.
    assert_eq!(device.run_as(0xD2, location(), pay_out(usd(100))), Err(DrawerError::NotOpener));
    assert_eq!(device.run_as(0xD1, id(0x101), pay_out(usd(100))), Err(DrawerError::WrongLocation));
    device.run(pay_out(usd(100))).unwrap();
    // The close records what the drawer expected itself: $100.00, $5.00 in, $1.00 out.
    assert_eq!(
        device.run(DrawerCommand::Close { counted: eur(10_400) }),
        Err(DrawerError::WrongCurrency)
    );
    assert_eq!(
        device.run(DrawerCommand::Close { counted: usd(-1) }),
        Err(DrawerError::Invalid(PayloadError::Invalid("counted")))
    );
    device.run(DrawerCommand::Close { counted: usd(10_450) }).unwrap();
    let closed = device.script.drawer.closed().unwrap();
    assert_eq!((closed.expected, closed.variance()), (usd(10_400), Some(usd(50))));
    assert_eq!(device.run(take(0xB3, usd(100))), Err(DrawerError::Closed));
    assert_eq!(device.run(DrawerCommand::Close { counted: usd(0) }), Err(DrawerError::Closed));
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The drawer payloads, pinned forever: one example of each schema. Python's `cbor2` encoded
/// each from the key tables (ADR-0031). If this test fails, a payload format changed, and
/// stored events would no longer decode.
#[test]
fn the_payload_formats_are_pinned() {
    let pinned = [
        (DrawerEvent::Opened { float: usd(15_000) }, "drawer.opened", "a10182193a9863555344"),
        (
            taken(0xB1, usd(1_089)),
            "drawer.cash_taken",
            "a201500192f0c10000700080000000000000b1028219044163555344",
        ),
        (
            DrawerEvent::CashMoved(CashMoved {
                kind: Movement::PaidOut,
                amount: usd(2_500),
                reason: Reason {
                    code: ReasonCode::new("supplier").unwrap(),
                    note: Some(Note::new("milk delivery").unwrap()),
                },
            }),
            "drawer.cash_moved",
            "a4010102821909c4635553440368737570706c696572046d6d696c6b2064656c6976657279",
        ),
        (
            DrawerEvent::NoSale { reason: reason("change_for_customer") },
            "drawer.no_sale",
            "a101736368616e67655f666f725f637573746f6d6572",
        ),
        (
            DrawerEvent::Closed(DrawerClosed { counted: usd(17_432), expected: usd(17_589) }),
            "drawer.closed",
            "a201821944186355534402821944b563555344",
        ),
    ];
    assert_eq!(pinned.len(), DrawerEvent::SCHEMAS.len());
    for (event, name, hex) in pinned {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), name);
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice(), "{name}");
        assert_eq!(DrawerEvent::decode(&schema, &payload), Ok(event), "{name}");
    }
}

#[test]
fn payloads_keep_their_rules() {
    let decode = |event: &DrawerEvent| {
        let schema = event.schema().to_ref().unwrap();
        DrawerEvent::from_value(&schema, &event.to_value())
    };
    let invalid = |field| Err(DecodeError::Malformed(PayloadError::Invalid(field)));
    assert!(decode(&DrawerEvent::Opened { float: usd(0) }).is_ok());
    assert_eq!(decode(&DrawerEvent::Opened { float: usd(-1) }), invalid("float"));
    assert_eq!(decode(&taken(1, usd(0))), invalid("amount"));
    assert_eq!(decode(&moved(Movement::Drop, usd(-5))), invalid("amount"));
    let close = |counted, expected| DrawerEvent::Closed(DrawerClosed { counted, expected });
    assert!(decode(&close(usd(0), usd(-100))).is_ok(), "a drawer can be expected to be short");
    assert_eq!(decode(&close(usd(-1), usd(0))), invalid("counted"));
    assert_eq!(decode(&close(usd(100), eur(100))), invalid("expected"));
    // A movement's kind is one of three.
    let schema = moved(Movement::Drop, usd(5)).schema().to_ref().unwrap();
    let mut entries =
        moved(Movement::Drop, usd(5)).to_value().as_map().unwrap().clone().into_entries();
    entries.retain(|(key, _)| key.as_u64() != Some(1));
    entries.push((Value::Unsigned(1), Value::Unsigned(3)));
    let map = Value::Map(keel_events::cbor::Map::from_entries(entries).unwrap());
    assert!(DrawerEvent::from_value(&schema, &map).is_err());
}

/// Amounts that overflow: the fold reports them as such, and the commands refuse them before
/// they're recorded. Found in review: the fold called them another currency, and a session
/// holding one couldn't close.
#[test]
fn amounts_that_overflow_are_refused_and_reported() {
    let mut script = Script::opened(usd(0));
    script.apply(&taken(0xB1, usd(i64::MAX)));
    let over = script.apply(&taken(0xB2, usd(1)));
    assert_eq!(script.kinds(), [(over, ConflictKind::Overflow)]);
    assert_eq!(script.drawer.sums().unwrap().taken, usd(i64::MAX));

    let take =
        |payment, amount| DrawerCommand::TakeCash(CashTaken { payment: id(payment), amount });
    let paid_in = |amount| {
        DrawerCommand::MoveCash(CashMoved { kind: Movement::PaidIn, amount, reason: reason("x") })
    };
    let mut device = Device1 { script: Script::new() };
    device.run(DrawerCommand::Open { float: usd(100), currency: Currency::USD }).unwrap();
    // $1.00 of float and the most there is would be more than the drawer could hold.
    assert_eq!(device.run(take(0xB1, usd(i64::MAX))), Err(DrawerError::Overflow));
    device.run(take(0xB1, usd(i64::MAX - 100))).unwrap();
    assert_eq!(device.run(paid_in(usd(1))), Err(DrawerError::Overflow));
    device.run(DrawerCommand::Close { counted: usd(0) }).unwrap();

    // Dropped so far that a full count is more over than there is.
    let mut device = Device1 { script: Script::new() };
    device.run(DrawerCommand::Open { float: usd(0), currency: Currency::USD }).unwrap();
    let drop = DrawerCommand::MoveCash(CashMoved {
        kind: Movement::Drop,
        amount: usd(i64::MAX),
        reason: reason("x"),
    });
    device.run(drop).unwrap();
    assert_eq!(
        device.run(DrawerCommand::Close { counted: usd(i64::MAX) }),
        Err(DrawerError::Overflow)
    );
    device.run(DrawerCommand::Close { counted: usd(0) }).unwrap();
}
