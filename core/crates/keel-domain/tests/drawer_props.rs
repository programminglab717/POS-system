//! Property tests for the drawer session (ADR-0031), against independent models:
//! - the fold, given any events in any order, leaves the session as a model predicts: its float,
//!   what its cash came to, its payments, its no-sales, its close and its conflicts. The model
//!   reads the events as queries over their positions (the first opening, the first close that
//!   applies, the payments counted before each) rather than as the fold's state machine;
//! - a device's commands, faulty ones included, are refused exactly when a model of the rules
//!   refuses them, for the same reason, and accepted ones apply with no conflict, leaving the
//!   drawer expecting what the model expects.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use std::collections::BTreeSet;

use keel_domain::aggregate::{Aggregate, EventMeta};
use keel_domain::codec::ReasonCode;
use keel_domain::drawer::{
    CashMoved, CashTaken, ConflictKind, Drawer, DrawerClosed, DrawerCommand, DrawerError,
    DrawerEvent, Movement, Sums,
};
use keel_domain::order::Reason;
use keel_domain::payment::Payment;
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

fn reason() -> Reason {
    Reason { code: ReasonCode::new("supplier").unwrap(), note: None }
}

fn meta_at(device: u64, seq: u64, location: Id<Location>) -> EventMeta {
    EventMeta {
        event_id: id((device << 24) + seq),
        location,
        origin_device: id::<Device>(device),
        origin_seq: seq.try_into().unwrap(),
        hlc: Hlc::new(1_000 + seq, 0).unwrap(),
        business_date: "2026-11-12".parse().unwrap(),
        actor: Actor::TeamMember(id(0x501)),
        approval: None,
    }
}

/// A few amounts, mostly in dollars: few payments and few amounts, so events collide.
fn fold_amount() -> impl Strategy<Value = Money> {
    let minor = prop_oneof![Just(1_i64), Just(250), Just(1_000), 1_i64..5_000];
    (minor, prop::bool::weighted(0.85)).prop_map(
        |(minor, dollars)| {
            if dollars { usd(minor) } else { eur(minor) }
        },
    )
}

fn fold_event() -> impl Strategy<Value = DrawerEvent> {
    let kind = prop::sample::select(vec![Movement::PaidIn, Movement::PaidOut, Movement::Drop]);
    prop_oneof![
        2 => fold_amount().prop_map(|float| DrawerEvent::Opened { float }),
        6 => ((1_u64..=4), fold_amount())
            .prop_map(|(n, amount)| DrawerEvent::CashTaken(CashTaken { payment: id(0xB0 + n), amount })),
        4 => (kind, fold_amount())
            .prop_map(|(kind, amount)| DrawerEvent::CashMoved(CashMoved { kind, amount, reason: reason() })),
        1 => Just(DrawerEvent::NoSale { reason: reason() }),
        2 => (fold_amount(), fold_amount())
            .prop_map(|(counted, expected)| DrawerEvent::Closed(DrawerClosed { counted, expected })),
    ]
}

/// Events numbered in order, by device 1, or 2 when flagged, at the location, or another when
/// flagged.
fn numbered(events: Vec<(bool, bool, DrawerEvent)>) -> Vec<(EventMeta, DrawerEvent)> {
    (1..)
        .zip(events)
        .map(|(seq, (other_device, elsewhere, event))| {
            let device = if other_device { 2 } else { 1 };
            let at = if elsewhere { id(0x101) } else { location() };
            (meta_at(device, seq, at), event)
        })
        .collect()
}

/// What the fold model predicts.
#[derive(Debug, PartialEq, Eq)]
struct Expected {
    float: Option<Money>,
    sums: Option<Sums>,
    payments: BTreeSet<Id<Payment>>,
    no_sales: u64,
    closed: Option<DrawerClosed>,
    conflicts: Vec<(Id<Event>, ConflictKind)>,
}

