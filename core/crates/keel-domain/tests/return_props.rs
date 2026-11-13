//! Property tests for returns (ADR-0032), against independent models, on a closed order of a few
//! lines, some shared by two checks:
//! - a device's returns, faulty ones included, are refused exactly when a model of the rules
//!   refuses them, for the same reason; each accepted one refunds what the model works out, by
//!   its own arithmetic, and applies with no conflict; a line is never refunded more than its
//!   checks charged it, and one returned in full is refunded exactly that;
//! - the fold, given any returns and reopenings in any order, by the owner or another device,
//!   leaves each line returned, and reports conflicts, as a model predicts.

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
use keel_domain::codec::{CatalogVersion, Name, ReasonCode, RulesVersion};
use keel_domain::order::{
    Allocation, Channel, CheckClosed, CommandError, ConflictKind, ItemSnapshot, Line, LineAdded,
    LineCharge, LineReturned, LinesAllocated, LinesReturned, Mode, Order, OrderCommand,
    OrderCreated, OrderEvent, Reason, Return,
};
use keel_domain::refund::Refund;
use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Currency, Hlc, Id, Money, Quantity, Unit};
use proptest::prelude::*;
use support::id;

fn location() -> Id<Location> {
    id(0x100)
}

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::USD)
}

fn reason() -> Reason {
    Reason { code: ReasonCode::new("damaged").unwrap(), note: None }
}

/// The owner, and another device.
const OWNER: u64 = 0xD1;
const OTHER: u64 = 0xD2;

fn meta(device: u64, seq: u64) -> EventMeta {
    EventMeta {
        event_id: id((device << 24) + seq),
        location: location(),
        origin_device: id::<Device>(device),
        origin_seq: seq.try_into().unwrap(),
        hlc: Hlc::new(1_000 + seq, 0).unwrap(),
        business_date: "2026-11-13".parse().unwrap(),
        actor: Actor::TeamMember(id(0x501)),
        approval: None,
    }
}

/// A line of the order as sold: how much, in which unit, whether two checks share it, and what
/// each check charged it, its net and tax.
#[derive(Clone, Debug)]
struct Sold {
    micros: i64,
    unit: Unit,
    shared: bool,
    charges: Vec<(i64, i64)>,
}

fn any_sold() -> impl Strategy<Value = Sold> {
    let quantity = prop_oneof![
        3 => (1_i64..=6).prop_map(|count| (count * 1_000_000, Unit::Each)),
        1 => (1_i64..=3_000_000).prop_map(|micros| (micros, Unit::Kilogram)),
    ];
    let charge =
        (prop_oneof![1 => Just(0_i64), 4 => 0_i64..5_000, 1 => 0_i64..=(i64::MAX / 8)], 0_i64..500);
    (quantity, any::<bool>(), charge.clone(), charge).prop_map(
        |((micros, unit), shared, first, second)| Sold {
            micros,
            unit,
            shared,
            charges: if shared { vec![first, second] } else { vec![first] },
        },
    )
}

fn line_id(n: usize) -> Id<Line> {
    id(0x300 + u64::try_from(n).unwrap())
}

