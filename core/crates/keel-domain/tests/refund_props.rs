//! Property tests for refunds (ADR-0032, ADR-0035), against independent models:
//! - the fold, given any events in any order, leaves the refund as a model predicts: what it was
//!   started with, its completion, its failed attempts and its conflicts. The model reads the events as queries over
//!   their positions (the first start, the first completion that applies) rather than as the
//!   fold's state machine;
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
use keel_domain::codec::ReasonCode;
use keel_domain::order::Reason;
use keel_domain::payment::Payment;
use keel_domain::payment::Tender;
use keel_domain::refund::{
    ConflictKind, Refund, RefundCommand, RefundCompleted, RefundError, RefundEvent, RefundFailed,
    RefundInitiated,
};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Currency, Hlc, Id, Money};
use proptest::prelude::*;
use support::id;

fn location() -> Id<Location> {
    id(0x100)
}

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::USD)
}

fn eur(minor: i64) -> Money {
    Money::from_minor(minor, Currency::EUR)
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

/// A few amounts, mostly in dollars, so that events collide.
fn fold_amount() -> impl Strategy<Value = Money> {
    let minor = prop_oneof![Just(1_i64), Just(500), Just(708), 1_i64..1_000];
    (minor, prop::bool::weighted(0.85))
        .prop_map(|(minor, dollars)| if dollars { usd(minor) } else { eur(minor) })
}

/// A failed attempt, for one of a few reasons.
fn failure(n: u8) -> RefundFailed {
    let code = ["declined", "expired_card", "terminal_error"][usize::from(n % 3)];
    RefundFailed {
        reason: Reason { code: ReasonCode::new(code).unwrap(), note: None },
        reference: None,
    }
}

fn fold_event() -> impl Strategy<Value = RefundEvent> {
    let tender = prop::sample::select(vec![Tender::Cash, Tender::Card]);
    prop_oneof![
        2 => (tender, fold_amount(), any::<bool>()).prop_map(|(tender, amount, named)| {
            // A card refund names its payment, or, as version 1 could, none.
            let payment = (tender == Tender::Card && named).then(|| id::<Payment>(0xB1));
            RefundEvent::Initiated(RefundInitiated { order: id(0xA1), tender, amount, payment })
        }),
        2 => any::<u8>().prop_map(|n| RefundEvent::Failed(failure(n))),
        3 => (fold_amount(), prop::option::of(prop_oneof![-3_i64..=-1, 1_i64..=3])).prop_map(
            |(amount, rounding)| {
                let rounding = rounding.map(|minor| Money::from_minor(minor, amount.currency()));
                RefundEvent::Completed(RefundCompleted { amount, rounding })
            }
        ),
    ]
}

/// Events numbered in order, by device 1, or 2 when flagged, at the location, or another when
/// flagged.
fn numbered(events: Vec<(bool, bool, RefundEvent)>) -> Vec<(EventMeta, RefundEvent)> {
    (1..)
        .zip(events)
        .map(|(seq, (other_device, elsewhere, event))| {
            let device = if other_device { 2 } else { 1 };
            let at = if elsewhere { id(0x101) } else { location() };
            (meta_at(device, seq, at), event)
        })
        .collect()
}

/// What a refund was started with: its tender, amount and payment, by which device, in which
/// event.
type Started = (Tender, Money, Option<Id<Payment>>, Id<Device>, Id<Event>);

/// What the fold model predicts.
#[derive(Debug, PartialEq, Eq)]
struct Expected {
    initiated: Option<Started>,
    completed: Option<RefundCompleted>,
    /// The latest failure applied, while the refund is owed, and how many applied.
    failed: Option<RefundFailed>,
    failures: u32,
    owed: bool,
    conflicts: Vec<(Id<Event>, ConflictKind)>,
}

fn expected(events: &[(EventMeta, RefundEvent)]) -> Expected {
    let mut conflicts = Vec::new();
    let start = events.iter().position(|(_, event)| matches!(event, RefundEvent::Initiated(_)));
    let mut completed = None;
    let mut failed = None;
    let mut failures = 0;
    let started = start.map(|s| {
        let (meta, RefundEvent::Initiated(initiated)) = &events[s] else {
            panic!("the first start")
        };
        (meta, *initiated)
    });
    for (i, (meta, event)) in events.iter().enumerate() {
        let at = meta.event_id;
        if Some(i) == start {
            continue;
        }
        if matches!(event, RefundEvent::Initiated(_)) {
            // A start after the first.
            conflicts.push((at, ConflictKind::DuplicateInitiation));
            continue;
        }
        let Some((opened, initiated)) = started.filter(|_| start < Some(i)) else {
            conflicts.push((at, ConflictKind::BeforeInitiation));
            continue;
        };
        if meta.location != opened.location {
            conflicts.push((at, ConflictKind::WrongLocation));
            continue;
        }
        if meta.origin_device != opened.origin_device {
            conflicts.push((at, ConflictKind::NotInitiator { by: meta.origin_device }));
        }
        // A failure leaves the refund owed, unless it was paid back before.
        let RefundEvent::Completed(completion) = event else {
            if completed.is_some() {
                conflicts.push((at, ConflictKind::FailedAfterCompletion));
            } else if let RefundEvent::Failed(failure) = event {
                failed = Some(failure.clone());
                failures += 1;
            }
            continue;
        };
        if completed.is_some() {
            conflicts.push((at, ConflictKind::DuplicateCompletion));
            continue;
        }
        if completion.amount.currency() != initiated.amount.currency() {
            conflicts.push((at, ConflictKind::CurrencyMismatch));
            continue;
        }
        if completion.amount.minor() > initiated.amount.minor() {
            conflicts.push((at, ConflictKind::BeyondInitiated));
        }
        completed = Some(*completion);
    }
    Expected {
        initiated: started.map(|(meta, initiated)| {
            (
                initiated.tender,
                initiated.amount,
                initiated.payment,
                meta.origin_device,
                meta.event_id,
            )
        }),
        owed: started.is_some() && completed.is_none(),
        failed: failed.filter(|_| completed.is_none()),
        completed,
        failures,
        conflicts,
    }
}

fn folded(events: &[(EventMeta, RefundEvent)]) -> Refund {
    let mut refund = Refund::new(id(0xC1));
    for (meta, event) in events {
        // Only events a kernel could decode are ever folded.
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(&RefundEvent::decode(&schema, &payload).unwrap(), event);
        refund.apply(meta, event);
    }
    refund
}

fn actual(refund: &Refund) -> Expected {
    Expected {
        initiated: refund
            .info()
            .map(|info| (info.tender, info.amount, info.payment, info.device, info.initiated_by)),
        completed: refund.completed().copied(),
        failed: refund.failed().cloned(),
        failures: refund.failures(),
        owed: refund.is_owed(),
        conflicts: refund.conflicts().iter().map(|c| (c.event, c.kind)).collect(),
    }
}

// ---------------------------------------------------------------------------------------------
// The command rules, modelled.

#[derive(Clone, Debug)]
struct Step {
    /// 0 starts the refund, 1 completes it, 2 records a failed attempt.
    kind: u8,
    card: bool,
    amount: i64,
    rounding: i64,
    fault: u8,
}

fn any_step() -> impl Strategy<Value = Step> {
    let fault = prop_oneof![6 => Just(0_u8), 4 => 1_u8..=6];
    let kind = prop_oneof![2 => Just(0_u8), 3 => Just(1_u8), 2 => Just(2_u8)];
    (kind, any::<bool>(), -2_i64..1_000, -2_i64..=2, fault).prop_map(
        |(kind, card, amount, rounding, fault)| Step { kind, card, amount, rounding, fault },
    )
}

/// The command a step stands for, from which device, and where. Fault 1 is another device;
/// 2 another location; 3 euros; 4 an amount of zero or less; 5 a rounding of zero; 6 a start
/// whose payment doesn't fit its tender: a card's with none, a cash refund's with one.
fn command(step: &Step) -> (RefundCommand, u64, Id<Location>) {
    let money = |minor: i64| if step.fault == 3 { eur(minor) } else { usd(minor) };
    let amount =
        if step.fault == 4 { money(step.amount.min(0)) } else { money(step.amount.max(1)) };
    let command = match step.kind {
        0 => {
            let tender = if step.card { Tender::Card } else { Tender::Cash };
            let payment = (step.card != (step.fault == 6)).then(|| id::<Payment>(0xB1));
            RefundCommand::Initiate(RefundInitiated { order: id(0xA1), tender, amount, payment })
        }
        1 => {
            let rounding = (step.rounding != 0 || step.fault == 5).then(|| money(step.rounding));
            RefundCommand::Complete(RefundCompleted { amount, rounding })
        }
        _ => RefundCommand::Fail(failure(u8::try_from(step.amount.rem_euclid(3)).unwrap())),
    };
    let device = if step.fault == 1 { 2 } else { 1 };
    let at = if step.fault == 2 { id(0x101) } else { location() };
    (command, device, at)
}

/// The command model: a refund as the rules see it.
#[derive(Default)]
struct Model {
    /// What it was started for, by which tender and device, and where.
    started: Option<(Money, Tender, u64, Id<Location>)>,
    completed: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    AlreadyInitiated,
    NotInitiated,
    WrongLocation,
    NotInitiator,
    WrongCurrency,
    AlreadyCompleted,
    BeyondInitiated,
    WrongTender,
    Invalid,
}

fn refusal_of(error: &RefundError) -> Refusal {
    match error {
        RefundError::AlreadyInitiated => Refusal::AlreadyInitiated,
        RefundError::NotInitiated => Refusal::NotInitiated,
        RefundError::WrongLocation => Refusal::WrongLocation,
        RefundError::NotInitiator => Refusal::NotInitiator,
        RefundError::WrongCurrency => Refusal::WrongCurrency,
        RefundError::AlreadyCompleted => Refusal::AlreadyCompleted,
        RefundError::BeyondInitiated => Refusal::BeyondInitiated,
        RefundError::WrongTender => Refusal::WrongTender,
        RefundError::Invalid(_) => Refusal::Invalid,
        other => panic!("no command here should be refused with {other:?}"),
    }
}

impl Model {
    fn refused(&self, command: &RefundCommand, device: u64, at: Id<Location>) -> Option<Refusal> {
        if let RefundCommand::Initiate(initiated) = command {
            return if self.started.is_some() {
                Some(Refusal::AlreadyInitiated)
            } else if (initiated.tender == Tender::Card) != initiated.payment.is_some() {
                Some(Refusal::WrongTender)
            } else if !initiated.amount.is_positive() {
                Some(Refusal::Invalid)
            } else {
                None
            };
        }
        // An outcome: only the device that started it records one, there, while it is owed.
        let Some((amount, tender, by, place)) = self.started else {
            return Some(Refusal::NotInitiated);
        };
        if at != place {
            return Some(Refusal::WrongLocation);
        } else if device != by {
            return Some(Refusal::NotInitiator);
        } else if self.completed {
            return Some(Refusal::AlreadyCompleted);
        }
        match command {
            RefundCommand::Complete(completed) => {
                if completed.amount.currency() != amount.currency() {
                    Some(Refusal::WrongCurrency)
                } else if completed.amount.minor() > amount.minor() {
                    Some(Refusal::BeyondInitiated)
                } else if !completed.amount.is_positive()
                    || completed.rounding.is_some_and(Money::is_zero)
                {
                    Some(Refusal::Invalid)
                } else {
                    None
                }
            }
            // Cash is paid back as it is started: it never fails.
            _ => (tender != Tender::Card).then_some(Refusal::WrongTender),
        }
    }

    fn apply(&mut self, command: &RefundCommand, device: u64, at: Id<Location>) {
        match command {
            RefundCommand::Initiate(initiated) => {
                self.started = Some((initiated.amount, initiated.tender, device, at));
            }
            RefundCommand::Complete(_) => self.completed = true,
            RefundCommand::Fail(_) => {}
        }
    }
}

proptest! {
    /// The fold leaves any events as the model predicts.
    #[test]
    fn the_fold_matches_the_model(
        events in prop::collection::vec(
            (prop::bool::weighted(0.1), prop::bool::weighted(0.08), fold_event()),
            0..12,
        ),
    ) {
        let events = numbered(events);
        prop_assert_eq!(actual(&folded(&events)), expected(&events));
    }

    /// A device's commands are refused exactly as the model refuses them, and accepted ones
    /// apply with no conflict.
    #[test]
    fn commands_do_what_the_model_says(steps in prop::collection::vec(any_step(), 1..12)) {
        let mut refund = Refund::new(id(0xC1));
        let mut model = Model::default();
        for (seq, step) in (1..).zip(&steps) {
            let (command, device, at) = command(step);
            let decided = refund.decide(at, id(device), command.clone());
            prop_assert_eq!(decided.as_ref().err().map(refusal_of), model.refused(&command, device, at));
            if let Ok(event) = decided {
                let before = refund.conflicts().len();
                refund.apply(&meta_at(device, seq, at), &event);
                prop_assert_eq!(refund.conflicts().len(), before);
                model.apply(&command, device, at);
            }
        }
    }
}