fn expected(events: &[(EventMeta, DrawerEvent)]) -> Expected {
    let mut conflicts = Vec::new();
    let opening = events.iter().position(|(_, event)| matches!(event, DrawerEvent::Opened { .. }));
    let Some(o) = opening else {
        // Nothing applies to a session never opened.
        let conflicts =
            events.iter().map(|(meta, _)| (meta.event_id, ConflictKind::BeforeOpening)).collect();
        return Expected {
            float: None,
            sums: None,
            payments: BTreeSet::new(),
            no_sales: 0,
            closed: None,
            conflicts,
        };
    };
    let (opened, DrawerEvent::Opened { float }) = &events[o] else { panic!("the first opening") };
    let (place, opener, currency) = (opened.location, opened.origin_device, float.currency());
    let zero = Money::zero(currency);
    let mut sums = Sums { taken: zero, paid_in: zero, paid_out: zero, dropped: zero };
    let (mut payments, mut no_sales, mut closed) = (BTreeSet::new(), 0, None);
    for (i, (meta, event)) in events.iter().enumerate() {
        let at = meta.event_id;
        if i == o {
            continue;
        }
        if matches!(event, DrawerEvent::Opened { .. }) {
            // None comes before the first.
            conflicts.push((at, ConflictKind::DuplicateOpening));
            continue;
        }
        if i < o {
            conflicts.push((at, ConflictKind::BeforeOpening));
            continue;
        }
        if meta.location != place {
            conflicts.push((at, ConflictKind::WrongLocation));
            continue;
        }
        if meta.origin_device != opener {
            conflicts.push((at, ConflictKind::NotOpener { by: meta.origin_device }));
        }
        let was_closed = closed.is_some();
        // What, if anything, keeps the event from applying.
        let refused = match event {
            DrawerEvent::CashTaken(taken) if payments.contains(&taken.payment) => {
                Some(ConflictKind::DuplicatePayment(taken.payment))
            }
            DrawerEvent::CashTaken(taken) if taken.amount.currency() != currency => {
                Some(ConflictKind::CurrencyMismatch)
            }
            DrawerEvent::CashMoved(moved) if moved.amount.currency() != currency => {
                Some(ConflictKind::CurrencyMismatch)
            }
            DrawerEvent::Closed(_) if was_closed => Some(ConflictKind::DuplicateClose),
            DrawerEvent::Closed(close) if close.counted.currency() != currency => {
                Some(ConflictKind::CurrencyMismatch)
            }
            _ => None,
        };
        if let Some(kind) = refused {
            conflicts.push((at, kind));
            continue;
        }
        match event {
            DrawerEvent::CashTaken(taken) => {
                sums.taken = sums.taken.checked_add(taken.amount).unwrap();
                payments.insert(taken.payment);
            }
            DrawerEvent::CashMoved(moved) => {
                let sum = match moved.kind {
                    Movement::PaidIn => &mut sums.paid_in,
                    Movement::PaidOut => &mut sums.paid_out,
                    Movement::Drop => &mut sums.dropped,
                };
                *sum = sum.checked_add(moved.amount).unwrap();
            }
            DrawerEvent::NoSale { .. } => no_sales += 1,
            DrawerEvent::Closed(close) => closed = Some(*close),
            DrawerEvent::Opened { .. } => panic!("handled above"),
        }
        if was_closed {
            conflicts.push((at, ConflictKind::AfterClose));
        }
    }
    Expected { float: Some(*float), sums: Some(sums), payments, no_sales, closed, conflicts }
}

fn folded(events: &[(EventMeta, DrawerEvent)]) -> Drawer {
    let mut drawer = Drawer::new(id(0xDA));
    for (meta, event) in events {
        // Only events a kernel could decode are ever folded.
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(&DrawerEvent::decode(&schema, &payload).unwrap(), event);
        drawer.apply(meta, event);
    }
    drawer
}