/// The order, closed: its lines added, the shared ones allocated in halves to the main check and
/// check 0xC1, each check closed charging what `sold` says, and the order closed. Every event is
/// the owner's, and none conflicts.
fn closed_order(sold: &[Sold]) -> (Order, u64) {
    let mut order = Order::new(id(0xA));
    let mut seq = 0;
    let mut record =
        |order: &mut Order, command: Option<OrderCommand>, event: Option<OrderEvent>| {
            let event = event
                .unwrap_or_else(|| order.decide(location(), id(OWNER), command.unwrap()).unwrap());
            seq += 1;
            order.apply(&meta(OWNER, seq), &event);
            assert!(order.conflicts().is_empty(), "{:?}", order.conflicts());
        };
    let created = OrderCreated {
        channel: Channel::Pos,
        mode: Mode::Takeout,
        currency: Currency::USD,
        revenue_center: None,
        table: None,
        guest_count: None,
        customer: None,
        owner: None,
    };
    record(&mut order, Some(OrderCommand::Create(created)), None);
    for (n, line) in sold.iter().enumerate() {
        let added = LineAdded {
            line: line_id(n),
            item: ItemSnapshot {
                variant: id(0x400),
                catalog_version: CatalogVersion::from_bytes([7; 32]),
                name: Name::new("Espresso").unwrap(),
                tax_category: id(0x500),
                unit_price: usd(325),
            },
            quantity: Quantity::from_micros(line.micros, line.unit),
            modifiers: Vec::new(),
            seat: None,
            course: None,
            notes: None,
        };
        record(&mut order, Some(OrderCommand::AddLine(added)), None);
    }
    let main = order.main_check();
    let shared: Vec<usize> = (0..sold.len()).filter(|&n| sold[n].shared).collect();
    if !shared.is_empty() {
        record(&mut order, Some(OrderCommand::OpenCheck(id(0xC1))), None);
        let halves = shared.iter().flat_map(|&n| {
            [main, id(0xC1)].map(|check| Allocation {
                line: line_id(n),
                check,
                shares: NonZeroU16::MIN,
            })
        });
        let allocated = LinesAllocated::new(halves).unwrap();
        record(&mut order, Some(OrderCommand::AllocateLines(allocated)), None);
    }
    let snapshot = |check, part: usize| {
        let lines: Vec<LineCharge> = (0..sold.len())
            .filter(|&n| part == 0 || sold[n].shared)
            .map(|n| {
                let (net, tax) = sold[n].charges[part];
                LineCharge { line: line_id(n), gross: usd(net), net: usd(net), tax: usd(tax) }
            })
            .collect();
        let tax = Money::sum(Currency::USD, lines.iter().map(|line| line.tax)).unwrap();
        let net = Money::sum(Currency::USD, lines.iter().map(|line| line.net)).unwrap();
        let taxes = if tax.is_positive() {
            let taxable = if net.is_positive() { net } else { usd(1) };
            vec![keel_domain::order::TaxCharge { tax: id(0x900), taxable, amount: tax }]
        } else {
            Vec::new()
        };
        let total = net.checked_add(tax).unwrap();
        CheckClosed {
            check,
            rules_version: RulesVersion::from_bytes([9; 32]),
            lines,
            taxes,
            total,
            payments: total
                .is_positive()
                .then(|| keel_domain::codec::IdSet::new([id(0xB1)]).unwrap()),
        }
    };
    record(&mut order, None, Some(OrderEvent::CheckClosed(snapshot(main, 0))));
    if !shared.is_empty() {
        record(&mut order, None, Some(OrderEvent::CheckClosed(snapshot(id(0xC1), 1))));
    }
    record(&mut order, None, Some(OrderEvent::Closed));
    (order, seq)
}

/// `amount`'s share for `part` of `whole`, rounded half up: the model's own arithmetic, by the
/// quotient and how its remainder compares with half the whole.
fn model_share(amount: i64, part: i64, whole: i64) -> i64 {
    let (amount, part, whole) = (
        u128::try_from(amount).unwrap(),
        u128::try_from(part).unwrap(),
        u128::try_from(whole).unwrap(),
    );
    let product = amount * part;
    let (quotient, remainder) = (product / whole, product % whole);
    let up = u128::from(remainder * 2 >= whole);
    i64::try_from(quotient + up).unwrap()
}

/// What the model knows of a line.
#[derive(Clone, Debug)]
struct Known {
    micros: i64,
    unit: Unit,
    net: i64,
    tax: i64,
    returned: (i64, i64, i64),
}

