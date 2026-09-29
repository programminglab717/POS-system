//! Known-answer tests for payments: the state machine, each conflict rule, each command's
//! checks, the payload rules, and the pinned payloads.

use keel_events::cbor::Value;
use keel_events::envelope::{Actor, Event, Location, SchemaName, SchemaRef};
use keel_types::{Currency, Hlc, Id, Money};

use super::*;
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::{Note, PayloadError, ProcessorRef, ReasonCode};
use crate::order::Reason;
use crate::schema::{DecodeError, DomainEvent};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::from_code("USD").unwrap())
}

fn cad(minor: i64) -> Money {
    Money::from_minor(minor, Currency::from_code("CAD").unwrap())
}

fn location() -> Id<Location> {
    id(0x100)
}

fn reference(text: &str) -> ProcessorRef {
    ProcessorRef::new(text).unwrap()
}

fn ended(code: &str) -> PaymentEnded {
    PaymentEnded {
        reason: Reason { code: ReasonCode::new(code).unwrap(), note: None },
        reference: None,
    }
}

fn initiated(tender: Tender, amount: Money) -> PaymentEvent {
    PaymentEvent::Initiated(PaymentInitiated { order: id(0xA), check: id(0xC1), tender, amount })
}

fn authorized(amount: Money) -> PaymentEvent {
    PaymentEvent::Authorized(PaymentAuthorized { amount, reference: Some(reference("pi_1")) })
}

fn captured(amount: Money) -> PaymentCaptured {
    PaymentCaptured { amount, tip: None, reference: Some(reference("pi_1")), cash: None }
}

fn in_cash(amount: Money, tendered: Money, rounding: Option<Money>) -> PaymentCaptured {
    PaymentCaptured {
        amount,
        tip: None,
        reference: None,
        cash: Some(CashTendered { tendered, rounding }),
    }
}

/// Applies events to a payment, each with fresh metadata, as a replica folding them in canonical
/// order would.
struct Script {
    payment: Payment,
    events: u64,
}

impl Script {
    fn new() -> Script {
        Script { payment: Payment::new(id(0xB1)), events: 0 }
    }

    fn initiated(tender: Tender) -> Script {
        let mut script = Script::new();
        script.apply(&initiated(tender, usd(2500)));
        script
    }