fn actual(drawer: &Drawer) -> Expected {
    Expected {
        float: drawer.info().map(|info| info.float),
        sums: drawer.sums().copied(),
        payments: drawer.payments().clone(),
        no_sales: drawer.no_sales(),
        closed: drawer.closed().copied(),
        conflicts: drawer.conflicts().iter().map(|c| (c.event, c.kind)).collect(),
    }
}

/// Only events whose payloads are valid: a kernel decodes no others.
fn valid(event: &DrawerEvent) -> bool {
    match event {
        DrawerEvent::Closed(close) => close.counted.currency() == close.expected.currency(),
        _ => true,
    }
}

// ---------------------------------------------------------------------------------------------
// The command rules, modelled.

#[derive(Clone, Debug)]
struct Step {
    kind: u8,
    amount: i64,
    payment: u64,
    fault: u8,
}

fn any_step() -> impl Strategy<Value = Step> {
    let fault = prop_oneof![7 => Just(0_u8), 3 => 1_u8..=5];
    (0_u8..5, -2_i64..3_000, 1_u64..=5, fault).prop_map(|(kind, amount, payment, fault)| Step {
        kind,
        amount,
        payment,
        fault,
    })
}

/// The command a step stands for, from which device, and where. Fault 1 is another device;
/// 2 another location; 3 euros; 4 an amount of zero or less; 5 none.
fn command(step: &Step) -> (DrawerCommand, u64, Id<Location>) {
    let money = |minor: i64| if step.fault == 3 { eur(minor) } else { usd(minor) };
    let amount =
        if step.fault == 4 { money(step.amount.min(0)) } else { money(step.amount.max(1)) };
    let command = match step.kind {
        0 => DrawerCommand::Open { float: amount, currency: Currency::USD },
        1 => DrawerCommand::TakeCash(CashTaken { payment: id(0xB0 + step.payment), amount }),
        2 => {
            let kind = [Movement::PaidIn, Movement::PaidOut, Movement::Drop]
                [usize::try_from(step.payment % 3).unwrap()];
            DrawerCommand::MoveCash(CashMoved { kind, amount, reason: reason() })
        }
        3 => DrawerCommand::NoSale(reason()),
        _ => DrawerCommand::Close { counted: if step.fault == 4 { money(-1) } else { amount } },
    };
    let device = if step.fault == 1 { 2 } else { 1 };
    let at = if step.fault == 2 { id(0x101) } else { location() };
    (command, device, at)
}

/// The command model: a drawer as the rules see it.
#[derive(Default)]
struct Model {
    float: Option<Money>,
    /// The device that opened it, and where.
    opener: u64,
    place: Option<Id<Location>>,
    expected: i64,
    payments: BTreeSet<Id<Payment>>,
    closed: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    AlreadyOpened,
    NotOpened,
    Closed,
    WrongLocation,
    NotOpener,
    WrongCurrency,
    AlreadyTaken,
    Invalid,
}

fn refusal_of(error: &DrawerError) -> Refusal {
    match error {
        DrawerError::AlreadyOpened => Refusal::AlreadyOpened,
        DrawerError::NotOpened => Refusal::NotOpened,
        DrawerError::Closed => Refusal::Closed,
        DrawerError::WrongLocation => Refusal::WrongLocation,
        DrawerError::NotOpener => Refusal::NotOpener,
        DrawerError::WrongCurrency => Refusal::WrongCurrency,
        DrawerError::AlreadyTaken => Refusal::AlreadyTaken,
        DrawerError::Invalid(_) => Refusal::Invalid,
        other => panic!("no command here should be refused with {other:?}"),
    }
}

