//! Property tests for checkout, against an independent model of its rules:
//! - on one device working a table, starting payments and closing checks, faulty attempts
//!   included, are refused exactly when the model refuses them, and for the same reason;
//! - every check's balance, and the order's issues, are always what the model computes from the
//!   order and its payments;
//! - a check closes with exactly what pricing charges it and the payments that settled it;
//! - when devices work the same table at once, on stale views, the merged order and payments
//!   have exactly the issues the model finds, and so does each device's partial view.
//!
//! The model reads the order and the payments' states and adds up minor units itself; it trusts
//! `keel-pricing` for a check's total, which is verified on its own.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::NonZeroU16;
use std::collections::{BTreeMap, BTreeSet};

use keel_domain::aggregate::{Aggregate, EventMeta};
use keel_domain::checkout::{Balance, Checkout, CheckoutError, Issue};
use keel_domain::codec::{CatalogVersion, IdSet, Name, ReasonCode, RulesVersion};
use keel_domain::order::{
    Allocation, Channel, Check, CheckClosed, CommandError, ItemSnapshot, Line, LineAdded,
    LinesAllocated, Mode, Order, OrderCommand, OrderCreated, OrderEvent, OrderStatus, Reason,
};
use keel_domain::payment::{
    CashTendered, Payment, PaymentAuthorized, PaymentCaptured, PaymentCommand, PaymentEnded,
    PaymentEvent, PaymentInitiated, PaymentStatus, Tender,
};
use keel_events::envelope::{Actor, Device, Location};
use keel_pricing::{Rules, Tax, price};
use keel_types::{Currency, Hlc, Id, Money, Quantity, Rate, Unit};
use proptest::prelude::*;
use proptest::sample::Index;
use support::{id, usd};

fn location() -> Id<Location> {
    id(0x100)
}

fn other_location() -> Id<Location> {
    id(0x101)
}

fn order_id() -> Id<Order> {
    id(0xA)
}

fn eur() -> Currency {
    Currency::from_code("EUR").unwrap()
}

fn dollars(minor: i64) -> Money {
    Money::from_minor(minor, usd())
}