fn known_lines(sold: &[Sold]) -> BTreeMap<Id<Line>, Known> {
    (0..sold.len())
        .map(|n| {
            let (net, tax) = sold[n]
                .charges
                .iter()
                .fold((0, 0), |(net, tax), part| (net + part.0, tax + part.1));
            (
                line_id(n),
                Known { micros: sold[n].micros, unit: sold[n].unit, net, tax, returned: (0, 0, 0) },
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Commands.

#[derive(Clone, Debug)]
enum Step {
    /// A return of each line picked, by how much: a count of units for a count, millionths
    /// otherwise; faults: 1 a line the order doesn't have, 2 the other unit, 3 nothing, 4 a
    /// line twice, 5 the refund of an earlier return, 6 another device, 7 no lines.
    Return { lines: Vec<(prop::sample::Index, i64)>, fault: u8 },
    /// Reopen the order.
    Reopen,
}

fn any_step() -> impl Strategy<Value = Step> {
    let line = (any::<prop::sample::Index>(), prop_oneof![3 => 1_i64..=3, 1 => 1_i64..=2_000_000]);
    prop_oneof![
        12 => (prop::collection::vec(line, 1..=3), prop_oneof![6 => Just(0_u8), 4 => 1_u8..=7])
            .prop_map(|(lines, fault)| Step::Return { lines, fault }),
        1 => Just(Step::Reopen),
    ]
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    NotOwner,
    NotClosed,
    Returned,
    NothingToReopen,
    ReturnExists,
    UnknownLine(Id<Line>),
    WrongUnit(Id<Line>),
    ReturnBeyond(Id<Line>),
    Invalid,
}

fn refusal_of(error: &CommandError) -> Refusal {
    match *error {
        CommandError::NotOwner(_) => Refusal::NotOwner,
        CommandError::NotClosed => Refusal::NotClosed,
        CommandError::Returned => Refusal::Returned,
        CommandError::NothingToReopen => Refusal::NothingToReopen,
        CommandError::ReturnExists(_) => Refusal::ReturnExists,
        CommandError::UnknownLine(line) => Refusal::UnknownLine(line),
        CommandError::WrongUnit(line) => Refusal::WrongUnit(line),
        CommandError::ReturnBeyond(line) => Refusal::ReturnBeyond(line),
        CommandError::Invalid(_) => Refusal::Invalid,
        ref other => panic!("no command here should be refused with {other:?}"),
    }
}

/// What the model expects of a step: refused, or each line's quantity, net and tax refunded.
type Outcome = Result<Vec<(Id<Line>, i64, i64, i64)>, Refusal>;

struct Model {
    lines: BTreeMap<Id<Line>, Known>,
    closed: bool,
    refunds: BTreeSet<Id<Refund>>,
}

impl Model {
    fn any_returned(&self) -> bool {
        self.lines.values().any(|known| known.returned.0 > 0)
    }

    fn reopen(&mut self) -> Result<(), Refusal> {
        if self.any_returned() {
            return Err(Refusal::Returned);
        }
        if !self.closed {
            return Err(Refusal::NothingToReopen);
        }
        self.closed = false;
        Ok(())
    }

    fn returns(
        &mut self,
        refund: Id<Refund>,
        device: u64,
        asked: &[(Id<Line>, Quantity)],
    ) -> Outcome {
        if device != OWNER {
            return Err(Refusal::NotOwner);
        }
        if !self.closed {
            return Err(Refusal::NotClosed);
        }
        if self.refunds.contains(&refund) {
            return Err(Refusal::ReturnExists);
        }
        let mut sorted = asked.to_vec();
        sorted.sort_by_key(|(line, _)| line.to_bytes());
        let ids: BTreeSet<Id<Line>> = sorted.iter().map(|(line, _)| *line).collect();
        if ids.len() != sorted.len() || sorted.is_empty() {
            return Err(Refusal::Invalid);
        }
        let mut refunded = Vec::new();
        for &(line, quantity) in &sorted {
            let known = self.lines.get(&line).ok_or(Refusal::UnknownLine(line))?;
            if quantity.unit() != known.unit {
                return Err(Refusal::WrongUnit(line));
            }
            if quantity.micros() <= 0 {
                return Err(Refusal::Invalid);
            }
            let (before, after) = (known.returned.0, known.returned.0 + quantity.micros());
            if after > known.micros {
                return Err(Refusal::ReturnBeyond(line));
            }
            let part = |amount| {
                model_share(amount, after, known.micros) - model_share(amount, before, known.micros)
            };
            refunded.push((line, quantity.micros(), part(known.net), part(known.tax)));
        }
        self.refunds.insert(refund);
        for &(line, micros, net, tax) in &refunded {
            let known = self.lines.get_mut(&line).unwrap();
            known.returned =
                (known.returned.0 + micros, known.returned.1 + net, known.returned.2 + tax);
        }
        Ok(refunded)
    }
}

/// A return asked for: its refund, the device asking, and how much of each line.
type Asked = (Id<Refund>, u64, Vec<(Id<Line>, Quantity)>);

/// The command a return step stands for: its refund, its device, and what it asks.
fn asked(
    step_lines: &[(prop::sample::Index, i64)],
    fault: u8,
    sold: &[Sold],
    seq: u64,
    last_refund: Option<Id<Refund>>,
) -> Asked {
    let mut lines: Vec<(Id<Line>, Quantity)> = step_lines
        .iter()
        .map(|(index, amount)| {
            let n = index.index(sold.len());
            let unit = if fault == 2 {
                if sold[n].unit == Unit::Each { Unit::Kilogram } else { Unit::Each }
            } else {
                sold[n].unit
            };
            let micros = match (fault, unit) {
                (3, _) => 0,
                (_, Unit::Each) => (amount % 4) * 1_000_000,
                _ => *amount,
            };
            let line = if fault == 1 { id(0x3FF) } else { line_id(n) };
            (line, Quantity::from_micros(micros.max(i64::from(fault != 3)), unit))
        })
        .collect();
    if fault == 4 {
        lines.push(lines[0]);
    }
    if fault == 7 {
        lines.clear();
    }
    let refund = match last_refund {
        Some(refund) if fault == 5 => refund,
        _ => id(0xC000 + seq),
    };
    let device = if fault == 6 { OTHER } else { OWNER };
    (refund, device, lines)
}

proptest! {
    /// A device's returns and reopenings are refused exactly as the model refuses them; accepted
    /// returns refund what the model works out, apply with no conflict, never refund a line more
    /// than it was charged, and refund one returned in full exactly that.
    #[test]
    fn returns_do_what_the_model_says(
        sold in prop::collection::vec(any_sold(), 1..=4),
        steps in prop::collection::vec(any_step(), 1..12),
    ) {
        let (mut order, mut seq) = closed_order(&sold);
        let mut model = Model { lines: known_lines(&sold), closed: true, refunds: BTreeSet::new() };
        let mut last_refund = None;
        for step in &steps {
            seq += 1;
            match step {
                Step::Reopen => {
                    let decided = order.decide(location(), id(OWNER), OrderCommand::Reopen(reason()));
                    prop_assert_eq!(decided.as_ref().err().map(refusal_of).map_or(Ok(()), Err), model.reopen());
                    if let Ok(event) = decided {
                        order.apply(&meta(OWNER, seq), &event);
                    }
                }
                Step::Return { lines, fault } => {
                    let (refund, device, asked) = asked(lines, *fault, &sold, seq, last_refund);
                    let command = OrderCommand::ReturnLines(Return { refund, lines: asked.clone(), reason: reason() });
                    let decided = order.decide(location(), id(device), command);
                    let outcome = model.returns(refund, device, &asked);
                    let got: Outcome = match &decided {
                        Err(error) => Err(refusal_of(error)),
                        Ok(OrderEvent::LinesReturned(returned)) => Ok(returned
                            .lines
                            .iter()
                            .map(|line| (line.line, line.quantity.micros(), line.net.minor(), line.tax.minor()))
                            .collect()),
                        Ok(other) => panic!("a return recorded {other:?}"),
                    };
                    prop_assert_eq!(&got, &outcome);
                    if let Ok(event) = decided {
                        let before = order.conflicts().len();
                        order.apply(&meta(device, seq), &event);
                        prop_assert_eq!(order.conflicts().len(), before, "an accepted return conflicted");
                        last_refund = Some(refund);
                    }
                }
            }
            for (line, known) in &model.lines {
                let returned = order.line(*line).unwrap().returned();
                let got = returned.map_or((0, 0, 0), |r| (r.quantity.micros(), r.net.minor(), r.tax.minor()));
                prop_assert_eq!(got, known.returned);
                prop_assert!(known.returned.1 <= known.net && known.returned.2 <= known.tax);
                if known.returned.0 == known.micros {
                    prop_assert_eq!((known.returned.1, known.returned.2), (known.net, known.tax));
                }
            }
        }
    }

    /// Any returns and reopenings, by the owner or another device, fold as the model predicts.
    #[test]
    fn returns_fold_as_the_model_predicts(
        sold in prop::collection::vec(any_sold(), 1..=4),
        events in prop::collection::vec(any_fold_event(), 0..12),
    ) {
        let (mut order, seq) = closed_order(&sold);
        let mut model = FoldModel {
            lines: known_lines(&sold),
            closed: true,
            charged: true,
            refunds: BTreeSet::new(),
            conflicts: Vec::new(),
        };
        for (i, (other, event)) in (1..).zip(&events) {
            let device = if *other { OTHER } else { OWNER };
            let meta = meta(device, seq + i);
            order.apply(&meta, &event.of(&sold));
            model.apply(meta.event_id, device, &event.of(&sold));
        }
        let got: Vec<(Id<Event>, ConflictKind)> = order.conflicts().iter().map(|c| (c.event, c.kind)).collect();
        prop_assert_eq!(got, model.conflicts.clone());
        for (line, known) in &model.lines {
            let returned = order.line(*line).unwrap().returned();
            let got = returned.map_or((0, 0, 0), |r| (r.quantity.micros(), r.net.minor(), r.tax.minor()));
            prop_assert_eq!(got, known.returned);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The fold.

/// A return of lines picked, in one currency, with a refund from a few; or a reopening.
#[derive(Clone, Debug)]
enum FoldEvent {
    Returned { refund: u64, euros: bool, lines: BTreeMap<u64, (bool, i64, i64, i64)> },
    Reopened,
}

fn any_fold_event() -> impl Strategy<Value = (bool, FoldEvent)> {
    // Each line: whether in the other unit, how much, and the net and tax refunded. Line 0xFF is
    // one the order doesn't have.
    let line = (prop::bool::weighted(0.1), 1_i64..=3_000_000, 0_i64..3_000, 0_i64..300);
    let lines =
        prop::collection::btree_map(prop_oneof![8 => 0_u64..4, 1 => Just(0xFF_u64)], line, 1..=3);
    let event = prop_oneof![
        10 => (1_u64..=4, prop::bool::weighted(0.1), lines)
            .prop_map(|(refund, euros, lines)| FoldEvent::Returned { refund, euros, lines }),
        1 => Just(FoldEvent::Reopened),
    ];
    (prop::bool::weighted(0.1), event)
}

impl FoldEvent {
    fn of(&self, sold: &[Sold]) -> OrderEvent {
        match self {
            FoldEvent::Reopened => OrderEvent::Reopened { reason: reason() },
            FoldEvent::Returned { refund, euros, lines } => {
                let currency = if *euros { Currency::EUR } else { Currency::USD };
                let lines = lines
                    .iter()
                    .map(|(&n, &(other_unit, micros, net, tax))| {
                        let index = usize::try_from(n).unwrap();
                        let unit = sold.get(index).map_or(Unit::Each, |line| line.unit);
                        let unit = if other_unit {
                            if unit == Unit::Each { Unit::Kilogram } else { Unit::Each }
                        } else {
                            unit
                        };
                        let line = if index < sold.len() { line_id(index) } else { id(0x3FF) };
                        LineReturned {
                            line,
                            quantity: Quantity::from_micros(micros, unit),
                            net: Money::from_minor(net, currency),
                            tax: Money::from_minor(tax, currency),
                        }
                    })
                    .collect();
                OrderEvent::LinesReturned(LinesReturned {
                    refund: id(0xC0 + refund),
                    lines,
                    reason: reason(),
                })
            }
        }
    }
}

struct FoldModel {
    lines: BTreeMap<Id<Line>, Known>,
    closed: bool,
    /// Whether the checks' snapshots stand: a reopening opens them again.
    charged: bool,
    refunds: BTreeSet<Id<Refund>>,
    conflicts: Vec<(Id<Event>, ConflictKind)>,
}

impl FoldModel {
    fn apply(&mut self, at: Id<Event>, device: u64, event: &OrderEvent) {
        if device != OWNER {
            self.conflicts.push((at, ConflictKind::NotOwner { by: id(device), owner: id(OWNER) }));
        }
        match event {
            OrderEvent::Reopened { .. } => {
                if self.lines.values().any(|known| known.returned.0 > 0) {
                    self.conflicts.push((at, ConflictKind::ReopenedAfterReturn));
                }
                self.closed = false;
                self.charged = false;
            }
            OrderEvent::LinesReturned(returned) => self.apply_returned(at, returned),
            other => panic!("not a fold event here: {other:?}"),
        }
    }

    fn apply_returned(&mut self, at: Id<Event>, returned: &LinesReturned) {
        if !self.refunds.insert(returned.refund) {
            return self.conflicts.push((at, ConflictKind::DuplicateReturn(returned.refund)));
        }
        if !self.closed {
            self.conflicts.push((at, ConflictKind::ReturnedBeforeClose));
        }
        for line in &returned.lines {
            let Some(known) = self.lines.get_mut(&line.line) else {
                self.conflicts.push((at, ConflictKind::UnknownLine(line.line)));
                continue;
            };
            if line.net.currency() != Currency::USD {
                self.conflicts.push((at, ConflictKind::CurrencyMismatch(line.line)));
                continue;
            }
            if line.quantity.unit() != known.unit {
                self.conflicts.push((at, ConflictKind::UnitMismatch(line.line)));
                continue;
            }
            known.returned = (
                known.returned.0 + line.quantity.micros(),
                known.returned.1 + line.net.minor(),
                known.returned.2 + line.tax.minor(),
            );
            let (net, tax) = if self.charged { (known.net, known.tax) } else { (0, 0) };
            if known.returned.0 > known.micros || known.returned.1 > net || known.returned.2 > tax {
                self.conflicts.push((at, ConflictKind::ReturnedBeyond(line.line)));
            }
        }
    }
}