    fn meta_at(&mut self, location: Id<Location>) -> EventMeta {
        self.events = self.events.checked_add(1).unwrap();
        EventMeta {
            event_id: id(0xF000_u64.checked_add(self.events).unwrap()),
            location,
            origin_device: id(0xD),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_790_600_000_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-09-28".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        }
    }

    fn apply(&mut self, event: &PaymentEvent) -> Id<Event> {
        self.apply_at(location(), event)
    }

    fn apply_at(&mut self, location: Id<Location>, event: &PaymentEvent) -> Id<Event> {
        let meta = self.meta_at(location);
        self.payment.apply(&meta, event);
        meta.event_id
    }

    fn kinds(&self) -> Vec<ConflictKind> {
        self.payment.conflicts().iter().map(|conflict| conflict.kind).collect()
    }

    fn outcome(&self) -> Option<Outcome> {
        self.payment.status().outcome()
    }
}

#[test]
fn a_card_payment_is_authorized_then_captured() {
    let mut script = Script::new();
    assert!(script.payment.info().is_none() && !script.payment.is_unresolved());
    let start = script.apply(&initiated(Tender::Card, usd(2500)));
    let info = script.payment.info().unwrap().clone();
    assert_eq!(
        (info.location, info.order, info.check, info.tender, info.amount, info.initiated_by),
        (location(), id(0xA), id(0xC1), Tender::Card, usd(2500), start)
    );
    assert_eq!(script.payment.id(), id(0xB1));
    assert!(script.payment.is_unresolved());

    script.apply(&authorized(usd(2500)));
    assert!(
        matches!(script.payment.status(), PaymentStatus::Authorized(a) if a.amount == usd(2500))
    );
    assert!(script.payment.is_unresolved());
    let mut capture = captured(usd(2500));
    capture.tip = Some(usd(500));
    script.apply(&PaymentEvent::Captured(capture.clone()));
    assert_eq!(script.payment.captured(), Some(&capture));
    assert!(!script.payment.is_unresolved());
    // A card customer pays the amount and the tip, and gets no change.
    assert_eq!((capture.received(), capture.change()), (Some(usd(3000)), None));
    assert_eq!(script.kinds(), []);
    assert!(script.payment.skipped().is_empty() && !script.payment.needs_update());
}

#[test]
fn a_cash_payment_records_the_cash_handed_over() {
    // CAD 16.52 in cash rounds to 16.50; the customer hands over 20.00 and gets 3.50 back.
    let mut script = Script::new();
    script.apply(&initiated(Tender::Cash, cad(1652)));
    let cash = in_cash(cad(1652), cad(2000), Some(cad(-2)));
    script.apply(&PaymentEvent::Captured(cash.clone()));
    assert_eq!(script.payment.captured(), Some(&cash));
    assert_eq!((cash.received(), cash.change()), (Some(cad(1650)), Some(cad(350))));
    // Keep the change: the tip takes it.
    let keep = PaymentCaptured { tip: Some(cad(350)), ..cash };
    assert_eq!((keep.received(), keep.change()), (Some(cad(2000)), Some(cad(0))));
}

#[test]
fn outcomes_follow_the_state_machine() {
    // In turn: straight to any outcome, or through an authorization.
    for (events, outcome) in [
        (vec![authorized(usd(2500))], Outcome::Authorized),
        (vec![PaymentEvent::Captured(captured(usd(2500)))], Outcome::Captured),
        (vec![PaymentEvent::Failed(ended("declined"))], Outcome::Failed),
        (vec![PaymentEvent::Voided(ended("cancelled"))], Outcome::Voided),
        (
            vec![authorized(usd(2500)), PaymentEvent::Captured(captured(usd(2000)))],
            Outcome::Captured,
        ),
        (vec![authorized(usd(2500)), PaymentEvent::Failed(ended("expired"))], Outcome::Failed),
        (vec![authorized(usd(2500)), PaymentEvent::Voided(ended("cancelled"))], Outcome::Voided),
    ] {
        let mut script = Script::initiated(Tender::Card);
        for event in &events {
            script.apply(event);
        }
        assert_eq!((script.outcome(), script.kinds()), (Some(outcome), vec![]), "{events:?}");
    }
}

#[test]
fn money_that_moved_stays_moved() {
    // Once captured, a payment stays captured: every later outcome is reported.
    let mut script = Script::initiated(Tender::Card);
    let capture = captured(usd(2500));
    script.apply(&PaymentEvent::Captured(capture.clone()));
    script.apply(&PaymentEvent::Voided(ended("cancelled")));
    script.apply(&PaymentEvent::Failed(ended("declined")));
    script.apply(&authorized(usd(2500)));
    script.apply(&PaymentEvent::Captured(captured(usd(100))));
    assert_eq!(script.payment.captured(), Some(&capture));
    let after = |outcome| ConflictKind::OutOfTurn { after: Outcome::Captured, outcome };
    assert_eq!(
        script.kinds(),
        [
            after(Outcome::Voided),
            after(Outcome::Failed),
            after(Outcome::Authorized),
            after(Outcome::Captured)
        ]
    );

    // A capture after a failure or a void applies: the money moved after all.
    let mut script = Script::initiated(Tender::Card);
    script.apply(&PaymentEvent::Failed(ended("declined")));
    script.apply(&PaymentEvent::Captured(captured(usd(2500))));
    assert_eq!(script.outcome(), Some(Outcome::Captured));
    assert_eq!(
        script.kinds(),
        [ConflictKind::OutOfTurn { after: Outcome::Failed, outcome: Outcome::Captured }]
    );

    // An authorization after a void holds the card again: the payment is unresolved.
    let mut script = Script::initiated(Tender::Card);
    script.apply(&PaymentEvent::Voided(ended("cancelled")));
    script.apply(&authorized(usd(2500)));
    assert!(script.payment.is_unresolved());
    assert_eq!(
        script.kinds(),
        [ConflictKind::OutOfTurn { after: Outcome::Voided, outcome: Outcome::Authorized }]
    );

    // No money moved either way: repeats change nothing, and aren't reported.
    let mut script = Script::initiated(Tender::Card);
    script.apply(&authorized(usd(2500)));
    script.apply(&authorized(usd(1000)));
    assert!(
        matches!(script.payment.status(), PaymentStatus::Authorized(a) if a.amount == usd(2500))
    );
    script.apply(&PaymentEvent::Failed(ended("declined")));
    script.apply(&PaymentEvent::Voided(ended("cancelled")));
    script.apply(&PaymentEvent::Failed(ended("again")));
    assert!(
        matches!(script.payment.status(), PaymentStatus::Failed(e) if e.reason.code.as_str() == "declined")
    );
    assert_eq!(script.kinds(), []);
}

#[test]
fn events_outside_the_payment_are_not_applied() {
    let mut script = Script::new();
    script.apply(&PaymentEvent::Captured(captured(usd(2500))));
    assert_eq!(
        (script.payment.info(), script.kinds()),
        (None, vec![ConflictKind::BeforeInitiation])
    );
    script.apply(&initiated(Tender::Card, usd(2500)));
    script.apply(&initiated(Tender::Cash, usd(100)));
    assert_eq!(script.payment.info().unwrap().tender, Tender::Card);
    script.apply_at(id(0x101), &PaymentEvent::Captured(captured(usd(2500))));
    script.apply(&PaymentEvent::Captured(captured(cad(2500))));
    script.apply(&authorized(cad(2500)));
    assert!(script.payment.is_unresolved());
    assert_eq!(
        script.kinds(),
        [
            ConflictKind::BeforeInitiation,
            ConflictKind::DuplicateInitiation,
            ConflictKind::WrongLocation,
            ConflictKind::CurrencyMismatch,
            ConflictKind::CurrencyMismatch,
        ]
    );
    let meta = script.meta_at(location());
    let newer = SchemaRef {
        name: SchemaName::new("payment.captured").unwrap(),
        version: 2.try_into().unwrap(),
    };
    script.payment.skip(&meta, &newer, &DecodeError::UnknownSchema);
    assert!(script.payment.needs_update());
}

/// A payment as a device sees it, and the commands it runs.
fn decide(script: &Script, command: PaymentCommand) -> Result<PaymentEvent, PaymentError> {
    script.payment.decide(location(), command)
}

#[test]
fn card_commands_follow_the_payments_life() {
    let script = Script::new();
    assert_eq!(
        decide(&script, PaymentCommand::Fail(ended("declined"))),
        Err(PaymentError::NotInitiated)
    );

    let mut script = Script::initiated(Tender::Card);
    let authorize = |amount| {
        PaymentCommand::Authorize(PaymentAuthorized { amount, reference: Some(reference("pi_1")) })
    };
    assert_eq!(
        script.payment.decide(id(0x101), authorize(usd(2500))),
        Err(PaymentError::WrongLocation)
    );
    assert_eq!(decide(&script, authorize(usd(2501))), Err(PaymentError::TooMuch));
    assert_eq!(decide(&script, authorize(cad(2500))), Err(PaymentError::WrongCurrency));
    assert_eq!(
        decide(&script, authorize(usd(0))),
        Err(PaymentError::Invalid(PayloadError::Invalid("amount")))
    );
    let partial = decide(&script, authorize(usd(2000))).unwrap();
    script.apply(&partial);
    assert_eq!(decide(&script, authorize(usd(2000))), Err(PaymentError::AlreadyAuthorized));
    // A capture is for no more than the authorization; a card has no cash details.
    assert_eq!(
        decide(&script, PaymentCommand::Capture(captured(usd(2001)))),
        Err(PaymentError::TooMuch)
    );
    let cash = in_cash(usd(2000), usd(2000), None);
    assert_eq!(decide(&script, PaymentCommand::Capture(cash)), Err(PaymentError::WrongTender));
    let capture = decide(&script, PaymentCommand::Capture(captured(usd(2000)))).unwrap();
    script.apply(&capture);
    assert_eq!(
        decide(&script, PaymentCommand::Void(ended("cancelled"))),
        Err(PaymentError::Resolved)
    );
    assert_eq!(script.kinds(), []);
}

#[test]
fn cash_commands_record_what_was_tendered() {
    let mut script = Script::initiated(Tender::Cash);
    let authorize =
        PaymentCommand::Authorize(PaymentAuthorized { amount: usd(2500), reference: None });
    assert_eq!(decide(&script, authorize), Err(PaymentError::WrongTender));
    assert_eq!(
        decide(
            &script,
            PaymentCommand::Capture(PaymentCaptured { reference: None, ..captured(usd(2500)) })
        ),
        Err(PaymentError::WrongTender)
    );
    let referenced =
        PaymentCaptured { reference: Some(reference("x")), ..in_cash(usd(2500), usd(3000), None) };
    assert_eq!(
        decide(&script, PaymentCommand::Capture(referenced)),
        Err(PaymentError::WrongTender)
    );
    let failed = PaymentEnded { reference: Some(reference("x")), ..ended("cancelled") };
    assert_eq!(decide(&script, PaymentCommand::Fail(failed)), Err(PaymentError::WrongTender));
    assert_eq!(
        decide(&script, PaymentCommand::Capture(in_cash(usd(2501), usd(3000), None))),
        Err(PaymentError::TooMuch)
    );
    // What was tendered must cover what is paid.
    assert_eq!(
        decide(&script, PaymentCommand::Capture(in_cash(usd(2500), usd(2000), None))),
        Err(PaymentError::Invalid(PayloadError::Invalid("tendered")))
    );
    let capture =
        decide(&script, PaymentCommand::Capture(in_cash(usd(2500), usd(3000), None))).unwrap();
    script.apply(&capture);
    assert_eq!(script.payment.captured().and_then(PaymentCaptured::change), Some(usd(500)));
}

#[test]
fn payload_rules_across_fields_are_enforced() {
    let decode = |event: &PaymentEvent| {
        let schema = event.schema().to_ref().unwrap();
        PaymentEvent::from_value(&schema, &event.to_value())
    };
    let invalid = |field| Err(DecodeError::Malformed(PayloadError::Invalid(field)));
    assert_eq!(decode(&initiated(Tender::Cash, usd(0))), invalid("amount"));
    assert_eq!(decode(&authorized(usd(-1))), invalid("amount"));
    assert_eq!(decode(&PaymentEvent::Captured(captured(usd(0)))), invalid("amount"));
    let tip =
        |tip| PaymentEvent::Captured(PaymentCaptured { tip: Some(tip), ..captured(usd(100)) });
    assert_eq!(decode(&tip(usd(0))), invalid("tip"));
    assert_eq!(decode(&tip(cad(100))), invalid("tip"));
    let cash = |tendered, rounding| PaymentEvent::Captured(in_cash(usd(100), tendered, rounding));
    assert_eq!(decode(&cash(usd(100), Some(usd(0)))), invalid("rounding"));
    assert_eq!(decode(&cash(usd(100), Some(cad(-2)))), invalid("rounding"));
    assert_eq!(decode(&cash(usd(99), None)), invalid("tendered"));
    assert_eq!(decode(&cash(cad(200), None)), invalid("tendered"));
    // Rounded up, the customer pays more; rounded down to nothing, nothing at all.
    assert_eq!(decode(&cash(usd(101), Some(usd(2)))), invalid("tendered"));
    assert!(decode(&cash(usd(102), Some(usd(2)))).is_ok());
    let tiny = PaymentEvent::Captured(in_cash(usd(2), usd(0), Some(usd(-2))));
    assert!(decode(&tiny).is_ok());
    let negative = PaymentEvent::Captured(in_cash(usd(2), usd(0), Some(usd(-3))));
    assert_eq!(decode(&negative), invalid("tendered"));
    // A rounding comes with what was tendered.
    let mut payload =
        cash(usd(100), Some(usd(2))).to_value().as_map().unwrap().clone().into_entries();
    payload.retain(|(key, _)| key.as_u64() != Some(4));
    let schema = PaymentEvent::Captured(captured(usd(1))).schema().to_ref().unwrap();
    let map = Value::Map(keel_events::cbor::Map::from_entries(payload).unwrap());
    assert_eq!(PaymentEvent::from_value(&schema, &map), invalid("rounding"));
}

#[test]
fn the_registry_lists_every_schema_once() {
    let mut names: Vec<(&str, u32)> =
        PaymentEvent::SCHEMAS.iter().map(|schema| (schema.name, schema.version)).collect();
    for schema in PaymentEvent::SCHEMAS {
        let reference = schema.to_ref().unwrap();
        assert!(reference.name.as_str().starts_with("payment."));
        assert!(schema.matches(&reference));
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), PaymentEvent::SCHEMAS.len());
    let unknown = SchemaRef {
        name: SchemaName::new("payment.refunded").unwrap(),
        version: 1.try_into().unwrap(),
    };
    assert_eq!(PaymentEvent::from_value(&unknown, &Value::Null), Err(DecodeError::UnknownSchema));
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The payment payloads, pinned forever: an example of each schema, and of a capture both by
/// card and in cash. Python's `cbor2` encoded each one itself from the documented key tables,
/// and confirmed it is canonical. If this test fails, a payload format changed, and stored
/// events would no longer decode.
#[test]
fn the_payload_formats_are_pinned() {
    let pinned = [
        (
            initiated(Tender::Card, usd(1089)),
            "a401500192f0c100007000800000000000000a02500192f0c10000700080000000000000c10301048219044163555344",
        ),
        (
            PaymentEvent::Authorized(PaymentAuthorized {
                amount: usd(1089),
                reference: Some(reference("pi_3Keel01")),
            }),
            "a2018219044163555344026a70695f334b65656c3031",
        ),
        (
            PaymentEvent::Captured(PaymentCaptured {
                amount: usd(1089),
                tip: Some(usd(200)),
                reference: Some(reference("pi_3Keel01")),
                cash: None,
            }),
            "a3018219044163555344028218c863555344036a70695f334b65656c3031",
        ),
        (
            PaymentEvent::Captured(in_cash(cad(1652), cad(2000), Some(cad(-2)))),
            "a301821906746343414404821907d06343414405822163434144",
        ),
        (
            PaymentEvent::Failed(PaymentEnded {
                reason: Reason {
                    code: ReasonCode::new("declined").unwrap(),
                    note: Some(Note::new("insufficient funds").unwrap()),
                },
                reference: Some(reference("pi_3Keel02")),
            }),
            "a301686465636c696e65640272696e73756666696369656e742066756e6473036a70695f334b65656c3032",
        ),
        (PaymentEvent::Voided(ended("cancelled")), "a1016963616e63656c6c6564"),
    ];
    let mut covered: Vec<&str> = Vec::new();
    for (event, hex) in pinned {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice(), "{}", schema.name);
        assert_eq!(PaymentEvent::decode(&schema, &payload), Ok(event));
        covered.push(event_name(&schema));
    }
    covered.dedup();
    let names: Vec<&str> = PaymentEvent::SCHEMAS.iter().map(|schema| schema.name).collect();
    assert_eq!(covered, names);
}

fn event_name(schema: &SchemaRef) -> &'static str {
    PaymentEvent::SCHEMAS.iter().find(|known| known.matches(schema)).unwrap().name
}