fn reason(n: u8) -> Reason {
    let code = ["declined", "walkout", "birthday", "wrong_tender"][usize::from(n % 4)];
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

/// An 8.875% tax on the first category, rounded once per check, and none on the second.
fn rules() -> Rules {
    Rules {
        taxes: vec![Tax {
            id: id(0x900),
            name: "Sales tax".to_owned(),
            rate: Rate::from_percent("8.875".parse().unwrap()).unwrap(),
            categories: vec![id(0x500)],
            dining: None,
        }],
        ..Rules::untaxed()
    }
}

fn version() -> RulesVersion {
    RulesVersion::from_bytes([3; 32])
}

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
// A device working a table: the order, and the payments it knows.

/// A seed for one action on the table. A fault makes it one checkout must refuse, or might.
#[derive(Clone, Debug)]
struct Step {
    kind: u8,
    pick: Index,
    other: Index,
    amount: u8,
    fault: u8,
}

fn any_step() -> impl Strategy<Value = Step> {
    let fault = prop_oneof![7 => Just(0_u8), 3 => 1_u8..=7];
    (any::<u8>(), any::<Index>(), any::<Index>(), any::<u8>(), fault)
        .prop_map(|(kind, pick, other, amount, fault)| Step { kind, pick, other, amount, fault })
}

/// What a device does at the table.
#[derive(Clone, Debug)]
enum Act {
    Order(OrderCommand),
    Start {
        payment: Id<Payment>,
        check: Id<Check>,
        tender: Tender,
        amount: Money,
    },
    Outcome {
        payment: Id<Payment>,
        command: PaymentCommand,
    },
    Close(Id<Check>),
    /// Pay what a check owes, in cash, in full, then close it: three actions in a row.
    Settle(Id<Check>),
    /// A payment initiated without checkout, as only a faulty or hostile kernel would: in
    /// another currency, or for a check the order doesn't have.
    Forge {
        payment: Id<Payment>,
        check: Id<Check>,
        amount: Money,
    },
}

#[derive(Clone)]
struct Working {
    device: u64,
    order: Order,
    payments: Vec<Payment>,
    seq: u64,
    hlc: u64,
    minted: u64,
    order_log: Vec<(EventMeta, OrderEvent)>,
    payment_log: Vec<(Id<Payment>, EventMeta, PaymentEvent)>,
}

impl Working {
    fn new(device: u64, order: Order, payments: Vec<Payment>, hlc: u64) -> Working {
        Working {
            device,
            order,
            payments,
            seq: 0,
            hlc,
            minted: 0,
            order_log: Vec::new(),
            payment_log: Vec::new(),
        }
    }

    /// A new identifier in `space`, which no other device uses. Identifiers are handed out in no
    /// particular order, so a check's lines aren't in order of identifier by chance: an odd
    /// multiplier shuffles the first 4,096.
    fn fresh<T>(&self, space: u64) -> Id<T> {
        id((self.device << 20) + (space << 12) + ((self.minted * 0x9E5) & 0xFFF))
    }

    fn meta(&mut self, from: Id<Location>) -> EventMeta {
        self.seq += 1;
        self.hlc += 1;
        meta_at(self.device, self.seq, self.hlc, from)
    }

    fn checkout(&self) -> Checkout<'_> {
        Checkout::new(&self.order, &self.payments, &RULES, version())
    }

    fn record_order(&mut self, event: OrderEvent, from: Id<Location>) {
        let meta = self.meta(from);
        self.order.apply(&meta, &event);
        self.order_log.push((meta, event));
    }

    fn record_payment(&mut self, payment: Id<Payment>, event: PaymentEvent, from: Id<Location>) {
        let meta = self.meta(from);
        if let Some(known) = self.payments.iter_mut().find(|known| known.id() == payment) {
            known.apply(&meta, &event);
        } else {
            let mut fresh = Payment::new(payment);
            fresh.apply(&meta, &event);
            self.payments.push(fresh);
        }
        self.payment_log.push((payment, meta, event));
    }

    /// The action a step stands for on the device's view, and where it comes from. Fault 1
    /// names a check or payment that doesn't exist; faults 2 to 4 ask for too much, nothing, or
    /// euros; fault 5 reuses a payment's identifier; fault 6 forges a payment; fault 7 acts from
    /// another location.
    fn act(&self, step: &Step) -> (Act, Id<Location>) {
        let from = if step.fault == 7 { other_location() } else { location() };
        let lines: Vec<Id<Line>> = self.order.live_lines().map(Line::id).collect();
        let checks: Vec<Id<Check>> = self.order.checks().iter().map(Check::id).collect();
        let fresh_check = self.fresh(2);
        let pick_check = |index: &Index| {
            if step.fault == 1 || checks.is_empty() {
                fresh_check
            } else {
                checks[index.index(checks.len())]
            }
        };
        let act = match step.kind % 24 {
            0..=3 => {
                let price = 100 + i64::from(step.amount) * 7;
                Act::Order(OrderCommand::AddLine(line(
                    self.fresh(1),
                    price,
                    step.amount.is_multiple_of(3),
                )))
            }
            4 => Act::Order(OrderCommand::OpenCheck(fresh_check)),
            5 | 6 if !lines.is_empty() => {
                let line = lines[step.pick.index(lines.len())];
                let first = pick_check(&step.other);
                let second = checks[(step.other.index(checks.len()) + 1) % checks.len()];
                let mut allocations =
                    vec![Allocation { line, check: first, shares: NonZeroU16::MIN }];
                if step.amount.is_multiple_of(2) && second != first {
                    let shares = NonZeroU16::new(1 + u16::from(step.amount % 3)).unwrap();
                    allocations.push(Allocation { line, check: second, shares });
                }
                Act::Order(OrderCommand::AllocateLines(LinesAllocated::new(allocations).unwrap()))
            }
            7 if !lines.is_empty() => Act::Order(OrderCommand::CompLine {
                line: lines[step.pick.index(lines.len())],
                reason: reason(2),
            }),
            8..=10 => {
                let check = pick_check(&step.pick);
                let due = self.checkout().balance(check).map_or(0, |balance| balance.due.minor());
                let part = i64::from(step.amount % 4) * due / 4;
                let minor = match step.fault {
                    2 => due + 1 + i64::from(step.amount % 3),
                    3 => 0,
                    // A cent short, so the close that follows is refused.
                    _ if step.amount % 7 == 1 => due - 1,
                    _ if step.amount.is_multiple_of(2) => due,
                    _ => due - part,
                };
                let currency = if step.fault == 4 { eur() } else { usd() };
                let amount = Money::from_minor(minor, currency);
                let known = self.payments.first().map(Payment::id);
                let payment = match (step.fault, known) {
                    (5, Some(known)) => known,
                    _ => self.fresh(3),
                };
                if step.fault == 6 {
                    let check = if step.amount.is_multiple_of(2) { fresh_check } else { check };
                    let currency = if step.amount.is_multiple_of(2) { usd() } else { eur() };
                    Act::Forge { payment, check, amount: Money::from_minor(minor.max(1), currency) }
                } else {
                    let tender =
                        if step.amount.is_multiple_of(2) { Tender::Cash } else { Tender::Card };
                    Act::Start { payment, check, tender, amount }
                }
            }
            11..=14 if !self.payments.is_empty() => {
                let payment = &self.payments[step.pick.index(self.payments.len())];
                let command = outcome(payment, step);
                Act::Outcome { payment: payment.id(), command }
            }
            15..=16 => Act::Close(pick_check(&step.pick)),
            17 => Act::Order(OrderCommand::Close),
            18 => Act::Order(OrderCommand::Reopen(reason(3))),
            19 if step.amount.is_multiple_of(3) => Act::Order(OrderCommand::Void(reason(1))),
            20..=23 => Act::Settle(pick_check(&step.pick)),
            _ => Act::Order(OrderCommand::AddLine(line(self.fresh(1), 450, false))),
        };
        (act, from)
    }

    /// The actions that settle `check`: a payment in cash for what it owes, if it owes
    /// anything, captured in full; then its close. Each comes from the view after the one
    /// before, so this gives the next one, or `None` when the close has been tried.
    fn settling(&self, check: Id<Check>, done: &[Act]) -> Option<Act> {
        match done {
            [] => {
                let due = self.checkout().balance(check).map(|balance| balance.due);
                match due {
                    Ok(due) if due.is_positive() => Some(Act::Start {
                        payment: self.fresh(3),
                        check,
                        tender: Tender::Cash,
                        amount: due,
                    }),
                    _ => Some(Act::Close(check)),
                }
            }
            [Act::Start { payment, amount, .. }] => {
                let started = self.payments.iter().any(|known| known.id() == *payment);
                if !started {
                    return Some(Act::Close(check));
                }
                let cash = CashTendered { tendered: *amount, rounding: None };
                let capture = PaymentCaptured {
                    amount: *amount,
                    tip: None,
                    reference: None,
                    cash: Some(cash),
                };
                Some(Act::Outcome { payment: *payment, command: PaymentCommand::Capture(capture) })
            }
            [Act::Start { .. }, Act::Outcome { .. }] => Some(Act::Close(check)),
            _ => None,
        }
    }

    /// What checkout decides about starting a payment or closing a check, without recording
    /// it; `None` for other actions.
    fn decide(&self, act: &Act, from: Id<Location>) -> Option<Result<(), CheckoutError>> {
        let checkout = self.checkout();
        Some(match *act {
            Act::Start { payment, check, tender, amount } => {
                checkout.start_payment(from, payment, check, tender, amount).map(|_| ())
            }
            Act::Close(check) => checkout.close_check(from, check).map(|_| ()),
            _ => return None,
        })
    }

    /// Runs an action; returns whether it was accepted.
    fn run(&mut self, act: Act, from: Id<Location>) -> bool {
        self.minted += 1;
        match act {
            Act::Settle(check) => {
                let mut done = Vec::new();
                let mut closed = false;
                while let Some(next) = self.settling(check, &done) {
                    closed = self.run(next.clone(), from) && matches!(next, Act::Close(_));
                    done.push(next);
                }
                return closed;
            }
            Act::Order(command) => {
                let Ok(event) = self.order.decide(from, command) else { return false };
                self.record_order(event, from);
            }
            Act::Start { payment, check, tender, amount } => {
                let started = self.checkout().start_payment(from, payment, check, tender, amount);
                let Ok(event) = started else { return false };
                self.record_payment(payment, event, from);
            }
            Act::Outcome { payment, command } => {
                let known = self.payments.iter().find(|known| known.id() == payment).unwrap();
                let Ok(event) = known.decide(from, command) else { return false };
                self.record_payment(payment, event, from);
            }
            Act::Close(check) => {
                let Ok(event) = self.checkout().close_check(from, check) else { return false };
                self.record_order(event, from);
            }
            Act::Forge { payment, check, amount } => {
                let initiated =
                    PaymentInitiated { order: order_id(), check, tender: Tender::Card, amount };
                self.record_payment(payment, PaymentEvent::Initiated(initiated), from);
            }
        }
        true
    }

    fn step(&mut self, step: &Step) -> bool {
        let (act, from) = self.act(step);
        self.run(act, from)
    }
}

