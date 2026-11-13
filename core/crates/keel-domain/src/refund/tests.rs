//! Known answers for refunds (ADR-0032): their payloads, pinned; the fold, which applies what it
//! can and reports the rest; and the commands, which refuse it.

use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Currency, Hlc, Id, Money};

use super::*;
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::PayloadError;
use crate::payment::Tender;
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

fn initiated(amount: Money) -> RefundInitiated {
    RefundInitiated { order: id(0xA1), tender: Tender::Cash, amount }
}

fn completed(amount: Money) -> RefundCompleted {
    RefundCompleted { amount, rounding: None }
}

/// Applies events to a refund, each with fresh metadata.
struct Script {
    refund: Refund,
    events: u64,
}

impl Script {
    fn new() -> Script {
        Script { refund: Refund::new(id(0xC1)), events: 0 }
    }

    fn apply_by(&mut self, device: u64, location: Id<Location>, event: &RefundEvent) -> Id<Event> {
        self.events = self.events.checked_add(1).unwrap();
        let meta = EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location,
            origin_device: id::<Device>(device),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_790_600_000_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-11-13".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        };
        self.refund.apply(&meta, event);
        meta.event_id
    }

    fn apply(&mut self, event: &RefundEvent) -> Id<Event> {
        self.apply_by(0xD1, location(), event)
    }

    fn kinds(&self) -> Vec<(Id<Event>, ConflictKind)> {
        self.refund.conflicts().iter().map(|conflict| (conflict.event, conflict.kind)).collect()
    }
}

#[test]
fn a_refund_is_started_then_paid_back() {
    let mut script = Script::new();
    let started = script.apply(&RefundEvent::Initiated(initiated(usd(708))));
    let paid = RefundCompleted { amount: usd(708), rounding: Some(usd(2)) };
    script.apply(&RefundEvent::Completed(paid));
    let info = script.refund.info().unwrap();
    assert_eq!((info.order, info.tender, info.amount), (id(0xA1), Tender::Cash, usd(708)));
    assert_eq!((info.device, info.initiated_by), (id(0xD1), started));
    assert_eq!(script.refund.completed(), Some(&paid));
    assert_eq!(paid.handed(), Some(usd(710)));
    assert!(script.refund.conflicts().is_empty());
}

#[test]
fn what_doesnt_fit_is_reported() {
    let mut script = Script::new();
    let early = script.apply(&RefundEvent::Completed(completed(usd(100))));
    script.apply(&RefundEvent::Initiated(initiated(usd(500))));
    let again = script.apply(&RefundEvent::Initiated(initiated(usd(900))));
    let elsewhere = script.apply_by(0xD1, id(0x101), &RefundEvent::Completed(completed(usd(500))));
    let foreign = script.apply(&RefundEvent::Completed(completed(eur(500))));
    // Another device's completion, for more than was started: it applies, and is reported twice.
    let other = script.apply_by(0xD2, location(), &RefundEvent::Completed(completed(usd(600))));
    let twice = script.apply(&RefundEvent::Completed(completed(usd(500))));
    assert_eq!(script.refund.info().unwrap().amount, usd(500));
    assert_eq!(script.refund.completed().unwrap().amount, usd(600));
    assert_eq!(
        script.kinds(),
        [
            (early, ConflictKind::BeforeInitiation),
            (again, ConflictKind::DuplicateInitiation),
            (elsewhere, ConflictKind::WrongLocation),
            (foreign, ConflictKind::CurrencyMismatch),
            (other, ConflictKind::NotInitiator { by: id(0xD2) }),
            (other, ConflictKind::BeyondInitiated),
            (twice, ConflictKind::DuplicateCompletion),
        ]
    );
}

#[test]
fn commands_refuse_what_the_fold_would_report() {
    let mut script = Script::new();
    let mut run = |device: u64, location: Id<Location>, command: RefundCommand| {
        let event = script.refund.decide(location, id(device), command)?;
        let before = script.refund.conflicts().len();
        script.apply_by(device, location, &event);
        assert_eq!(script.refund.conflicts().len(), before, "a valid command caused a conflict");
        Ok::<(), RefundError>(())
    };
    let complete = |amount| RefundCommand::Complete(completed(amount));
    assert_eq!(run(0xD1, location(), complete(usd(100))), Err(RefundError::NotInitiated));
    assert_eq!(
        run(0xD1, location(), RefundCommand::Initiate(initiated(usd(0)))),
        Err(RefundError::Invalid(PayloadError::Invalid("amount")))
    );
    run(0xD1, location(), RefundCommand::Initiate(initiated(usd(500)))).unwrap();
    assert_eq!(
        run(0xD1, location(), RefundCommand::Initiate(initiated(usd(500)))),
        Err(RefundError::AlreadyInitiated)
    );
    assert_eq!(run(0xD1, id(0x101), complete(usd(500))), Err(RefundError::WrongLocation));
    assert_eq!(run(0xD2, location(), complete(usd(500))), Err(RefundError::NotInitiator));
    assert_eq!(run(0xD1, location(), complete(eur(500))), Err(RefundError::WrongCurrency));
    assert_eq!(run(0xD1, location(), complete(usd(501))), Err(RefundError::BeyondInitiated));
    let zero_rounding =
        RefundCommand::Complete(RefundCompleted { amount: usd(500), rounding: Some(usd(0)) });
    assert_eq!(
        run(0xD1, location(), zero_rounding),
        Err(RefundError::Invalid(PayloadError::Invalid("rounding")))
    );
    run(0xD1, location(), complete(usd(500))).unwrap();
    assert_eq!(run(0xD1, location(), complete(usd(500))), Err(RefundError::AlreadyCompleted));
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The refund payloads, pinned forever: one example of each schema. Python's `cbor2` encoded
/// each from the key tables (ADR-0032). If this test fails, a payload format changed, and stored
/// events would no longer decode.
#[test]
fn the_payload_formats_are_pinned() {
    let pinned = [
        (
            RefundEvent::Initiated(initiated(usd(708))),
            "refund.initiated",
            "a301500192f0c10000700080000000000000a1020003821902c463555344",
        ),
        (
            RefundEvent::Completed(RefundCompleted { amount: usd(708), rounding: Some(usd(2)) }),
            "refund.completed",
            "a201821902c46355534402820263555344",
        ),
    ];
    assert_eq!(pinned.len(), RefundEvent::SCHEMAS.len());
    for (event, name, hex) in pinned {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), name);
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice(), "{name}");
        assert_eq!(RefundEvent::decode(&schema, &payload), Ok(event), "{name}");
    }
}

#[test]
fn payloads_keep_their_rules() {
    let decode = |event: &RefundEvent| {
        let schema = event.schema().to_ref().unwrap();
        RefundEvent::from_value(&schema, &event.to_value())
    };
    let invalid = |field| Err(DecodeError::Malformed(PayloadError::Invalid(field)));
    assert_eq!(decode(&RefundEvent::Initiated(initiated(usd(0)))), invalid("amount"));
    assert_eq!(decode(&RefundEvent::Initiated(initiated(usd(-1)))), invalid("amount"));
    assert_eq!(decode(&RefundEvent::Completed(completed(usd(0)))), invalid("amount"));
    let rounded = |rounding| {
        RefundEvent::Completed(RefundCompleted { amount: usd(100), rounding: Some(rounding) })
    };
    assert!(decode(&rounded(usd(-2))).is_ok(), "rounding can take away");
    assert_eq!(decode(&rounded(usd(0))), invalid("rounding"));
    assert_eq!(decode(&rounded(eur(2))), invalid("rounding"));
}