impl Model {
    fn refused(&self, command: &DrawerCommand, device: u64, at: Id<Location>) -> Option<Refusal> {
        let dollars = |amount: &Money| amount.currency() == Currency::USD;
        if let DrawerCommand::Open { float, .. } = command {
            return if self.float.is_some() {
                Some(Refusal::AlreadyOpened)
            } else if !dollars(float) {
                Some(Refusal::WrongCurrency)
            } else if float.is_negative() {
                Some(Refusal::Invalid)
            } else {
                None
            };
        }
        if self.float.is_none() {
            return Some(Refusal::NotOpened);
        }
        if Some(at) != self.place {
            return Some(Refusal::WrongLocation);
        }
        if device != self.opener {
            return Some(Refusal::NotOpener);
        }
        if self.closed {
            return Some(Refusal::Closed);
        }
        match command {
            DrawerCommand::TakeCash(taken) if !dollars(&taken.amount) => {
                Some(Refusal::WrongCurrency)
            }
            DrawerCommand::TakeCash(taken) if self.payments.contains(&taken.payment) => {
                Some(Refusal::AlreadyTaken)
            }
            DrawerCommand::TakeCash(taken) if !taken.amount.is_positive() => Some(Refusal::Invalid),
            DrawerCommand::MoveCash(moved) if !dollars(&moved.amount) => {
                Some(Refusal::WrongCurrency)
            }
            DrawerCommand::MoveCash(moved) if !moved.amount.is_positive() => Some(Refusal::Invalid),
            DrawerCommand::Close { counted } if !dollars(counted) => Some(Refusal::WrongCurrency),
            DrawerCommand::Close { counted } if counted.is_negative() => Some(Refusal::Invalid),
            _ => None,
        }
    }

    fn apply(&mut self, command: &DrawerCommand, device: u64, at: Id<Location>) {
        match command {
            DrawerCommand::Open { float, .. } => {
                self.float = Some(*float);
                self.opener = device;
                self.place = Some(at);
                self.expected = float.minor();
            }
            DrawerCommand::TakeCash(taken) => {
                self.expected += taken.amount.minor();
                self.payments.insert(taken.payment);
            }
            DrawerCommand::MoveCash(moved) if moved.kind.takes_out() => {
                self.expected -= moved.amount.minor();
            }
            DrawerCommand::MoveCash(moved) => self.expected += moved.amount.minor(),
            DrawerCommand::NoSale(_) => {}
            DrawerCommand::Close { .. } => self.closed = true,
        }
    }
}

proptest! {
    /// The fold leaves any events as the model predicts.
    #[test]
    fn the_fold_matches_the_model(
        events in prop::collection::vec(
            (prop::bool::weighted(0.1), prop::bool::weighted(0.08), fold_event()),
            0..40,
        ),
    ) {
        let events: Vec<_> = numbered(events).into_iter().filter(|(_, event)| valid(event)).collect();
        let drawer = folded(&events);
        prop_assert_eq!(actual(&drawer), expected(&events));
        // What it should hold is what the model's sums say.
        if let (Some(float), Some(sums)) = (expected(&events).float, expected(&events).sums) {
            let held = float.minor() + sums.taken.minor() + sums.paid_in.minor()
                - sums.paid_out.minor() - sums.dropped.minor();
            prop_assert_eq!(drawer.expected().unwrap().map(Money::minor), Some(held));
        }
    }

    /// A device's commands are refused exactly as the model refuses them, and accepted ones
    /// apply with no conflict.
    #[test]
    fn commands_do_what_the_model_says(steps in prop::collection::vec(any_step(), 1..40)) {
        let mut drawer = Drawer::new(id(0xDA));
        let mut model = Model::default();
        for (seq, step) in (1..).zip(&steps) {
            let (command, device, at) = command(step);
            let decided = drawer.decide(at, id(device), command.clone());
            prop_assert_eq!(decided.as_ref().err().map(refusal_of), model.refused(&command, device, at));
            if let Ok(event) = decided {
                let before = drawer.conflicts().len();
                drawer.apply(&meta_at(device, seq, at), &event);
                prop_assert_eq!(drawer.conflicts().len(), before);
                model.apply(&command, device, at);
                if let DrawerEvent::Closed(closed) = event {
                    prop_assert_eq!(closed.expected.minor(), model.expected);
                }
                if model.float.is_some() {
                    prop_assert_eq!(drawer.expected().unwrap().map(Money::minor), Some(model.expected));
                }
            }
        }
    }
}