static RULES: std::sync::LazyLock<Rules> = std::sync::LazyLock::new(rules);

/// A line at `price`, taxed or not.
fn line(line: Id<Line>, price: i64, untaxed: bool) -> LineAdded {
    LineAdded {
        line,
        item: ItemSnapshot {
            variant: id(0x400),
            catalog_version: CatalogVersion::from_bytes([1; 32]),
            name: Name::new("Dinner").unwrap(),
            tax_category: id(if untaxed { 0x501 } else { 0x500 }),
            unit_price: dollars(price),
        },
        quantity: Quantity::from_whole(1, Unit::Each).unwrap(),
        modifiers: Vec::new(),
        seat: None,
        course: None,
        notes: None,
    }
}

/// An outcome for `payment`: mostly what a terminal or drawer would report, in full or in part.
fn outcome(payment: &Payment, step: &Step) -> PaymentCommand {
    let info = payment.info().unwrap();
    let limit = match payment.status() {
        PaymentStatus::Authorized(authorized) => authorized.amount,
        _ => info.amount,
    };
    let amount = Money::from_minor(
        limit.minor() - i64::from(step.amount % 2).min(limit.minor() - 1),
        limit.currency(),
    );
    let ended = PaymentEnded { reason: reason(0), reference: None };
    match step.kind % 4 {
        0 => PaymentCommand::Authorize(PaymentAuthorized { amount, reference: None }),
        1 | 2 => {
            let tip =
                step.amount.is_multiple_of(3).then(|| Money::from_minor(150, amount.currency()));
            let cash = (info.tender == Tender::Cash).then(|| {
                let paid = tip.map_or(amount, |tip| amount.checked_add(tip).unwrap());
                CashTendered { tendered: paid, rounding: None }
            });
            PaymentCommand::Capture(PaymentCaptured { amount, tip, reference: None, cash })
        }
        _ if step.amount.is_multiple_of(2) => PaymentCommand::Fail(ended),
        _ => PaymentCommand::Void(ended),
    }
}

