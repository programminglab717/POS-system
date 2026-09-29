//! Property tests for the payment aggregate, against independent models:
//! - the fold, given any events in any order, leaves the payment exactly as a model of the fold
//!   predicts, conflicts included. The model is written as queries over the events' positions
//!   (the first capture, and runs of authorizations and of failures or voids before it) rather
//!   than as a state machine, so it shares no structure with the fold;
//! - folding signed events takes only the aggregate's own stream;
//! - a device's commands, faulty ones included, and commands at the edges of the rules, are
//!   refused exactly when a model of the command rules refuses them, and for the same reason;
//!   accepted ones never cause a conflict, and have the effect the model predicts;
//! - devices acting concurrently on the same payment, on stale views, converge to what the fold
//!   model predicts.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::time::Duration;

use keel_domain::aggregate::{Aggregate, EventMeta, fold};
use keel_domain::codec::{Note, ProcessorRef, ReasonCode};
use keel_domain::order::Reason;
use keel_domain::payment::{
    CashTendered, ConflictKind, Outcome, Payment, PaymentAuthorized, PaymentCaptured,
    PaymentCommand, PaymentEnded, PaymentError, PaymentEvent, PaymentInfo, PaymentInitiated,
    PaymentStatus, Tender,
};
use keel_domain::schema::{DecodeError, DomainEvent};
use keel_events::envelope::{Actor, Device, Event, Location, StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_events::log::{EventDraft, LogConfig, LogHead, LogWriter};
use keel_types::{Currency, Hlc, Id, Money, SeededEntropy, Timestamp};
use proptest::prelude::*;
use support::{id, usd};

fn location() -> Id<Location> {
    id(0x100)
}

fn other_location() -> Id<Location> {
    id(0x101)
}

fn payment_id() -> Id<Payment> {
    id(0xB1)
}

fn eur() -> Currency {
    Currency::from_code("EUR").unwrap()
}

fn reason(n: u8) -> Reason {
    let code = ["declined", "cancelled", "expired", "timeout"][usize::from(n % 4)];
    let note = n.is_multiple_of(5).then(|| Note::new("at the terminal").unwrap());
    Reason { code: ReasonCode::new(code).unwrap(), note }
}

fn reference(n: u8) -> Option<ProcessorRef> {
    (!n.is_multiple_of(3)).then(|| ProcessorRef::new(&format!("pi_{n}")).unwrap())
}

/// Event metadata for event `seq` of device `device`, at `hlc`, recorded at `location`.
fn meta_at(device: u64, seq: u64, hlc: u64, location: Id<Location>) -> EventMeta {
    EventMeta {
        event_id: id((device << 24) + seq),
        location,
        origin_device: id::<Device>(device),
        origin_seq: seq.try_into().unwrap(),
        hlc: Hlc::new(hlc, 0).unwrap(),
        business_date: "2026-09-28".parse().unwrap(),
        actor: Actor::TeamMember(id(0x300)),
        approval: None,
    }
}

// ---------------------------------------------------------------------------------------------
// An independent model of the fold, as queries over the events' positions.

struct Expected {
    info: Option<PaymentInfo>,
    status: PaymentStatus,
    conflicts: Vec<(Id<Event>, ConflictKind)>,
}

/// The outcome an event records, if it records one.
fn outcome_of(event: &PaymentEvent) -> Option<Outcome> {
    match event {
        PaymentEvent::Initiated(_) => None,
        PaymentEvent::Authorized(_) => Some(Outcome::Authorized),
        PaymentEvent::Captured(_) => Some(Outcome::Captured),
        PaymentEvent::Failed(_) => Some(Outcome::Failed),
        PaymentEvent::Voided(_) => Some(Outcome::Voided),
    }
}

/// The status an event records.
fn status_of(event: &PaymentEvent) -> PaymentStatus {
    match event {
        PaymentEvent::Initiated(_) => PaymentStatus::Initiated,
        PaymentEvent::Authorized(authorized) => PaymentStatus::Authorized(authorized.clone()),
        PaymentEvent::Captured(captured) => PaymentStatus::Captured(captured.clone()),
        PaymentEvent::Failed(ended) => PaymentStatus::Failed(ended.clone()),
        PaymentEvent::Voided(ended) => PaymentStatus::Voided(ended.clone()),
    }
}

/// The money an event names, if any.
fn amount_of(event: &PaymentEvent) -> Option<Money> {
    match event {
        PaymentEvent::Initiated(initiated) => Some(initiated.amount),
        PaymentEvent::Authorized(authorized) => Some(authorized.amount),
        PaymentEvent::Captured(captured) => Some(captured.amount),
        PaymentEvent::Failed(_) | PaymentEvent::Voided(_) => None,
    }
}

/// What the model predicts for `events`, folded in the order given.
fn expected(events: &[(EventMeta, PaymentEvent)]) -> Expected {
    let at = |i: usize| events[i].0.event_id;
    let mut conflicts = Vec::new();
    let Some(start) =
        events.iter().position(|(_, event)| matches!(event, PaymentEvent::Initiated(_)))
    else {
        // Nothing applies to a payment that wasn't initiated.
        conflicts.extend((0..events.len()).map(|i| (at(i), ConflictKind::BeforeInitiation)));
        return Expected { info: None, status: PaymentStatus::Initiated, conflicts };
    };
    let (meta, PaymentEvent::Initiated(initiated)) = &events[start] else { unreachable!() };
    let info = PaymentInfo {
        location: meta.location,
        order: initiated.order,
        check: initiated.check,
        tender: initiated.tender,
        amount: initiated.amount,
        initiated_by: meta.event_id,
    };
    // The outcomes that apply: after the initiation, at its location, in its currency.
    let mut outcomes: Vec<(usize, Outcome)> = Vec::new();
    for (i, (meta, event)) in events.iter().enumerate() {
        let conflict = if i == start {
            continue;
        } else if matches!(event, PaymentEvent::Initiated(_)) {
            ConflictKind::DuplicateInitiation
        } else if i < start {
            ConflictKind::BeforeInitiation
        } else if meta.location != info.location {
            ConflictKind::WrongLocation
        } else if amount_of(event).is_some_and(|amount| amount.currency() != info.amount.currency())
        {
            ConflictKind::CurrencyMismatch
        } else {
            outcomes.push((i, outcome_of(event).unwrap()));
            continue;
        };
        conflicts.push((at(i), conflict));
    }
    // The first capture: the money moved, and nothing after it changes the payment.
    let capture = outcomes.iter().position(|&(_, outcome)| outcome == Outcome::Captured);
    let before = &outcomes[..capture.unwrap_or(outcomes.len())];
    // Before it, authorizations and ends (failures and voids) come in runs. Within a run the
    // first event holds; an authorization run after an end run holds the card again, which is
    // out of turn.
    let authorizes = |outcome: Outcome| outcome == Outcome::Authorized;
    let mut runs: Vec<&[(usize, Outcome)]> =
        before.chunk_by(|left, right| authorizes(left.1) == authorizes(right.1)).collect();
    for pair in runs.windows(2) {
        let (previous, run) = (pair[0][0], pair[1][0]);
        if authorizes(run.1) {
            let kind = ConflictKind::OutOfTurn { after: previous.1, outcome: run.1 };
            conflicts.push((at(run.0), kind));
        }
    }
    let settled = runs.pop().map(|run| run[0]);
    let status = match capture {
        Some(capture) => {
            let (i, _) = outcomes[capture];
            if let Some((_, after)) = settled.filter(|&(_, outcome)| !authorizes(outcome)) {
                conflicts
                    .push((at(i), ConflictKind::OutOfTurn { after, outcome: Outcome::Captured }));
            }
            for &(j, outcome) in &outcomes[capture + 1..] {
                let kind = ConflictKind::OutOfTurn { after: Outcome::Captured, outcome };
                conflicts.push((at(j), kind));
            }
            status_of(&events[i].1)
        }
        None => settled.map_or(PaymentStatus::Initiated, |(i, _)| status_of(&events[i].1)),
    };
    Expected { info: Some(info), status, conflicts }
}

fn sorted(conflicts: Vec<(Id<Event>, ConflictKind)>) -> Vec<(Id<Event>, String)> {
    let mut named: Vec<(Id<Event>, String)> =
        conflicts.into_iter().map(|(event, kind)| (event, format!("{kind:?}"))).collect();
    named.sort();
    named
}

/// Checks the payment folded from `events` against the fold model.
fn check(payment: &Payment, events: &[(EventMeta, PaymentEvent)]) -> Result<(), TestCaseError> {
    let expected = expected(events);
    prop_assert_eq!(payment.info(), expected.info.as_ref());
    prop_assert_eq!(payment.status(), &expected.status);
    prop_assert_eq!(
        payment.is_unresolved(),
        expected.info.is_some()
            && matches!(expected.status, PaymentStatus::Initiated | PaymentStatus::Authorized(_))
    );
    prop_assert_eq!(
        payment.captured(),
        match &expected.status {
            PaymentStatus::Captured(captured) => Some(captured),
            _ => None,
        }
    );
    let actual =
        payment.conflicts().iter().map(|conflict| (conflict.event, conflict.kind)).collect();
    prop_assert_eq!(sorted(actual), sorted(expected.conflicts));
    Ok(())
}

/// Folds `events` in the order given, after checking that each is one a kernel could decode.
fn folded(events: &[(EventMeta, PaymentEvent)]) -> Result<Payment, TestCaseError> {
    let mut payment = Payment::new(payment_id());
    for (meta, event) in events {
        let (schema, payload) = event.encode().unwrap();
        prop_assert_eq!(&PaymentEvent::decode(&schema, &payload).unwrap(), event);
        payment.apply(meta, event);
    }
    Ok(payment)
}

// ---------------------------------------------------------------------------------------------
// Events aimed at the fold's rules: one payment, a few amounts, and sometimes another currency
// or location.

fn fold_currency() -> impl Strategy<Value = Currency> {
    prop_oneof![6 => Just(usd()), 1 => Just(eur())]
}

fn fold_amount() -> impl Strategy<Value = Money> {
    (fold_currency(), 1_i64..=3).prop_map(|(currency, n)| Money::from_minor(n * 500, currency))
}

fn fold_initiated() -> impl Strategy<Value = PaymentInitiated> {
    (prop::sample::select(Tender::ALL), fold_amount(), 0_u64..2).prop_map(
        |(tender, amount, check)| PaymentInitiated {
            order: id(0xA),
            check: id(0xC1 + check),
            tender,
            amount,
        },
    )
}

fn fold_captured() -> impl Strategy<Value = PaymentCaptured> {
    (fold_amount(), prop::option::of(1_i64..300), any::<u8>(), any::<bool>()).prop_map(
        |(amount, tip, n, cash)| {
            let tip = tip.map(|tip| Money::from_minor(tip, amount.currency()));
            let due = tip.map_or(amount, |tip| amount.checked_add(tip).unwrap());
            let cash = cash.then(|| CashTendered {
                tendered: due.checked_add(Money::from_minor(i64::from(n), due.currency())).unwrap(),
                rounding: None,
            });
            let reference = if cash.is_some() { None } else { reference(n) };
            PaymentCaptured { amount, tip, reference, cash }
        },
    )
}

fn fold_ended() -> impl Strategy<Value = PaymentEnded> {
    any::<u8>().prop_map(|n| PaymentEnded { reason: reason(n), reference: reference(n) })
}

fn any_fold_event() -> impl Strategy<Value = (bool, PaymentEvent)> {
    let event = prop_oneof![
        1 => fold_initiated().prop_map(PaymentEvent::Initiated),
        3 => (fold_amount(), any::<u8>()).prop_map(|(amount, n)| {
            PaymentEvent::Authorized(PaymentAuthorized { amount, reference: reference(n) })
        }),
        3 => fold_captured().prop_map(PaymentEvent::Captured),
        2 => fold_ended().prop_map(PaymentEvent::Failed),
        2 => fold_ended().prop_map(PaymentEvent::Voided),
    ];
    (prop::bool::weighted(0.08), event)
}

/// Events numbered in the order given, recorded by one device at `location()`, or at another
/// location when flagged.
fn numbered(events: Vec<(bool, PaymentEvent)>) -> Vec<(EventMeta, PaymentEvent)> {
    (1..)
        .zip(events)
        .map(|(seq, (elsewhere, event))| {
            let at = if elsewhere { other_location() } else { location() };
            (meta_at(1, seq, 1_000 + seq, at), event)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// A model of one device's commands.

/// A seed for one command. A fault makes it one the device must refuse, or might.
#[derive(Clone, Debug)]
struct Step {
    kind: u8,
    amount: u8,
    fault: u8,
}

fn any_step() -> impl Strategy<Value = Step> {
    let fault = prop_oneof![6 => Just(0_u8), 4 => 1_u8..=7];
    (any::<u8>(), any::<u8>(), fault).prop_map(|(kind, amount, fault)| Step { kind, amount, fault })
}

/// An amount near `limit`: a little under it, at it, or, with fault 1, over it; fault 2 puts it
/// in euros, and fault 3 makes it zero.
fn near(limit: Money, step: &Step) -> Money {
    let minor = match step.fault {
        1 => limit.minor() + 1 + i64::from(step.amount % 3),
        3 => 0,
        _ => limit.minor() - i64::from(step.amount % 3).min(limit.minor() - 1),
    };
    let currency = if step.fault == 2 { eur() } else { limit.currency() };
    Money::from_minor(minor, currency)
}

/// The command a step stands for, on a payment initiated with `info`, and the location it comes
/// from. Fault 4 gives the details of the other tender; fault 5 puts a processor reference on a
/// cash payment's outcome; fault 6 tenders too little cash, or a cash rounding of zero; fault 7
/// comes from another location.
fn command(
    info: &PaymentInfo,
    status: &PaymentStatus,
    step: &Step,
) -> (PaymentCommand, Id<Location>) {
    let cash = (info.tender == Tender::Cash) != (step.fault == 4);
    let reference = if cash && step.fault != 5 { None } else { reference(step.amount | 1) };
    let limit = match status {
        PaymentStatus::Authorized(authorized) => authorized.amount,
        _ => info.amount,
    };
    let command = match step.kind % 4 {
        0 => PaymentCommand::Authorize(PaymentAuthorized {
            amount: near(info.amount, step),
            reference,
        }),
        1 => {
            let amount = near(limit, step);
            let tip =
                step.amount.is_multiple_of(4).then(|| Money::from_minor(100, amount.currency()));
            let cash = cash.then(|| {
                let due = tip.map_or(amount, |tip| amount.checked_add(tip).unwrap());
                if step.fault == 6 {
                    let short = due.checked_sub(Money::from_minor(1, due.currency())).unwrap();
                    let rounding =
                        step.amount.is_multiple_of(2).then(|| Money::from_minor(0, due.currency()));
                    CashTendered {
                        tendered: if rounding.is_some() { due } else { short },
                        rounding,
                    }
                } else {
                    let rounding = step
                        .amount
                        .is_multiple_of(3)
                        .then(|| Money::from_minor(-2, due.currency()));
                    let paid = rounding.map_or(due, |rounding| due.checked_add(rounding).unwrap());
                    let extra = Money::from_minor(i64::from(step.amount) * 10, due.currency());
                    CashTendered { tendered: paid.checked_add(extra).unwrap(), rounding }
                }
            });
            PaymentCommand::Capture(PaymentCaptured { amount, tip, reference, cash })
        }
        2 => PaymentCommand::Fail(PaymentEnded { reason: reason(step.amount), reference }),
        _ => PaymentCommand::Void(PaymentEnded { reason: reason(step.amount), reference }),
    };
    let from = if step.fault == 7 { other_location() } else { location() };
    (command, from)
}

/// Whether a capture's details satisfy the payload's rules, by the model: amounts in one
/// currency, the amount and any tip more than zero, a rounding never zero, and cash that covers
/// what is paid, which is zero or more.
fn capture_valid(captured: &PaymentCaptured) -> bool {
    let currency = captured.amount.currency();
    let tip = captured.tip.map_or(0, Money::minor);
    let tip_valid = captured.tip.is_none_or(|tip| tip.currency() == currency && tip.minor() > 0);
    let cash_valid = captured.cash.is_none_or(|cash| {
        let rounding = cash.rounding.map_or(0, Money::minor);
        let paid = i128::from(captured.amount.minor()) + i128::from(tip) + i128::from(rounding);
        cash.rounding.is_none_or(|r| r.currency() == currency && r.minor() != 0)
            && cash.tendered.currency() == currency
            && paid >= 0
            && i128::from(cash.tendered.minor()) >= paid
    });
    captured.amount.minor() > 0 && tip_valid && cash_valid
}

/// Why a command is refused, without the details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    WrongLocation,
    Resolved,
    WrongTender,
    AlreadyAuthorized,
    WrongCurrency,
    TooMuch,
    /// The event wouldn't satisfy its schema, such as an amount of zero.
    Invalid,
}

fn refusal_of(error: &PaymentError) -> Refusal {
    match error {
        PaymentError::WrongLocation => Refusal::WrongLocation,
        PaymentError::Resolved => Refusal::Resolved,
        PaymentError::WrongTender => Refusal::WrongTender,
        PaymentError::AlreadyAuthorized => Refusal::AlreadyAuthorized,
        PaymentError::WrongCurrency => Refusal::WrongCurrency,
        PaymentError::TooMuch => Refusal::TooMuch,
        PaymentError::Invalid(_) => Refusal::Invalid,
        other => panic!("no command here should be refused with {other:?}"),
    }
}

/// Why the model refuses `command` from `from` on a payment initiated with `info` whose status
/// is `status`: the first rule it breaks, taking the rules in this order: the device's
/// location; an unresolved payment; the tender; one authorization; the amount's currency, then
/// its limit; then the payload's own rules. `None` if the command is allowed.
fn refusal(
    info: &PaymentInfo,
    status: &PaymentStatus,
    command: &PaymentCommand,
    from: Id<Location>,
) -> Option<Refusal> {
    if from != info.location {
        return Some(Refusal::WrongLocation);
    }
    let authorized = match status {
        PaymentStatus::Initiated => None,
        PaymentStatus::Authorized(authorized) => Some(authorized.amount),
        _ => return Some(Refusal::Resolved),
    };
    let cash = info.tender == Tender::Cash;
    // An amount in the payment's currency, and no more than `limit`.
    let within = |amount: Money, limit: Money| {
        if amount.currency() != limit.currency() {
            Some(Refusal::WrongCurrency)
        } else if amount.minor() > limit.minor() {
            Some(Refusal::TooMuch)
        } else {
            None
        }
    };
    match command {
        PaymentCommand::Authorize(authorization) => {
            if cash {
                Some(Refusal::WrongTender)
            } else if authorized.is_some() {
                Some(Refusal::AlreadyAuthorized)
            } else {
                within(authorization.amount, info.amount)
                    .or((authorization.amount.minor() <= 0).then_some(Refusal::Invalid))
            }
        }
        PaymentCommand::Capture(captured) => {
            if cash != captured.cash.is_some() || cash && captured.reference.is_some() {
                Some(Refusal::WrongTender)
            } else {
                within(captured.amount, authorized.unwrap_or(info.amount))
                    .or((!capture_valid(captured)).then_some(Refusal::Invalid))
            }
        }
        PaymentCommand::Fail(ended) | PaymentCommand::Void(ended) => {
            (cash && ended.reference.is_some()).then_some(Refusal::WrongTender)
        }
    }
}

/// Commands at the edges of the rules, for a payment initiated with `info` whose status is
/// `status`: authorizations and captures for what was asked, for the authorization, a unit over
/// either, nothing, or in another currency, each with the details of either tender; and
/// failures and voids with and without a processor reference.
fn edges(info: &PaymentInfo, status: &PaymentStatus) -> Vec<PaymentCommand> {
    let asked = info.amount;
    let limit = match status {
        PaymentStatus::Authorized(authorized) => authorized.amount,
        _ => asked,
    };
    let amounts = [
        asked,
        Money::from_minor(asked.minor() + 1, asked.currency()),
        limit,
        Money::from_minor(limit.minor() + 1, limit.currency()),
        Money::from_minor(0, asked.currency()),
        Money::from_minor(limit.minor(), eur()),
    ];
    let mut commands = Vec::new();
    for amount in amounts {
        commands.push(PaymentCommand::Authorize(PaymentAuthorized { amount, reference: None }));
        for cash in [None, Some(CashTendered { tendered: amount, rounding: None })] {
            let captured = PaymentCaptured { amount, tip: None, reference: None, cash };
            commands.push(PaymentCommand::Capture(captured));
        }
    }
    for reference in [None, reference(1)] {
        let ended = PaymentEnded { reason: reason(0), reference };
        commands.push(PaymentCommand::Fail(ended.clone()));
        commands.push(PaymentCommand::Void(ended));
    }
    commands
}

/// Checks the edge commands on `payment`, as the device sees it, against the model, without
/// recording them.
fn check_edges(payment: &Payment, status: &PaymentStatus) -> Result<(), TestCaseError> {
    let info = payment.info().unwrap();
    for edge in edges(info, status) {
        let decided = payment.decide(location(), edge.clone());
        prop_assert_eq!(
            decided.err().map(|error| refusal_of(&error)),
            refusal(info, status, &edge, location()),
            "{:?} when {:?}",
            edge,
            status
        );
    }
    Ok(())
}

/// The status a command records.
fn recorded(command: &PaymentCommand) -> PaymentStatus {
    match command {
        PaymentCommand::Authorize(authorized) => PaymentStatus::Authorized(authorized.clone()),
        PaymentCommand::Capture(captured) => PaymentStatus::Captured(captured.clone()),
        PaymentCommand::Fail(ended) => PaymentStatus::Failed(ended.clone()),
        PaymentCommand::Void(ended) => PaymentStatus::Voided(ended.clone()),
    }
}

/// A device working on its own view of a payment.
struct Working {
    device: u64,
    view: Payment,
    seq: u64,
    hlc: u64,
    log: Vec<(EventMeta, PaymentEvent)>,
}

impl Working {
    fn new(device: u64, view: Payment, hlc: u64) -> Working {
        Working { device, view, seq: 0, hlc, log: Vec::new() }
    }

    fn record(&mut self, event: PaymentEvent, from: Id<Location>) {
        self.seq += 1;
        self.hlc += 1;
        let meta = meta_at(self.device, self.seq, self.hlc, from);
        self.view.apply(&meta, &event);
        self.log.push((meta, event));
    }

    /// Runs a step; returns whether its command was accepted.
    fn step(&mut self, step: &Step) -> bool {
        let Some(info) = self.view.info().cloned() else { return false };
        let (command, from) = command(&info, self.view.status(), step);
        let Ok(event) = self.view.decide(from, command) else { return false };
        self.record(event, from);
        true
    }
}

/// Folds events in canonical order: by HLC, then device, then position in the device's log.
fn replica(
    mut events: Vec<(EventMeta, PaymentEvent)>,
) -> (Payment, Vec<(EventMeta, PaymentEvent)>) {
    events.sort_by(|(a, _), (b, _)| {
        (a.hlc, a.origin_device, a.origin_seq).cmp(&(b.hlc, b.origin_device, b.origin_seq))
    });
    let mut payment = Payment::new(payment_id());
    for (meta, event) in &events {
        payment.apply(meta, event);
    }
    (payment, events)
}

/// A payment started by `device` for `amount`, by `tender`.
fn started(tender: Tender, amount: Money) -> Working {
    let mut device = Working::new(1, Payment::new(payment_id()), 1_000);
    let initiated = PaymentInitiated { order: id(0xA), check: id(0xC1), tender, amount };
    device.record(PaymentEvent::Initiated(initiated), location());
    device
}

/// Signs events in one device's log, each for its stream: a payment's identifier, and the stream
/// kind.
fn signed(events: &[(Id<Payment>, &str, PaymentEvent)]) -> Vec<SignedEvent> {
    let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[5; 32]).unwrap();
    let config = LogConfig {
        device: id(0xD),
        location: location(),
        head: LogHead::EMPTY,
        latest_hlc: Hlc::ZERO,
        max_forward_drift: Duration::from_secs(60),
    };
    let mut writer = LogWriter::new(config, signer, SeededEntropy::new(1));
    let now = Timestamp::from_millis(1_790_600_000_000).unwrap();
    events
        .iter()
        .map(|(stream, kind, event)| {
            let (schema, payload) = event.encode().unwrap();
            let draft = EventDraft {
                stream: StreamRef { kind: StreamKind::new(kind).unwrap(), id: stream.cast() },
                schema,
                business_date: "2026-09-28".parse().unwrap(),
                actor: Actor::TeamMember(id(0x300)),
                approval: None,
                causation: None,
                correlation: None,
                payload,
            };
            writer.prepare(draft, now).unwrap().commit()
        })
        .collect()
}

proptest! {
    /// Any events, in any order, fold into exactly the payment the fold model predicts. The
    /// events come from the wide generators, so any value the payloads allow takes part.
    #[test]
    fn the_fold_matches_the_model_for_any_events(
        events in prop::collection::vec(support::any_payment_event(), 0..24),
    ) {
        let events = numbered(events.into_iter().map(|event| (false, event)).collect());
        let payment = folded(&events)?;
        check(&payment, &events)?;
    }

    /// The same for events aimed at the state machine: one payment, mostly initiated first,
    /// then outcomes in any order, some from another location or in another currency.
    #[test]
    fn the_fold_matches_the_model_on_colliding_events(
        before in prop::collection::vec(any_fold_event(), 0..3),
        initiation in prop::option::weighted(0.9, (prop::bool::weighted(0.05), fold_initiated())),
        after in prop::collection::vec(any_fold_event(), 0..12),
    ) {
        let initiation = initiation.map(|(elsewhere, initiated)| (elsewhere, PaymentEvent::Initiated(initiated)));
        let events = numbered(before.into_iter().chain(initiation).chain(after).collect());
        let payment = folded(&events)?;
        check(&payment, &events)?;
    }

    /// Signed events fold into their own aggregate only: another payment's events, and an
    /// order's with the same identifier, are skipped.
    #[test]
    fn folds_take_only_their_own_streams_events(
        events in prop::collection::vec((0_u8..4, any_fold_event()), 0..16),
    ) {
        let events: Vec<(Id<Payment>, &str, PaymentEvent)> = events
            .into_iter()
            .map(|(stream, (_, event))| match stream {
                2 => (id(0xB2), "payment", event),
                3 => (payment_id(), "order", event),
                _ => (payment_id(), "payment", event),
            })
            .collect();
        let mut payment = Payment::new(payment_id());
        let mut own = Payment::new(payment_id());
        let mut others = Vec::new();
        for (signed, (stream, kind, event)) in signed(&events).iter().zip(&events) {
            fold(&mut payment, signed);
            if *stream == payment_id() && *kind == "payment" {
                own.apply(&EventMeta::of(signed.body()), event);
            } else {
                others.push(signed.body().event_id);
            }
        }
        prop_assert_eq!(payment.info(), own.info());
        prop_assert_eq!(payment.status(), own.status());
        prop_assert_eq!(payment.conflicts(), own.conflicts());
        let skipped: Vec<Id<Event>> = payment.skipped().iter().map(|skipped| skipped.event).collect();
        prop_assert_eq!(skipped, others);
        prop_assert!(payment.skipped().iter().all(|skipped| skipped.reason == DecodeError::WrongStream));
    }

    /// One device's commands, faulty ones included, are refused exactly when the model refuses
    /// them, and for the same reason; accepted ones never cause a conflict, and record the
    /// status the model predicts. At every state, commands at the edges of the rules are
    /// checked the same way, without being recorded.
    #[test]
    fn commands_do_what_the_model_says(
        tender in prop::sample::select(Tender::ALL),
        amount in 1_i64..5_000,
        steps in prop::collection::vec(any_step(), 1..8),
    ) {
        let mut device = started(tender, Money::from_minor(amount, usd()));
        let info = device.view.info().unwrap().clone();
        let mut status = PaymentStatus::Initiated;
        for step in &steps {
            check_edges(&device.view, &status)?;
            let (command, from) = command(&info, &status, step);
            let decided = device.view.decide(from, command.clone());
            let refused = decided.as_ref().err().map(refusal_of);
            prop_assert_eq!(refused, refusal(&info, &status, &command, from), "{:?} from {:?}", command, from);
            if let Ok(event) = decided {
                device.record(event, from);
                status = recorded(&command);
            }
            prop_assert!(device.view.conflicts().is_empty());
            prop_assert_eq!(device.view.status(), &status);
        }
        check_edges(&device.view, &status)?;
    }

    /// Devices acting at once on the same payment, from stale views: once their events are
    /// merged in canonical order, the payment is exactly what the fold model predicts.
    #[test]
    fn concurrent_devices_converge_as_the_model_says(
        tender in prop::sample::select(Tender::ALL),
        prefix in prop::collection::vec(any_step(), 0..3),
        devices in prop::collection::vec(prop::collection::vec(any_step(), 0..4), 2..=3),
    ) {
        let mut first = started(tender, Money::from_minor(1_500, usd()));
        for step in &prefix {
            first.step(step);
        }
        let mut events = first.log.clone();
        for (number, steps) in (2..).zip(&devices) {
            let mut device = Working::new(number, first.view.clone(), first.hlc);
            for step in steps {
                device.step(step);
            }
            events.extend(device.log);
        }
        let (payment, events) = replica(events);
        check(&payment, &events)?;
    }
}
