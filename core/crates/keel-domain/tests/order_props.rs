//! Property tests for the order aggregate, against independent models:
//! - the fold, given any events in any order, leaves the order exactly as a model of the fold
//!   predicts, conflicts included. The model is written as queries over the events' positions
//!   rather than as a state machine, so it shares no structure with the fold;
//! - folding signed events takes only the aggregate's own stream;
//! - a device's commands, including faulty ones, are accepted exactly when a model of the command
//!   rules allows them, and an accepted command never causes a conflict and has the effect the
//!   model predicts;
//! - when devices act concurrently on stale views, the merged order is what the fold model
//!   predicts;
//! - every check's basket holds its share of each line it is allocated, and the checks' shares
//!   of a line add up to the line.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::num::{NonZeroU8, NonZeroU16};
use core::time::Duration;
use std::collections::BTreeSet;

use keel_domain::aggregate::{Aggregate, EventMeta, fold};
use keel_domain::codec::{CatalogVersion, Change, IdSet, Name, Note, ReasonCode};
use keel_domain::order::{
    Allocation, AttributesChanged, Channel, Check, CheckShare, ChosenModifier, ConflictKind,
    ItemSnapshot, Line, LineAdded, LineChanged, LineStatus, LinesAllocated, Mode, Order,
    OrderCommand, OrderCreated, OrderEvent, OrderInfo, OrderStatus, Placement, Prefix, Reason,
    Stage,
};
use keel_domain::schema::{DecodeError, DomainEvent};
use keel_events::envelope::{Actor, Device, Event, Location, StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_events::log::{EventDraft, LogConfig, LogHead, LogWriter};
use keel_pricing::{Dining, Rules, price};
use keel_types::{Currency, Hlc, Id, Money, Quantity, SeededEntropy, Timestamp, Unit};
use proptest::prelude::*;
use proptest::sample::Index;
use support::{any_event, id, usd};

fn location() -> Id<Location> {
    id(0x100)
}

fn other_location() -> Id<Location> {
    id(0x101)
}

fn order_id() -> Id<Order> {
    id(0xA)
}

/// The main check: its identifier is the order's.
fn main_check() -> Id<Check> {
    order_id().cast()
}

/// One share of a line, on `check`.
fn whole(check: Id<Check>) -> CheckShare {
    CheckShare { check, shares: NonZeroU16::MIN }
}

fn eur() -> Currency {
    Currency::from_code("EUR").unwrap()
}

fn reason(n: u8) -> Reason {
    let code = ["kitchen_error", "birthday", "walkout", "wrong_item"][usize::from(n % 4)];
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

fn created() -> OrderCreated {
    OrderCreated {
        channel: Channel::Pos,
        mode: Mode::DineIn,
        currency: usd(),
        revenue_center: None,
        table: Some(id(0x200)),
        guest_count: NonZeroU16::new(2),
        customer: None,
        owner: None,
    }
}

fn quantity_of(n: u8, unit: Unit) -> Quantity {
    Quantity::from_whole(i64::from(n), unit).unwrap()
}

fn oat_milk(price: Money) -> ChosenModifier {
    ChosenModifier {
        modifier: id(0x600),
        name: Name::new("Oat milk").unwrap(),
        prefix: Prefix::Plain,
        quantity: NonZeroU8::MIN,
        placement: Placement::Whole,
        unit_price: price,
        modifiers: Vec::new(),
    }
}

/// Sets the value when `flag` is up, and clears it otherwise.
fn set_or_clear<T>(flag: bool, value: T) -> Change<T> {
    if flag { Change::Set(value) } else { Change::Clear }
}

// ---------------------------------------------------------------------------------------------
// Commands, from seeds.

/// A seed for one command, turned into a command against the device's current view. A fault
/// makes it a command the device must refuse.
#[derive(Clone, Debug)]
struct Step {
    kind: u8,
    pick: Index,
    other: Index,
    amount: u8,
    flag: bool,
    fault: u8,
}

fn any_step() -> impl Strategy<Value = Step> {
    let fault = prop_oneof![8 => Just(0_u8), 2 => 1_u8..=5];
    (any::<u8>(), any::<Index>(), any::<Index>(), any::<u8>(), any::<bool>(), fault).prop_map(
        |(kind, pick, other, amount, flag, fault)| Step { kind, pick, other, amount, flag, fault },
    )
}

fn modifiers(step: &Step, currency: Currency) -> Vec<ChosenModifier> {
    if step.amount.is_multiple_of(3) {
        return Vec::new();
    }
    vec![oat_milk(Money::from_minor(i64::from(step.amount % 2) * 50, currency))]
}

/// A line to add. Faults 2 to 4 price it in euros, give it a negative price, or a zero quantity.
fn line_added(line: Id<Line>, step: &Step) -> LineAdded {
    let (currency, price, count) = match step.fault {
        2 => (eur(), i64::from(step.amount) * 10, 1 + step.amount % 3),
        3 => (usd(), -1 - i64::from(step.amount), 1 + step.amount % 3),
        4 => (usd(), i64::from(step.amount) * 10, 0),
        _ => (usd(), i64::from(step.amount) * 10, 1 + step.amount % 3),
    };
    LineAdded {
        line,
        item: ItemSnapshot {
            variant: id(0x400),
            catalog_version: CatalogVersion::from_bytes([1; 32]),
            name: Name::new("Flat white").unwrap(),
            tax_category: id(0x500),
            unit_price: Money::from_minor(price, currency),
        },
        quantity: quantity_of(count, Unit::Each),
        modifiers: modifiers(step, currency),
        seat: step.flag.then(|| NonZeroU16::new(1 + u16::from(step.amount % 4)).unwrap()),
        course: None,
        notes: None,
    }
}

/// A change to a line. Faults 1 to 4 give a quantity in another unit, modifiers in euros, a
/// negative modifier price, or a zero quantity.
fn line_changed(line: Id<Line>, step: &Step) -> LineChanged {
    let mut changed = LineChanged::to(line);
    match (step.fault, step.amount % 5) {
        (1, _) => changed.quantity = Some(quantity_of(1 + step.amount % 4, Unit::Kilogram)),
        (2, _) => changed.modifiers = Some(vec![oat_milk(Money::from_minor(50, eur()))]),
        (3, _) => changed.modifiers = Some(vec![oat_milk(Money::from_minor(-50, usd()))]),
        (4, _) => changed.quantity = Some(quantity_of(0, Unit::Each)),
        (_, 0) => changed.quantity = Some(quantity_of(1 + step.amount % 4, Unit::Each)),
        (_, 1) => changed.modifiers = Some(modifiers(step, usd())),
        (_, 2) => {
            changed.seat = Some(set_or_clear(
                step.flag,
                NonZeroU16::new(1 + u16::from(step.amount % 3)).unwrap(),
            ));
        }
        (_, 3) => {
            changed.course =
                Some(set_or_clear(step.flag, NonZeroU8::new(1 + step.amount % 3).unwrap()));
        }
        _ => {
            changed.notes = Some(set_or_clear(
                step.flag,
                Note::new(if step.amount.is_multiple_of(2) { "no salt" } else { "extra hot" })
                    .unwrap(),
            ));
        }
    }
    changed
}

fn attributes(step: &Step) -> AttributesChanged {
    let n = u64::from(step.amount % 3);
    let mut changed = AttributesChanged::default();
    match step.amount % 6 {
        0 => changed.mode = Some(Mode::ALL[usize::from(step.amount % 8)]),
        1 => changed.table = Some(set_or_clear(step.flag, id(0x200 + n))),
        2 => {
            changed.guest_count = Some(set_or_clear(
                step.flag,
                NonZeroU16::new(1 + u16::from(step.amount % 3)).unwrap(),
            ));
        }
        3 => changed.customer = Some(set_or_clear(step.flag, id(0x700 + n))),
        4 => changed.owner = Some(set_or_clear(step.flag, id(0x300 + n))),
        _ => changed.revenue_center = Some(set_or_clear(step.flag, id(0x800 + n))),
    }
    changed
}

/// An allocation of one or two of the lines the device knows among one to three of its checks,
/// in shares of one or two. Fault 1 allocates a line it doesn't know, or to a check it doesn't
/// know.
fn allocation(
    view: &Order,
    step: &Step,
    fresh: Id<Line>,
    fresh_check: Id<Check>,
) -> LinesAllocated {
    let lines: Vec<Id<Line>> = view.lines().iter().map(Line::id).collect();
    let known: Vec<Id<Check>> = view.checks().iter().map(Check::id).collect();
    let mut chosen: Vec<Id<Line>> = if lines.is_empty() || (step.fault == 1 && step.flag) {
        vec![fresh]
    } else {
        vec![lines[step.pick.index(lines.len())]]
    };
    if step.amount.is_multiple_of(5) && lines.len() > 1 {
        chosen.push(lines[step.other.index(lines.len())]);
    }
    let first = step.other.index(known.len().max(1));
    let mut checks: BTreeSet<Id<Check>> = (0..=usize::from(step.amount % 3))
        .filter_map(|k| known.get((first + k) % known.len().max(1)).copied())
        .collect();
    if step.fault == 1 && !step.flag {
        checks.insert(fresh_check);
    }
    let allocations = chosen.iter().collect::<BTreeSet<_>>().into_iter().flat_map(|&line| {
        checks.iter().enumerate().map(move |(k, &check)| Allocation {
            line,
            check,
            shares: NonZeroU16::new(1 + u16::from((step.amount >> k) & 1)).unwrap(),
        })
    });
    LinesAllocated::new(allocations).unwrap()
}

/// The command a step stands for on the device's view, and the location it comes from: mostly
/// line work, sometimes the attributes or the checks, rarely closing the order. Lines are
/// picked among those the device knows, or a fresh identifier when it knows none. Fault 1 adds
/// a line that exists, opens a check that exists, or works on a line or check that doesn't;
/// fault 5 sends the command from another location.
fn command(
    view: &Order,
    step: &Step,
    fresh: Id<Line>,
    fresh_check: Id<Check>,
) -> (OrderCommand, Id<Location>) {
    let lines: Vec<Id<Line>> = view.lines().iter().map(Line::id).collect();
    let known =
        |index: &Index| if lines.is_empty() { fresh } else { lines[index.index(lines.len())] };
    let pick = |index: &Index| if step.fault == 1 { fresh } else { known(index) };
    let command = match (step.kind % 48, step.fault) {
        (0..=9, 1) => OrderCommand::AddLine(line_added(known(&step.pick), step)),
        (0..=9, _) => OrderCommand::AddLine(line_added(fresh, step)),
        (10..=13, _) => OrderCommand::ChangeLine(line_changed(known(&step.pick), step)),
        (14..=17, _) => OrderCommand::RemoveLine(pick(&step.pick)),
        (18..=23, _) => OrderCommand::FireLines(
            IdSet::new([pick(&step.pick), known(&step.other)].into_iter().collect::<BTreeSet<_>>())
                .unwrap(),
        ),
        (24..=27, _) => {
            OrderCommand::VoidLine { line: pick(&step.pick), reason: reason(step.amount) }
        }
        (28..=31, _) => {
            OrderCommand::CompLine { line: pick(&step.pick), reason: reason(step.amount) }
        }
        (32..=37, _) => OrderCommand::ChangeAttributes(attributes(step)),
        (38, _) => OrderCommand::Void(reason(step.amount)),
        (39, _) => OrderCommand::Abandon,
        (40..=41, 1) => {
            let checks = view.checks();
            let known = checks.get(step.pick.index(checks.len().max(1))).map(Check::id);
            OrderCommand::OpenCheck(known.unwrap_or(fresh_check))
        }
        (40..=41, _) => OrderCommand::OpenCheck(fresh_check),
        _ => OrderCommand::AllocateLines(allocation(view, step, fresh, fresh_check)),
    };
    let from = if step.fault == 5 { other_location() } else { location() };
    (command, from)
}

// ---------------------------------------------------------------------------------------------
// Lines and details, in the models' terms.

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelLine {
    id: Id<Line>,
    item: ItemSnapshot,
    status: LineStatus,
    comp: Option<Reason>,
    quantity: Quantity,
    modifiers: Vec<ChosenModifier>,
    seat: Option<NonZeroU16>,
    course: Option<NonZeroU8>,
    notes: Option<Note>,
    fired: bool,
    allocation: Vec<CheckShare>,
}

impl ModelLine {
    fn new(added: &LineAdded) -> ModelLine {
        ModelLine {
            id: added.line,
            item: added.item.clone(),
            status: LineStatus::Pending,
            comp: None,
            quantity: added.quantity,
            modifiers: added.modifiers.clone(),
            seat: added.seat,
            course: added.course,
            notes: added.notes.clone(),
            fired: false,
            allocation: vec![whole(main_check())],
        }
    }

    /// Applies a change: each field given replaces the line's.
    fn change(&mut self, changed: &LineChanged) {
        if let Some(quantity) = changed.quantity {
            self.quantity = quantity;
        }
        if let Some(modifiers) = &changed.modifiers {
            self.modifiers.clone_from(modifiers);
        }
        if let Some(seat) = changed.seat {
            self.seat = seat.into_option();
        }
        if let Some(course) = changed.course {
            self.course = course.into_option();
        }
        if let Some(notes) = &changed.notes {
            self.notes = notes.clone().into_option();
        }
    }

    fn live(&self) -> bool {
        matches!(self.status, LineStatus::Pending | LineStatus::Fired)
    }
}

/// The order's lines, in the models' terms.
fn lines_of(order: &Order) -> Vec<ModelLine> {
    order
        .lines()
        .iter()
        .map(|line| ModelLine {
            id: line.id(),
            item: line.item().clone(),
            status: line.status().clone(),
            comp: line.comp().cloned(),
            quantity: line.quantity(),
            modifiers: line.modifiers().to_vec(),
            seat: line.seat(),
            course: line.course(),
            notes: line.notes().cloned(),
            fired: line.was_fired(),
            allocation: line.allocation().to_vec(),
        })
        .collect()
}

/// The order's checks: each one's identifier and number.
fn checks_of(order: &Order) -> Vec<(Id<Check>, u32)> {
    order.checks().iter().map(|check| (check.id(), check.number().get())).collect()
}

/// Each line an allocation names, with its new shares: grouped here from the allocations
/// themselves.
fn allocated_lines(allocated: &LinesAllocated) -> Vec<(Id<Line>, Vec<CheckShare>)> {
    let mut lines: Vec<(Id<Line>, Vec<CheckShare>)> = Vec::new();
    for allocation in allocated.iter() {
        let share = CheckShare { check: allocation.check, shares: allocation.shares };
        match lines.iter_mut().find(|(line, _)| *line == allocation.line) {
            Some((_, shares)) => shares.push(share),
            None => lines.push((allocation.line, vec![share])),
        }
    }
    lines
}

fn stage_of(lines: &[ModelLine]) -> Stage {
    let live: Vec<&ModelLine> = lines.iter().filter(|line| line.live()).collect();
    if live.is_empty() {
        Stage::Draft
    } else if live.iter().any(|line| line.status == LineStatus::Pending) {
        Stage::Open
    } else {
        Stage::Submitted
    }
}

/// The order's details, as the event at `meta` creates it.
fn info_of(meta: &EventMeta, created: &OrderCreated) -> OrderInfo {
    OrderInfo {
        location: meta.location,
        channel: created.channel,
        currency: created.currency,
        created_by: meta.event_id,
        mode: created.mode,
        revenue_center: created.revenue_center,
        table: created.table,
        guest_count: created.guest_count,
        customer: created.customer,
        owner: created.owner,
    }
}

/// Applies an attribute change: each field given replaces the order's.
fn change_attributes(info: &mut OrderInfo, changed: &AttributesChanged) {
    fn set<T: Copy>(field: &mut Option<T>, change: Option<&Change<T>>) {
        match change {
            Some(Change::Set(value)) => *field = Some(*value),
            Some(Change::Clear) => *field = None,
            None => {}
        }
    }
    if let Some(mode) = changed.mode {
        info.mode = mode;
    }
    set(&mut info.revenue_center, changed.revenue_center.as_ref());
    set(&mut info.table, changed.table.as_ref());
    set(&mut info.guest_count, changed.guest_count.as_ref());
    set(&mut info.customer, changed.customer.as_ref());
    set(&mut info.owner, changed.owner.as_ref());
}

// ---------------------------------------------------------------------------------------------
// A model of one device's commands: which are allowed, and their effect.

struct Model {
    info: OrderInfo,
    lines: Vec<ModelLine>,
    status: OrderStatus,
    checks: Vec<Id<Check>>,
}

/// Whether a price can go into the order: in its currency (always US dollars here), and not
/// negative.
fn well_priced(price: Money) -> bool {
    price.currency() == usd() && !price.is_negative()
}

fn well_priced_modifiers(modifiers: &[ChosenModifier]) -> bool {
    modifiers.iter().all(|modifier| {
        well_priced(modifier.unit_price) && well_priced_modifiers(&modifier.modifiers)
    })
}

/// Whether a change given for an optional field changes it.
fn differs<T: PartialEq>(current: Option<&T>, change: Option<&Change<T>>) -> bool {
    match change {
        None => true,
        Some(Change::Set(value)) => current != Some(value),
        Some(Change::Clear) => current.is_some(),
    }
}

impl Model {
    fn new(info: OrderInfo) -> Model {
        Model { info, lines: Vec::new(), status: OrderStatus::Active, checks: vec![main_check()] }
    }

    fn line(&self, id: Id<Line>) -> Option<&ModelLine> {
        self.lines.iter().find(|line| line.id == id)
    }

    fn line_mut(&mut self, id: Id<Line>) -> &mut ModelLine {
        self.lines.iter_mut().find(|line| line.id == id).unwrap()
    }

    fn allows(&self, command: &OrderCommand, from: Id<Location>) -> bool {
        if from != self.info.location || self.status != OrderStatus::Active {
            return false;
        }
        let status = |id: Id<Line>| self.line(id).map(|line| line.status.clone());
        match command {
            OrderCommand::Create(_) => false,
            OrderCommand::AddLine(added) => {
                self.line(added.line).is_none()
                    && well_priced(added.item.unit_price)
                    && well_priced_modifiers(&added.modifiers)
                    && added.quantity.is_positive()
            }
            OrderCommand::ChangeLine(changed) => {
                self.line(changed.line).is_some_and(|line| Model::allows_change(line, changed))
            }
            OrderCommand::RemoveLine(line) => status(*line) == Some(LineStatus::Pending),
            OrderCommand::FireLines(lines) => {
                lines.iter().all(|line| status(line) == Some(LineStatus::Pending))
            }
            OrderCommand::VoidLine { line, .. } => status(*line) == Some(LineStatus::Fired),
            OrderCommand::CompLine { line, .. } => {
                self.line(*line).is_some_and(|line| line.live() && line.comp.is_none())
            }
            OrderCommand::ChangeAttributes(changed) => {
                let current = &self.info;
                *changed != AttributesChanged::default()
                    && changed.mode.is_none_or(|mode| mode != current.mode)
                    && differs(current.revenue_center.as_ref(), changed.revenue_center.as_ref())
                    && differs(current.table.as_ref(), changed.table.as_ref())
                    && differs(current.guest_count.as_ref(), changed.guest_count.as_ref())
                    && differs(current.customer.as_ref(), changed.customer.as_ref())
                    && differs(current.owner.as_ref(), changed.owner.as_ref())
            }
            OrderCommand::Void(_) => true,
            // Nothing may have been made: every line was removed before it was fired.
            OrderCommand::Abandon => !self.lines.iter().any(|line| line.live() || line.fired),
            OrderCommand::OpenCheck(check) => !self.checks.contains(check),
            // Every line named is live and gets a new allocation, to checks that exist.
            OrderCommand::AllocateLines(allocated) => {
                allocated_lines(allocated).iter().all(|(id, shares)| {
                    self.line(*id).is_some_and(|line| line.live() && line.allocation != *shares)
                        && shares.iter().all(|share| self.checks.contains(&share.check))
                })
            }
        }
    }

    /// A pending line's details can change, if every field given changes, a quantity stays
    /// positive and in the line's unit, and modifiers are well priced.
    fn allows_change(line: &ModelLine, changed: &LineChanged) -> bool {
        line.status == LineStatus::Pending
            && *changed != LineChanged::to(line.id)
            && changed.quantity.is_none_or(|quantity| {
                quantity != line.quantity
                    && quantity.unit() == line.quantity.unit()
                    && quantity.is_positive()
            })
            && changed.modifiers.as_ref().is_none_or(|modifiers| {
                *modifiers != line.modifiers && well_priced_modifiers(modifiers)
            })
            && differs(line.seat.as_ref(), changed.seat.as_ref())
            && differs(line.course.as_ref(), changed.course.as_ref())
            && differs(line.notes.as_ref(), changed.notes.as_ref())
    }

    fn apply(&mut self, command: &OrderCommand) {
        match command {
            OrderCommand::Create(_) => {}
            OrderCommand::AddLine(added) => self.lines.push(ModelLine::new(added)),
            OrderCommand::ChangeLine(changed) => self.line_mut(changed.line).change(changed),
            OrderCommand::RemoveLine(id) => self.line_mut(*id).status = LineStatus::Removed,
            OrderCommand::FireLines(lines) => {
                for id in lines.iter() {
                    let line = self.line_mut(id);
                    line.status = LineStatus::Fired;
                    line.fired = true;
                }
            }
            OrderCommand::VoidLine { line: id, reason } => {
                self.line_mut(*id).status = LineStatus::Voided(reason.clone());
            }
            OrderCommand::CompLine { line: id, reason } => {
                self.line_mut(*id).comp = Some(reason.clone());
            }
            OrderCommand::ChangeAttributes(changed) => change_attributes(&mut self.info, changed),
            OrderCommand::Void(reason) => self.status = OrderStatus::Voided(reason.clone()),
            OrderCommand::Abandon => self.status = OrderStatus::Abandoned,
            OrderCommand::OpenCheck(check) => self.checks.push(*check),
            OrderCommand::AllocateLines(allocated) => {
                for (id, shares) in allocated_lines(allocated) {
                    self.line_mut(id).allocation = shares;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Devices, and replicas that fold their events.

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

/// A device working on its own view of the order.
struct Working {
    device: u64,
    view: Order,
    seq: u64,
    hlc: u64,
    lines_minted: u64,
    checks_minted: u64,
    log: Vec<(EventMeta, OrderEvent)>,
}

impl Working {
    fn new(device: u64, view: Order, hlc: u64) -> Working {
        Working { device, view, seq: 0, hlc, lines_minted: 0, checks_minted: 0, log: Vec::new() }
    }

    /// An identifier for the device's next line, which no other device uses.
    fn fresh(&self) -> Id<Line> {
        id((self.device << 16) + 0x1000 + self.lines_minted)
    }

    /// An identifier for the device's next check, which no other device uses.
    fn fresh_check(&self) -> Id<Check> {
        id((self.device << 16) + 0x2000 + self.checks_minted)
    }

    /// Runs a step; returns whether its command was accepted.
    fn step(&mut self, step: &Step) -> bool {
        let (command, from) = command(&self.view, step, self.fresh(), self.fresh_check());
        self.run(command, from, u64::from(step.amount % 3))
    }

    fn run(&mut self, command: OrderCommand, from: Id<Location>, delay: u64) -> bool {
        let adds = matches!(command, OrderCommand::AddLine(_));
        let opens = matches!(command, OrderCommand::OpenCheck(_));
        let Ok(event) = self.view.decide(from, command) else { return false };
        if adds {
            self.lines_minted += 1;
        }
        if opens {
            self.checks_minted += 1;
        }
        self.seq += 1;
        self.hlc += 1 + delay;
        let meta = meta_at(self.device, self.seq, self.hlc, from);
        self.view.apply(&meta, &event);
        self.log.push((meta, event));
        true
    }
}

/// Folds events in canonical order: by HLC, then device, then position in the device's log.
fn replica(mut events: Vec<(EventMeta, OrderEvent)>) -> (Order, Vec<(EventMeta, OrderEvent)>) {
    events.sort_by(|(a, _), (b, _)| {
        (a.hlc, a.origin_device, a.origin_seq).cmp(&(b.hlc, b.origin_device, b.origin_seq))
    });
    let mut order = Order::new(order_id());
    for (meta, event) in &events {
        order.apply(meta, event);
    }
    (order, events)
}

// ---------------------------------------------------------------------------------------------
// An independent model of the fold, as queries over the events' positions.

/// What the fold model predicts.
struct Expected {
    info: Option<OrderInfo>,
    lines: Vec<ModelLine>,
    status: OrderStatus,
    checks: Vec<(Id<Check>, u32)>,
    conflicts: Vec<(Id<Event>, ConflictKind)>,
}

/// The lines an event refers to, other than by adding them.
fn referred(event: &OrderEvent) -> Vec<Id<Line>> {
    match event {
        OrderEvent::LineChanged(changed) => vec![changed.line],
        OrderEvent::LineRemoved { line }
        | OrderEvent::LineVoided { line, .. }
        | OrderEvent::LineComped { line, .. } => vec![*line],
        OrderEvent::LinesFired { lines } => lines.iter().collect(),
        OrderEvent::LinesAllocated(allocated) => {
            allocated_lines(allocated).into_iter().map(|(line, _)| line).collect()
        }
        _ => Vec::new(),
    }
}

/// Whether every price in `modifiers`, at any depth, is in `currency`.
fn priced_in(modifiers: &[ChosenModifier], currency: Currency) -> bool {
    modifiers.iter().all(|modifier| {
        modifier.unit_price.currency() == currency && priced_in(&modifier.modifiers, currency)
    })
}

/// What the model predicts for one line.
struct History {
    /// The line's final state.
    line: ModelLine,
    /// The position of the event that took it off, if any.
    terminal: Option<usize>,
    /// The position of the first event that fired it, even after it was taken off.
    sent: Option<usize>,
    /// The allocations of the line that didn't apply, by position, each with the first check
    /// it named that didn't exist yet.
    unknown_checks: Vec<(usize, Id<Check>)>,
}

/// What the model predicts for the line added at `birth`. The conflicts its events cause go into
/// `conflicts`.
///
/// `applying` lists the positions of the events that apply to the order: after its creation,
/// and at its location.
///
/// `opened` gives the position at which each check was opened.
#[allow(clippy::too_many_arguments, reason = "the model's queries need every part of it")]
fn expect_line(
    events: &[(EventMeta, OrderEvent)],
    applying: &[usize],
    birth: usize,
    currency: Currency,
    close: Option<usize>,
    opened: &[(Id<Check>, usize)],
    conflicts: &mut Vec<(Id<Event>, ConflictKind)>,
) -> History {
    let at = |i: usize| events[i].0.event_id;
    let OrderEvent::LineAdded(added) = &events[birth].1 else { unreachable!() };
    let id = added.line;
    let later: Vec<usize> = applying
        .iter()
        .copied()
        .filter(|&i| i > birth && referred(&events[i].1).contains(&id))
        .collect();
    // The line is live until its first removal or void.
    let terminal = later.iter().copied().find(|&i| {
        matches!(events[i].1, OrderEvent::LineRemoved { .. } | OrderEvent::LineVoided { .. })
    });
    let live = |i: usize| terminal.is_none_or(|terminal| i < terminal);
    let fires = |i: &usize| matches!(events[*i].1, OrderEvent::LinesFired { .. });
    let first_fire = later.iter().copied().find(|&i| live(i) && fires(&i));
    let sent = later.iter().copied().find(fires);
    let mut line = ModelLine::new(added);
    line.fired = sent.is_some();
    for &i in &later {
        let conflict = match &events[i].1 {
            OrderEvent::LineChanged(_) if !live(i) => Some(ConflictKind::ChangedAfterRemoval(id)),
            OrderEvent::LineChanged(changed)
                if changed.quantity.is_some_and(|q| q.unit() != added.quantity.unit()) =>
            {
                Some(ConflictKind::UnitMismatch(id))
            }
            OrderEvent::LineChanged(changed)
                if changed.modifiers.as_ref().is_some_and(|m| !priced_in(m, currency)) =>
            {
                Some(ConflictKind::CurrencyMismatch(id))
            }
            OrderEvent::LineChanged(changed) => {
                line.change(changed);
                first_fire
                    .is_some_and(|fire| fire < i)
                    .then_some(ConflictKind::ChangedAfterFire(id))
            }
            OrderEvent::LinesFired { .. } => {
                (!live(i)).then_some(ConflictKind::FiredAfterRemoval(id))
            }
            OrderEvent::LineComped { .. } if !live(i) => Some(ConflictKind::CompedAfterRemoval(id)),
            OrderEvent::LineComped { reason, .. } => {
                // The first comp wins.
                line.comp.get_or_insert_with(|| reason.clone());
                None
            }
            _ => None,
        };
        conflicts.extend(conflict.map(|kind| (at(i), kind)));
    }
    line.status = match terminal.map(|terminal| &events[terminal].1) {
        Some(OrderEvent::LineRemoved { .. }) => LineStatus::Removed,
        Some(OrderEvent::LineVoided { reason, .. }) => LineStatus::Voided(reason.clone()),
        _ if first_fire.is_some() => LineStatus::Fired,
        _ => LineStatus::Pending,
    };
    let removed = |i: &usize| matches!(events[*i].1, OrderEvent::LineRemoved { .. });
    if let Some(terminal) = terminal.filter(|t| first_fire.is_some() && removed(t)) {
        conflicts.push((at(terminal), ConflictKind::RemovedAfterFire(id)));
    }
    if close.is_some_and(|close| close < birth) {
        conflicts.push((at(birth), ConflictKind::AddedToClosedOrder(id)));
    }
    if let Some(fire) = first_fire.filter(|&fire| close.is_some_and(|close| close < fire)) {
        conflicts.push((at(fire), ConflictKind::FiredOnClosedOrder(id)));
    }
    // Its allocation: the last one naming it while it is live, to checks opened by then.
    let exists = |check: Id<Check>, i: usize| opened.iter().any(|&(c, at)| c == check && at < i);
    let mut unknown_checks = Vec::new();
    for &i in later.iter().filter(|&&i| live(i)) {
        let OrderEvent::LinesAllocated(allocated) = &events[i].1 else { continue };
        let (_, shares) =
            allocated_lines(allocated).into_iter().find(|(line, _)| *line == id).unwrap();
        match shares.iter().map(|share| share.check).find(|&check| !exists(check, i)) {
            Some(check) => unknown_checks.push((i, check)),
            None => line.allocation = shares,
        }
    }
    History { line, terminal, sent, unknown_checks }
}

/// What the model predicts for `events`, folded in the order given.
fn expected(events: &[(EventMeta, OrderEvent)]) -> Expected {
    let at = |i: usize| events[i].0.event_id;
    let mut conflicts = Vec::new();
    let Some(creation) =
        events.iter().position(|(_, event)| matches!(event, OrderEvent::Created(_)))
    else {
        // Nothing applies to an order that hasn't been created.
        conflicts.extend((0..events.len()).map(|i| (at(i), ConflictKind::BeforeCreation)));
        let status = OrderStatus::Active;
        return Expected { info: None, lines: Vec::new(), status, checks: Vec::new(), conflicts };
    };
    let (meta, OrderEvent::Created(created)) = &events[creation] else { unreachable!() };
    let (location, currency) = (meta.location, created.currency);
    let mut info = info_of(meta, created);

    // The events that apply: after the first creation, and at its location.
    let mut applying = Vec::new();
    for (i, (meta, event)) in events.iter().enumerate() {
        let conflict = if i == creation {
            continue;
        } else if matches!(event, OrderEvent::Created(_)) {
            ConflictKind::DuplicateCreation
        } else if i < creation {
            ConflictKind::BeforeCreation
        } else if meta.location != location {
            ConflictKind::WrongLocation
        } else {
            applying.push(i);
            continue;
        };
        conflicts.push((at(i), conflict));
    }
    let close = applying
        .iter()
        .copied()
        .find(|&i| matches!(events[i].1, OrderEvent::Voided { .. } | OrderEvent::Abandoned));
    // A line is born at the first applying addition of its identifier in the order's currency.
    let born = |line: Id<Line>| {
        applying.iter().copied().find(|&i| {
            matches!(&events[i].1, OrderEvent::LineAdded(added)
                if added.line == line && added.item.unit_price.currency() == currency)
        })
    };
    let mut births = Vec::new();
    for &i in &applying {
        match &events[i].1 {
            OrderEvent::LineAdded(added) => match born(added.line) {
                Some(birth) if birth == i => births.push(i),
                Some(birth) if birth < i => {
                    conflicts.push((at(i), ConflictKind::DuplicateLine(added.line)));
                }
                _ => conflicts.push((at(i), ConflictKind::CurrencyMismatch(added.line))),
            },
            OrderEvent::AttributesChanged(changed) => change_attributes(&mut info, changed),
            event => {
                for line in referred(event) {
                    if born(line).is_none_or(|birth| birth > i) {
                        conflicts.push((at(i), ConflictKind::UnknownLine(line)));
                    }
                }
            }
        }
    }

    // Checks: the main check comes with the creation, and each other with its first opening.
    let mut opened: Vec<(Id<Check>, usize)> = vec![(main_check(), creation)];
    for &i in &applying {
        if let OrderEvent::CheckOpened { check } = &events[i].1 {
            if opened.iter().any(|(opened, _)| opened == check) {
                conflicts.push((at(i), ConflictKind::DuplicateCheck(*check)));
            } else {
                opened.push((*check, i));
            }
        }
    }
    let checks = (1..).zip(&opened).map(|(number, &(check, _))| (check, number)).collect();

    let mut lines = Vec::new();
    // Whether, when the order closed, a line was live or had been fired: something was ordered.
    let mut ordered_at_close = false;
    // Allocations that named checks that didn't exist yet: each check once per event.
    let mut unknown_checks = BTreeSet::new();
    for &birth in &births {
        let history =
            expect_line(events, &applying, birth, currency, close, &opened, &mut conflicts);
        unknown_checks.extend(history.unknown_checks.iter().copied());
        ordered_at_close |= close.is_some_and(|close| {
            birth < close
                && (history.terminal.is_none_or(|terminal| terminal > close)
                    || history.sent.is_some_and(|sent| sent < close))
        });
        lines.push(history.line);
    }
    let status = match close.map(|close| &events[close].1) {
        Some(OrderEvent::Voided { reason }) => OrderStatus::Voided(reason.clone()),
        Some(_) => OrderStatus::Abandoned,
        None => OrderStatus::Active,
    };
    if let Some(close) = close.filter(|_| status == OrderStatus::Abandoned && ordered_at_close) {
        conflicts.push((at(close), ConflictKind::AbandonedWithLines));
    }
    conflicts.extend(
        unknown_checks.into_iter().map(|(i, check)| (at(i), ConflictKind::UnknownCheck(check))),
    );
    Expected { info: Some(info), lines, status, checks, conflicts }
}

fn sorted(conflicts: Vec<(Id<Event>, ConflictKind)>) -> Vec<(Id<Event>, String)> {
    let mut named: Vec<(Id<Event>, String)> =
        conflicts.into_iter().map(|(event, kind)| (event, format!("{kind:?}"))).collect();
    named.sort();
    named
}

/// Checks the order folded from `events` against the fold model.
fn check(order: &Order, events: &[(EventMeta, OrderEvent)]) -> Result<(), TestCaseError> {
    let expected = expected(events);
    prop_assert_eq!(order.info(), expected.info.as_ref());
    prop_assert_eq!(lines_of(order), expected.lines.clone());
    prop_assert_eq!(order.status(), &expected.status);
    prop_assert_eq!(order.stage(), stage_of(&expected.lines));
    prop_assert_eq!(checks_of(order), expected.checks);
    let actual = order.conflicts().iter().map(|conflict| (conflict.event, conflict.kind)).collect();
    prop_assert_eq!(sorted(actual), sorted(expected.conflicts));
    Ok(())
}

/// Checks what must hold of any order, whatever its events: lines are unique, priced in the
/// order's currency and of positive quantity, and nothing applies before the order is created.
fn invariants(order: &Order) -> Result<(), TestCaseError> {
    let ids: BTreeSet<Id<Line>> = order.lines().iter().map(Line::id).collect();
    prop_assert_eq!(ids.len(), order.lines().len());
    if let Some(info) = order.info() {
        for line in order.lines() {
            prop_assert_eq!(line.item().unit_price.currency(), info.currency);
            prop_assert!(
                line.modifiers().iter().all(|modifier| modifier.is_priced_in(info.currency))
            );
            prop_assert!(line.quantity().is_positive());
        }
    } else {
        prop_assert!(order.lines().is_empty());
        prop_assert_eq!(order.status(), &OrderStatus::Active);
    }
    prop_assert_eq!(order.stage() == Stage::Draft, order.live_lines().next().is_none());
    basket_holds_the_live_lines(order)?;
    checks_hold_the_lines(order)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Checks the order's checks: numbered from 1, the main check first; every live line allocated
/// to checks that exist, in ascending order of check, in shares in lowest terms; and each
/// check's basket holding its share of each line allocated to it, the checks' shares of a line
/// adding up to the whole line.
fn checks_hold_the_lines(order: &Order) -> Result<(), TestCaseError> {
    let checks = checks_of(order);
    let ids: BTreeSet<Id<Check>> = checks.iter().map(|&(check, _)| check).collect();
    prop_assert_eq!(ids.len(), checks.len());
    let numbers: Vec<u32> = checks.iter().map(|&(_, number)| number).collect();
    prop_assert_eq!(numbers, (1..).take(checks.len()).collect::<Vec<u32>>());
    prop_assert!(order.check_basket(id(0xFFFF_FFFF)).is_none());
    if order.info().is_none() {
        prop_assert!(checks.is_empty() && order.check_basket(main_check()).is_none());
        return Ok(());
    }
    prop_assert_eq!(checks.first().map(|&(check, _)| check), Some(main_check()));
    let live: Vec<&Line> = order.live_lines().collect();
    for line in &live {
        let allocation = line.allocation();
        prop_assert!(!allocation.is_empty());
        prop_assert!(
            allocation.windows(2).all(|pair| pair[0].check.to_bytes() < pair[1].check.to_bytes())
        );
        prop_assert!(allocation.iter().all(|share| ids.contains(&share.check)));
        prop_assert_eq!(
            allocation.iter().fold(0, |d, share| gcd(d, u64::from(share.shares.get()))),
            1
        );
    }
    let rules = Rules::untaxed();
    let mut parts = vec![0_i128; live.len()];
    for &(check, _) in &checks {
        let basket = order.check_basket(check).unwrap();
        let held: Vec<(usize, &Line)> = live
            .iter()
            .enumerate()
            .filter(|(_, line)| line.allocation().iter().any(|share| share.check == check))
            .map(|(k, line)| (k, *line))
            .collect();
        prop_assert_eq!(basket.lines.len(), held.len());
        for (priced, (_, line)) in basket.lines.iter().zip(&held) {
            let allocation = line.allocation();
            let share = (allocation.len() > 1).then(|| keel_pricing::Share {
                weights: allocation.iter().map(|share| u64::from(share.shares.get())).collect(),
                index: allocation.iter().position(|share| share.check == check).unwrap(),
            });
            prop_assert_eq!(&priced.share, &share);
            prop_assert_eq!(priced.unit_price, line.item().unit_price);
            prop_assert_eq!(priced.quantity, line.quantity());
            prop_assert_eq!(priced.comped, line.comp().is_some());
        }
        if let Ok(totals) = price(&basket, &rules) {
            for (priced, &(k, _)) in totals.lines.iter().zip(&held) {
                parts[k] += i128::from(priced.gross.minor());
            }
        }
    }
    if let Ok(whole) = price(&order.basket().unwrap(), &rules) {
        let wholes: Vec<i128> =
            whole.lines.iter().map(|line| i128::from(line.gross.minor())).collect();
        prop_assert_eq!(parts, wholes);
    }
    Ok(())
}

/// Checks that the order's basket, which pricing prices, holds its live lines as they stand.
fn basket_holds_the_live_lines(order: &Order) -> Result<(), TestCaseError> {
    let Some(info) = order.info() else {
        prop_assert!(order.basket().is_none());
        return Ok(());
    };
    let basket = order.basket().unwrap();
    prop_assert_eq!(basket.currency, info.currency);
    prop_assert_eq!(basket.dining == Dining::OnPremises, info.mode == Mode::DineIn);
    prop_assert!(basket.discounts.is_empty() && basket.exemptions.is_empty());
    let live: Vec<&Line> = order.live_lines().collect();
    prop_assert_eq!(basket.lines.len(), live.len());
    for (priced, line) in basket.lines.iter().zip(live) {
        prop_assert_eq!(priced.unit_price, line.item().unit_price);
        prop_assert_eq!(priced.quantity, line.quantity());
        prop_assert_eq!(priced.tax_category, line.item().tax_category.cast());
        prop_assert_eq!(priced.comped, line.comp().is_some());
        prop_assert!(priced.discounts.is_empty());
        prop_assert!(same_modifiers(&priced.modifiers, line.modifiers()));
    }
    Ok(())
}

/// Whether pricing's modifiers are the chosen ones: the same prices and quantities, all the way
/// down.
fn same_modifiers(priced: &[keel_pricing::Modifier], chosen: &[ChosenModifier]) -> bool {
    priced.len() == chosen.len()
        && priced.iter().zip(chosen).all(|(priced, chosen)| {
            priced.unit_price == chosen.unit_price
                && priced.quantity.get() == u32::from(chosen.quantity.get())
                && same_modifiers(&priced.modifiers, &chosen.modifiers)
        })
}

/// Folds `events` in the order given, after checking that each is one a kernel could decode:
/// the fold is only ever given such events.
fn folded(events: &[(EventMeta, OrderEvent)]) -> Result<Order, TestCaseError> {
    let mut order = Order::new(order_id());
    for (meta, event) in events {
        let (schema, payload) = event.encode().unwrap();
        prop_assert_eq!(&OrderEvent::decode(&schema, &payload).unwrap(), event);
        order.apply(meta, event);
    }
    Ok(order)
}

// ---------------------------------------------------------------------------------------------
// Events aimed at the conflict rules: few line identifiers, so events often meet the same line,
// and sometimes another currency, unit or location.

fn fold_line() -> impl Strategy<Value = Id<Line>> {
    (1_u64..=4).prop_map(id)
}

fn fold_currency() -> impl Strategy<Value = Currency> {
    prop_oneof![4 => Just(usd()), 1 => Just(eur())]
}

fn fold_unit() -> impl Strategy<Value = Unit> {
    prop_oneof![4 => Just(Unit::Each), 1 => Just(Unit::Kilogram)]
}

/// A change to an optional field, or none: mostly none, so changes stay small.
fn maybe<T: Clone + core::fmt::Debug + 'static>(
    value: impl Strategy<Value = T> + 'static,
) -> impl Strategy<Value = Option<Change<T>>> {
    prop_oneof![
        3 => Just(None),
        1 => Just(Some(Change::Clear)),
        2 => value.prop_map(|value| Some(Change::Set(value))),
    ]
}

/// No modifiers, or oat milk at one of two prices.
fn fold_modifiers(currency: Currency, n: u8) -> Vec<ChosenModifier> {
    match n {
        0 => Vec::new(),
        n => vec![oat_milk(Money::from_minor(i64::from(n - 1) * 50, currency))],
    }
}

fn fold_created() -> impl Strategy<Value = OrderCreated> {
    (fold_currency(), 0_u8..3, any::<bool>()).prop_map(|(currency, n, flag)| OrderCreated {
        channel: Channel::ALL[usize::from(n)],
        mode: Mode::ALL[usize::from(n)],
        currency,
        revenue_center: None,
        table: flag.then(|| id(0x200 + u64::from(n))),
        guest_count: NonZeroU16::new(1 + u16::from(n)),
        customer: None,
        owner: (!flag).then(|| id(0x300 + u64::from(n))),
    })
}

fn fold_added() -> impl Strategy<Value = LineAdded> {
    (fold_line(), fold_currency(), fold_unit(), 0_u8..3, 0_u8..3).prop_map(
        |(line, currency, unit, n, modifiers)| LineAdded {
            line,
            item: ItemSnapshot {
                variant: id(0x400 + u64::from(n)),
                catalog_version: CatalogVersion::from_bytes([1; 32]),
                name: Name::new("Flat white").unwrap(),
                tax_category: id(0x500),
                unit_price: Money::from_minor(450 + i64::from(n) * 50, currency),
            },
            quantity: quantity_of(1 + n, unit),
            modifiers: fold_modifiers(currency, modifiers),
            seat: NonZeroU16::new(u16::from(n)),
            course: None,
            notes: None,
        },
    )
}

fn fold_changed() -> impl Strategy<Value = LineChanged> {
    let note = prop::sample::select(vec!["no salt", "extra hot"])
        .prop_map(|text| Note::new(text).unwrap());
    (
        fold_line(),
        prop::option::weighted(0.4, (1_u8..4, fold_unit())),
        prop::option::weighted(0.3, (fold_currency(), 0_u8..3)),
        maybe((1_u16..4).prop_map(|n| NonZeroU16::new(n).unwrap())),
        maybe((1_u8..4).prop_map(|n| NonZeroU8::new(n).unwrap())),
        maybe(note),
    )
        .prop_map(|(line, quantity, modifiers, seat, course, notes)| LineChanged {
            line,
            quantity: quantity.map(|(n, unit)| quantity_of(n, unit)),
            modifiers: modifiers.map(|(currency, n)| fold_modifiers(currency, n)),
            seat,
            course,
            notes,
        })
        .prop_filter("a change changes something", |changed| {
            *changed != LineChanged::to(changed.line)
        })
}

fn fold_attributes() -> impl Strategy<Value = AttributesChanged> {
    (
        prop::option::weighted(0.3, prop::sample::select(Mode::ALL)),
        maybe((0_u64..2).prop_map(|n| id(0x800 + n))),
        maybe((0_u64..2).prop_map(|n| id(0x200 + n))),
        maybe((1_u16..3).prop_map(|n| NonZeroU16::new(n).unwrap())),
        maybe((0_u64..2).prop_map(|n| id(0x700 + n))),
        maybe((0_u64..2).prop_map(|n| id(0x300 + n))),
    )
        .prop_map(|(mode, revenue_center, table, guest_count, customer, owner)| AttributesChanged {
            mode,
            revenue_center,
            table,
            guest_count,
            customer,
            owner,
        })
        .prop_filter("a change changes something", |changed| {
            *changed != AttributesChanged::default()
        })
}

/// The main check, or one of three others.
fn fold_check() -> impl Strategy<Value = Id<Check>> {
    prop_oneof![2 => Just(main_check()), 3 => (1_u64..=3).prop_map(|n| id(0xC0 + n))]
}

/// Allocations of one or two of the few lines, each among one to three of the few checks, in
/// shares of one to three: mostly splits.
fn fold_allocated() -> impl Strategy<Value = LinesAllocated> {
    let checks = prop::collection::btree_map(fold_check(), 1_u16..=3, 1..=3);
    prop::collection::btree_map(fold_line(), checks, 1..=2).prop_map(|lines| {
        let allocations = lines.into_iter().flat_map(|(line, checks)| {
            checks.into_iter().map(move |(check, shares)| Allocation {
                line,
                check,
                shares: NonZeroU16::new(shares).unwrap(),
            })
        });
        LinesAllocated::new(allocations).unwrap()
    })
}

/// An event, and whether it was recorded at another location.
fn any_fold_event() -> impl Strategy<Value = (bool, OrderEvent)> {
    let event = prop_oneof![
        1 => fold_created().prop_map(OrderEvent::Created),
        6 => fold_added().prop_map(OrderEvent::LineAdded),
        3 => fold_changed().prop_map(OrderEvent::LineChanged),
        2 => fold_line().prop_map(|line| OrderEvent::LineRemoved { line }),
        3 => prop::collection::btree_set(fold_line(), 1..=2)
            .prop_map(|lines| OrderEvent::LinesFired { lines: IdSet::new(lines).unwrap() }),
        2 => (fold_line(), any::<u8>())
            .prop_map(|(line, n)| OrderEvent::LineVoided { line, reason: reason(n) }),
        2 => (fold_line(), any::<u8>())
            .prop_map(|(line, n)| OrderEvent::LineComped { line, reason: reason(n) }),
        2 => fold_attributes().prop_map(OrderEvent::AttributesChanged),
        1 => any::<u8>().prop_map(|n| OrderEvent::Voided { reason: reason(n) }),
        1 => Just(OrderEvent::Abandoned),
        3 => fold_check().prop_map(|check| OrderEvent::CheckOpened { check }),
        4 => fold_allocated().prop_map(OrderEvent::LinesAllocated),
    ];
    (prop::bool::weighted(0.08), event)
}

/// An event aimed at splits: mostly allocations, with checks opened, and lines added, removed,
/// voided, fired and comped between them.
fn any_split_event() -> impl Strategy<Value = OrderEvent> {
    prop_oneof![
        8 => fold_allocated().prop_map(OrderEvent::LinesAllocated),
        2 => fold_check().prop_map(|check| OrderEvent::CheckOpened { check }),
        1 => fold_added().prop_map(OrderEvent::LineAdded),
        1 => fold_line().prop_map(|line| OrderEvent::LineRemoved { line }),
        1 => (fold_line(), any::<u8>())
            .prop_map(|(line, n)| OrderEvent::LineVoided { line, reason: reason(n) }),
        1 => fold_line().prop_map(|line| OrderEvent::LinesFired { lines: IdSet::new([line]).unwrap() }),
        1 => (fold_line(), any::<u8>())
            .prop_map(|(line, n)| OrderEvent::LineComped { line, reason: reason(n) }),
    ]
}

/// Events numbered in the order given, recorded by one device at `location()`, or at another
/// location when flagged.
fn numbered(events: Vec<(bool, OrderEvent)>) -> Vec<(EventMeta, OrderEvent)> {
    (1..)
        .zip(events)
        .map(|(seq, (elsewhere, event))| {
            let at = if elsewhere { other_location() } else { location() };
            (meta_at(1, seq, 1_000 + seq, at), event)
        })
        .collect()
}

/// Signs events in one device's log, each for its stream: an order's identifier, and the stream
/// kind.
fn signed(events: &[(Id<Order>, &str, OrderEvent)]) -> Vec<SignedEvent> {
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
    /// Any events, in any order, fold without panicking into exactly the order the fold model
    /// predicts, and the order keeps its invariants. The events come from the wide generators,
    /// so any value the payloads allow takes part.
    #[test]
    fn the_fold_matches_the_model_for_any_events(
        events in prop::collection::vec(any_event(), 0..40),
    ) {
        let events = numbered(events.into_iter().map(|event| (false, event)).collect());
        let order = folded(&events)?;
        check(&order, &events)?;
        invariants(&order)?;
    }

    /// The same for events aimed at the conflict rules. Most orders are created early, after a
    /// few events that came before the creation; some are created again, or never.
    #[test]
    fn the_fold_matches_the_model_on_colliding_events(
        before in prop::collection::vec(any_fold_event(), 0..4),
        creation in prop::option::weighted(0.9, (prop::bool::weighted(0.08), fold_created())),
        after in prop::collection::vec(any_fold_event(), 0..40),
    ) {
        let creation = creation.map(|(elsewhere, created)| (elsewhere, OrderEvent::Created(created)));
        let events = numbered(before.into_iter().chain(creation).chain(after).collect());
        let order = folded(&events)?;
        check(&order, &events)?;
        invariants(&order)?;
    }

    /// The same for splits: an order with lines and checks, then allocations of its lines among
    /// its checks, some naming checks opened later or never, between openings, removals, voids,
    /// fires and comps.
    #[test]
    fn the_fold_matches_the_model_on_splits(
        added in prop::collection::vec(fold_added(), 1..=4),
        opened in prop::collection::btree_set((1_u64..=3).prop_map(|n| id::<Check>(0xC0 + n)), 0..=3),
        after in prop::collection::vec(any_split_event(), 0..24),
    ) {
        let created = OrderEvent::Created(OrderCreated { currency: usd(), ..created() });
        let events: Vec<OrderEvent> = core::iter::once(created)
            .chain(added.into_iter().map(OrderEvent::LineAdded))
            .chain(opened.into_iter().map(|check| OrderEvent::CheckOpened { check }))
            .chain(after)
            .collect();
        let events = numbered(events.into_iter().map(|event| (false, event)).collect());
        let order = folded(&events)?;
        check(&order, &events)?;
        invariants(&order)?;
    }

    /// Signed events fold into their own aggregate only: the events of another order, or of
    /// another kind of stream with the same identifier, are skipped, and the order is exactly
    /// what its own events make it.
    #[test]
    fn folds_take_only_their_own_streams_events(
        events in prop::collection::vec((0_u8..4, any_fold_event()), 0..24),
    ) {
        let events: Vec<(Id<Order>, &str, OrderEvent)> = events
            .into_iter()
            .map(|(stream, (_, event))| match stream {
                2 => (id(0xB), "order", event),
                3 => (order_id(), "payment", event),
                _ => (order_id(), "order", event),
            })
            .collect();
        let mut order = Order::new(order_id());
        let mut own = Order::new(order_id());
        let mut others = Vec::new();
        for (signed, (stream, kind, event)) in signed(&events).iter().zip(&events) {
            fold(&mut order, signed);
            if *stream == order_id() && *kind == "order" {
                own.apply(&EventMeta::of(signed.body()), event);
            } else {
                others.push(signed.body().event_id);
            }
        }
        prop_assert_eq!(order.info(), own.info());
        prop_assert_eq!(order.lines(), own.lines());
        prop_assert_eq!(order.status(), own.status());
        prop_assert_eq!(order.checks(), own.checks());
        prop_assert_eq!(order.conflicts(), own.conflicts());
        let skipped: Vec<Id<Event>> = order.skipped().iter().map(|skipped| skipped.event).collect();
        prop_assert_eq!(skipped, others);
        prop_assert!(order.skipped().iter().all(|skipped| skipped.reason == DecodeError::WrongStream));
    }

    /// One device's commands, faulty ones included, are accepted exactly when the model allows
    /// them. Accepted commands never cause a conflict, and leave the order as the model
    /// predicts.
    #[test]
    fn commands_do_what_the_model_says(steps in prop::collection::vec(any_step(), 1..60)) {
        let mut device = Working::new(1, Order::new(order_id()), 1_000);
        prop_assert!(device.run(OrderCommand::Create(created()), location(), 0));
        let mut model = Model::new(info_of(&device.log[0].0, &created()));
        for step in &steps {
            let (command, from) =
                command(&device.view, step, device.fresh(), device.fresh_check());
            let allowed = model.allows(&command, from);
            let accepted = device.run(command.clone(), from, 0);
            prop_assert_eq!(accepted, allowed, "{:?} from {:?}", command, from);
            if accepted {
                model.apply(&command);
            }
            prop_assert!(device.view.conflicts().is_empty());
            prop_assert_eq!(device.view.info(), Some(&model.info));
            prop_assert_eq!(lines_of(&device.view), model.lines.clone());
            prop_assert_eq!(device.view.status(), &model.status);
            prop_assert_eq!(device.view.stage(), stage_of(&model.lines));
            let checks: Vec<Id<Check>> = checks_of(&device.view).into_iter().map(|(check, _)| check).collect();
            prop_assert_eq!(checks, model.checks.clone());
        }
    }

    /// Devices acting at once on stale views of the order: once their events are merged in
    /// canonical order, the order is exactly what the fold model predicts, conflicts included.
    #[test]
    fn concurrent_devices_converge_as_the_model_says(
        prefix in prop::collection::vec(any_step(), 0..12),
        devices in prop::collection::vec(prop::collection::vec(any_step(), 0..16), 2..=3),
    ) {
        let mut first = Working::new(1, Order::new(order_id()), 1_000);
        prop_assert!(first.run(OrderCommand::Create(created()), location(), 0));
        for step in &prefix {
            first.step(step);
        }
        let shared = first.view.clone();
        let start = first.hlc;
        let mut events = first.log.clone();
        for (number, steps) in (2..).zip(&devices) {
            let mut device = Working::new(number, shared.clone(), start);
            for step in steps {
                device.step(step);
            }
            events.extend(device.log);
        }
        let (order, events) = replica(events);
        check(&order, &events)?;
    }
}