/// A table: an open dine-in order in dollars, with its main check, on device 1.
fn table() -> Working {
    let mut device = Working::new(1, Order::new(order_id()), Vec::new(), 1_000);
    let created = OrderCreated {
        channel: Channel::Pos,
        mode: Mode::DineIn,
        currency: usd(),
        revenue_center: None,
        table: Some(id(0x200)),
        guest_count: None,
        customer: None,
        owner: None,
    };
    device.run(Act::Order(OrderCommand::Create(created)), location());
    device
}

// ---------------------------------------------------------------------------------------------
// The model: the rules, from the order and the payments' states.

/// The order's payments, each once, in ascending order of identifier.
fn order_payments(order: &Order, payments: &[Payment]) -> Vec<Payment> {
    let mut by_id: BTreeMap<[u8; 16], Payment> = BTreeMap::new();
    for payment in payments {
        if payment.info().is_some_and(|info| info.order == order.id()) {
            by_id.entry(payment.id().to_bytes()).or_insert_with(|| payment.clone());
        }
    }
    by_id.into_values().collect()
}

/// A check's total, captured amounts and tips, in minor units, and its unresolved payments.
struct ModelBalance {
    total: i128,
    captured: i128,
    tips: i128,
    unresolved: Vec<Id<Payment>>,
}

impl ModelBalance {
    fn due(&self) -> i128 {
        self.total - self.captured
    }
}

fn model_balance(order: &Order, payments: &[Payment], check: Id<Check>) -> Option<ModelBalance> {
    let currency = order.info()?.currency;
    let found = order.check(check)?;
    let total = match found.closed() {
        Some(closed) => closed.total,
        None => price(&order.check_basket(check)?, &RULES).ok()?.total,
    };
    let mut balance = ModelBalance {
        total: i128::from(total.minor()),
        captured: 0,
        tips: 0,
        unresolved: Vec::new(),
    };
    for payment in order_payments(order, payments) {
        let info = payment.info().unwrap();
        if info.check != check {
            continue;
        }
        match payment.status() {
            PaymentStatus::Initiated | PaymentStatus::Authorized(_) => {
                balance.unresolved.push(payment.id());
            }
            PaymentStatus::Captured(captured) if info.amount.currency() == currency => {
                balance.captured += i128::from(captured.amount.minor());
                balance.tips += i128::from(captured.tip.map_or(0, Money::minor));
            }
            _ => {}
        }
    }
    Some(balance)
}

/// What the model decides about starting a payment or closing a check from `from`: allowed, or
/// refused for the first rule it breaks, taking the rules in this order: the order is active
/// at the device's location; the check exists, and is open; then, for a payment, its identifier
/// is new, no payment on the check is unresolved, and the amount is in the order's currency,
/// more than zero and no more than the check owes; for a close, the check holds a live line, no
/// payment on it is unresolved, and its captured payments cover it. `None` for other actions.
fn model_decides(
    order: &Order,
    payments: &[Payment],
    act: &Act,
    from: Id<Location>,
) -> Option<Result<(), CheckoutError>> {
    let check = match act {
        Act::Start { check, .. } | Act::Close(check) => *check,
        _ => return None,
    };
    let refused = |error: CommandError| Some(Err(CheckoutError::Order(error)));
    let info = order.info().unwrap();
    if info.location != from {
        return refused(CommandError::WrongLocation);
    }
    if *order.status() != OrderStatus::Active {
        return refused(CommandError::OrderClosed);
    }
    match order.check(check) {
        None => return refused(CommandError::UnknownCheck(check)),
        Some(found) if !found.is_open() => return refused(CommandError::CheckClosed(check)),
        Some(_) => {}
    }
    let balance = model_balance(order, payments, check).unwrap();
    let due = Money::from_minor(i64::try_from(balance.due()).unwrap(), info.currency);
    let unresolved = balance.unresolved.first().copied();
    let on_check = |line: &Line| line.allocation().iter().any(|share| share.check == check);
    Some(if let Act::Start { payment, amount, .. } = act {
        if order_payments(order, payments).iter().any(|known| known.id() == *payment) {
            Err(CheckoutError::PaymentExists(*payment))
        } else if let Some(unresolved) = unresolved {
            Err(CheckoutError::Unresolved(unresolved))
        } else if amount.currency() != info.currency {
            Err(CheckoutError::WrongCurrency)
        } else if amount.minor() <= 0 {
            Err(CheckoutError::InvalidAmount)
        } else if i128::from(amount.minor()) > balance.due() {
            Err(CheckoutError::TooMuch { due })
        } else {
            Ok(())
        }
    } else if !order.live_lines().any(on_check) {
        Err(CheckoutError::Order(CommandError::NothingToClose))
    } else if let Some(unresolved) = unresolved {
        Err(CheckoutError::Unresolved(unresolved))
    } else if balance.due() > 0 {
        Err(CheckoutError::NotCovered { due })
    } else {
        Ok(())
    })
}

/// The order's issues, by the model: each payment's, in ascending order of payment, then each
/// check's.
fn model_issues(order: &Order, payments: &[Payment]) -> Vec<Issue> {
    let currency = order.info().unwrap().currency;
    let ended = matches!(order.status(), OrderStatus::Voided(_) | OrderStatus::Abandoned);
    let mut issues = Vec::new();
    for payment in order_payments(order, payments) {
        let info = payment.info().unwrap();
        let moving = matches!(
            payment.status(),
            PaymentStatus::Initiated | PaymentStatus::Authorized(_) | PaymentStatus::Captured(_)
        );
        let check = order.check(info.check);
        if check.is_none() {
            issues.push(Issue::UnknownCheck(payment.id()));
        }
        if info.amount.currency() != currency {
            issues.push(Issue::WrongCurrency(payment.id()));
        }
        if ended && moving {
            issues.push(Issue::OnVoidedOrder(payment.id()));
        }
        let settled_by: Option<Vec<Id<Payment>>> = check
            .and_then(Check::closed)
            .map(|closed| closed.payments.iter().flat_map(IdSet::iter).collect());
        if moving && settled_by.is_some_and(|settled| !settled.contains(&payment.id())) {
            issues.push(Issue::AfterClose(payment.id()));
        }
    }
    for check in order.checks() {
        let due = model_balance(order, payments, check.id()).unwrap().due();
        let by = Money::from_minor(i64::try_from(due.abs()).unwrap(), currency);
        if due < 0 {
            issues.push(Issue::Overpaid { check: check.id(), by });
        } else if due > 0 && !check.is_open() {
            issues.push(Issue::Underpaid { check: check.id(), by });
        }
    }
    issues
}

/// A captured payment of another order, on a check with the same identifier as this order's
/// main check.
fn foreign() -> Payment {
    let mut payment = Payment::new(id(0xF0));
    let check = order_id().cast();
    let initiated =
        PaymentInitiated { order: id(0xF), check, tender: Tender::Cash, amount: dollars(1000) };
    payment.apply(&meta_at(9, 1, 1, location()), &PaymentEvent::Initiated(initiated));
    let cash = Some(CashTendered { tendered: dollars(1000), rounding: None });
    let captured = PaymentCaptured { amount: dollars(1000), tip: None, reference: None, cash };
    payment.apply(&meta_at(9, 2, 2, location()), &PaymentEvent::Captured(captured));
    payment
}

/// Checks every check's balance, and the issues, against the model. Checkout is given each
/// payment twice, and another order's payment, which it must leave out.
fn agrees(order: &Order, payments: &[Payment]) -> Result<(), TestCaseError> {
    let foreign = foreign();
    let noisy = payments.iter().chain(payments).chain(core::iter::once(&foreign));
    let checkout = Checkout::new(order, noisy, &RULES, version());
    for check in order.checks() {
        let model = model_balance(order, payments, check.id()).unwrap();
        let balance: Balance = checkout.balance(check.id()).unwrap();
        prop_assert_eq!(i128::from(balance.total.minor()), model.total);
        prop_assert_eq!(i128::from(balance.captured.minor()), model.captured);
        prop_assert_eq!(i128::from(balance.tips.minor()), model.tips);
        prop_assert_eq!(i128::from(balance.due.minor()), model.due());
        prop_assert_eq!(&balance.unresolved, &model.unresolved);
    }
    prop_assert_eq!(
        checkout.balance(id(0xFFFF_FFFF)),
        Err(CheckoutError::Order(CommandError::UnknownCheck(id(0xFFFF_FFFF))))
    );
    prop_assert_eq!(checkout.issues().unwrap(), model_issues(order, payments));
    Ok(())
}

/// Checks that `closed` records exactly what pricing charges `check` now, on `order` as it was
/// before the close, and the captured payments in the order's currency that settled it.
fn closed_as_priced(
    order: &Order,
    payments: &[Payment],
    closed: &CheckClosed,
) -> Result<(), TestCaseError> {
    let basket = order.check_basket(closed.check).unwrap();
    let totals = price(&basket, &RULES).unwrap();
    let lines: Vec<Id<Line>> = order
        .live_lines()
        .filter(|line| line.allocation().iter().any(|share| share.check == closed.check))
        .map(Line::id)
        .collect();
    let mut expected: Vec<(Id<Line>, Money, Money, Money)> = lines
        .iter()
        .zip(&totals.lines)
        .map(|(line, priced)| (*line, priced.gross, priced.net, priced.tax))
        .collect();
    expected.sort_by_key(|(line, ..)| *line);
    let charged: Vec<(Id<Line>, Money, Money, Money)> = closed
        .lines
        .iter()
        .map(|charge| (charge.line, charge.gross, charge.net, charge.tax))
        .collect();
    prop_assert_eq!(charged, expected);
    let taxes: Vec<(Id<Tax>, Money, Money)> = totals
        .taxes
        .iter()
        .filter(|tax| tax.taxable.minor() > 0)
        .map(|tax| (tax.id, tax.taxable, tax.tax))
        .collect();
    let charged: Vec<(Id<Tax>, Money, Money)> =
        closed.taxes.iter().map(|charge| (charge.tax, charge.taxable, charge.amount)).collect();
    prop_assert_eq!(charged, taxes);
    prop_assert_eq!(closed.total, totals.total);
    prop_assert_eq!(closed.rules_version, version());
    let settled: BTreeSet<Id<Payment>> = order_payments(order, payments)
        .iter()
        .filter(|payment| {
            let info = payment.info().unwrap();
            info.check == closed.check
                && info.amount.currency() == usd()
                && payment.captured().is_some()
        })
        .map(Payment::id)
        .collect();
    let recorded: BTreeSet<Id<Payment>> = closed.payments.iter().flat_map(IdSet::iter).collect();
    prop_assert_eq!(recorded, settled);
    Ok(())
}

/// Runs an action on one device, checking what the model says of it: whether checkout accepts
/// it, and if not why, what a close records, and every balance and issue afterwards.
fn run_checked(device: &mut Working, act: &Act, from: Id<Location>) -> Result<(), TestCaseError> {
    let expected = model_decides(&device.order, &device.payments, act, from);
    prop_assert_eq!(device.decide(act, from), expected.clone(), "{:?} from {:?}", act, from);
    let before = (device.order.clone(), device.payments.clone());
    let accepted = device.run(act.clone(), from);
    if let Some(expected) = expected {
        prop_assert_eq!(accepted, expected.is_ok(), "{:?} from {:?}", act, from);
    }
    if let (true, Act::Close(_)) = (accepted, act) {
        let Some((_, OrderEvent::CheckClosed(closed))) = device.order_log.last() else {
            panic!("a close")
        };
        closed_as_priced(&before.0, &before.1, closed)?;
    }
    prop_assert!(device.order.conflicts().is_empty());
    prop_assert!(device.payments.iter().all(|payment| payment.conflicts().is_empty()));
    agrees(&device.order, &device.payments)
}

/// Merges devices' events: the order's in canonical order, and each payment's.
fn merged(devices: &[Working]) -> (Order, Vec<Payment>) {
    let key = |meta: &EventMeta| (meta.hlc, meta.origin_device, meta.origin_seq);
    let mut orders: Vec<&(EventMeta, OrderEvent)> =
        devices.iter().flat_map(|d| &d.order_log).collect();
    orders.sort_by_key(|(meta, _)| key(meta));
    let mut order = Order::new(order_id());
    for (meta, event) in orders {
        order.apply(meta, event);
    }
    let mut events: Vec<&(Id<Payment>, EventMeta, PaymentEvent)> =
        devices.iter().flat_map(|d| &d.payment_log).collect();
    events.sort_by_key(|(_, meta, _)| key(meta));
    let mut payments: BTreeMap<[u8; 16], Payment> = BTreeMap::new();
    for (id, meta, event) in events {
        payments.entry(id.to_bytes()).or_insert_with(|| Payment::new(*id)).apply(meta, event);
    }
    (order, payments.into_values().collect())
}

proptest! {
    /// One device working a table: checkout refuses to start a payment, or to close a check,
    /// exactly when the model refuses, and for the same reason; every balance and issue is the
    /// model's after every action; and each close records what pricing charged the check and who
    /// paid.
    #[test]
    fn checkout_does_what_the_model_says(steps in prop::collection::vec(any_step(), 1..80)) {
        let mut device = table();
        for step in &steps {
            let (act, from) = device.act(step);
            let Act::Settle(check) = act else {
                run_checked(&mut device, &act, from)?;
                continue;
            };
            let mut done = Vec::new();
            while let Some(next) = device.settling(check, &done) {
                run_checked(&mut device, &next, from)?;
                done.push(next);
            }
        }
    }

    /// Devices working the same table at once, on stale views: the merged order and payments
    /// have exactly the issues and balances the model computes, and so does each device's view
    /// of the merged order with only the payments it knows.
    #[test]
    fn concurrent_tables_have_the_issues_the_model_finds(
        prefix in prop::collection::vec(any_step(), 0..24),
        devices in prop::collection::vec(prop::collection::vec(any_step(), 0..20), 2..=3),
    ) {
        let mut first = table();
        for step in &prefix {
            first.step(step);
        }
        let mut working = vec![first.clone()];
        for (number, steps) in (2..).zip(&devices) {
            let mut device = Working::new(number, first.order.clone(), first.payments.clone(), first.hlc);
            for step in steps {
                device.step(step);
            }
            working.push(device);
        }
        let (order, payments) = merged(&working);
        agrees(&order, &payments)?;
        for device in &working {
            agrees(&order, &device.payments)?;
        }
    }
}
