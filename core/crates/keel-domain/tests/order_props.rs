//! Property tests for the order aggregate, against independent models:
//! - the fold, given any events in any order, leaves the order exactly as a model of the fold
//!   predicts, conflicts included. The model is written as queries over the events' positions,
//!   with one pass over them for the checks and what each line is allocated to, rather than as a
//!   state machine, so it shares no structure with the fold;
//! - folding signed events takes only the aggregate's own stream;
//! - a device's commands, including faulty ones, are refused exactly when a model of the rules
//!   refuses them, and for the same reason, and its closes of checks, through checkout once its
//!   payments cover them, are accepted exactly when the model allows them; an accepted command
//!   never causes a conflict, and has the effect the model predicts;
//! - when devices act concurrently on stale views, the merged order is what the fold model
//!   predicts. Commands on it, and on every state the merge passed through, are accepted
//!   exactly when the command model allows them, and a device carrying on with it does what the
//!   model says: a merge reaches states no single device does;
//! - every check's basket holds its share of each line it is allocated, the checks' shares of a
//!   line add up to the line, and a line's parts on closed checks were charged by them.

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

use std::collections::BTreeMap;

use keel_domain::aggregate::{Aggregate, EventMeta, fold};
use keel_domain::checkout::Checkout;
use keel_domain::codec::{CatalogVersion, Change, IdSet, Name, Note, ReasonCode, RulesVersion};
use keel_domain::order::{
    Allocation, AttributesChanged, Channel, Check, CheckClosed, CheckShare, ChosenModifier,
    CommandError, ConflictKind, ItemSnapshot, Line, LineAdded, LineChanged, LineCharge, LineStatus,
    LinesAllocated, Mode, Order, OrderCommand, OrderCreated, OrderEvent, OrderInfo, OrderStatus,
    Placement, Prefix, Reason, Stage,
};
use keel_domain::payment::{CashTendered, Payment, PaymentCaptured, PaymentCommand, Tender};
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

/// A step for a device playing a part in a race that closes or ends the order (see `action`
/// for the kinds): adding lines, opening checks and moving lines (0); closing checks and the
/// order, or reopening it (1); firing, voiding and moving lines (2); or removing lines and
/// abandoning or voiding the order (3).
fn racing_step(part: u8) -> BoxedStrategy<Step> {
    let kinds: Vec<(u32, u8)> = match part {
        0 => vec![(4, 0), (1, 40), (1, 42)],
        1 => vec![(4, 48), (1, 52), (1, 55)],
        2 => vec![(3, 18), (1, 24), (1, 42)],
        _ => vec![(4, 14), (1, 39), (1, 38)],
    };
    let kind = prop::strategy::Union::new_weighted(
        kinds.into_iter().map(|(weight, kind)| (weight, Just(kind))).collect(),
    );
    (any_step(), kind).prop_map(|(step, kind)| Step { kind, ..step }).boxed()
}

/// A device's steps in a race: a part, and one to five steps playing it.
fn racer() -> impl Strategy<Value = Vec<Step>> {
    (1_u8..=3).prop_flat_map(|part| prop::collection::vec(racing_step(part), 1..6))
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

/// What a device does: an order command, or closing a check through checkout, once it has paid
/// what the check owes.
#[derive(Clone, Debug)]
enum Action {
    Order(OrderCommand),
    CloseCheck(Id<Check>),
}

/// What a step stands for on the device's view, and the location it comes from: mostly line
/// work, sometimes the attributes or the checks, rarely voiding, closing or reopening the order.
/// Lines are picked among those the device knows, or a fresh identifier when it knows none.
/// Fault 1 adds a line that exists, opens a check that exists, or works on a line or check that
/// doesn't; fault 5 acts from another location.
fn action(
    view: &Order,
    step: &Step,
    fresh: Id<Line>,
    fresh_check: Id<Check>,
) -> (Action, Id<Location>) {
    let lines: Vec<Id<Line>> = view.lines().iter().map(Line::id).collect();
    let known =
        |index: &Index| if lines.is_empty() { fresh } else { lines[index.index(lines.len())] };
    let pick = |index: &Index| if step.fault == 1 { fresh } else { known(index) };
    let from = if step.fault == 5 { other_location() } else { location() };
    let command = match (step.kind % 56, step.fault) {
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
        (42..=47, _) => OrderCommand::AllocateLines(allocation(view, step, fresh, fresh_check)),
        (48..=51, 1) => return (Action::CloseCheck(fresh_check), from),
        (48..=51, _) => {
            let checks = view.checks();
            let picked = checks.get(step.pick.index(checks.len().max(1))).map(Check::id);
            return (Action::CloseCheck(picked.unwrap_or(fresh_check)), from);
        }
        (52..=53, _) => OrderCommand::Close,
        _ => OrderCommand::Reopen(reason(step.amount)),
    };
    (Action::Order(command), from)
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

/// A check as the models see it: its identifier, number, and the lines it was charged for, if
/// it is closed.
type ModelCheck = (Id<Check>, u32, Option<Vec<Id<Line>>>);

/// The order's checks, in the models' terms.
fn checks_of(order: &Order) -> Vec<ModelCheck> {
    order
        .checks()
        .iter()
        .map(|check| {
            let charged =
                check.closed().map(|closed| closed.lines.iter().map(|c| c.line).collect());
            (check.id(), check.number().get(), charged)
        })
        .collect()
}

/// A check, and the lines it was charged for once closed.
type Opened = (Id<Check>, Option<Vec<Id<Line>>>);

/// Checks numbered in the order they were opened.
fn with_numbers(checks: &[Opened]) -> Vec<ModelCheck> {
    (1..).zip(checks).map(|(number, (check, charged))| (*check, number, charged.clone())).collect()
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
    /// Each check, in the order it was opened, with the lines it was charged for once closed.
    checks: Vec<Opened>,
}

/// Whether every price in `modifiers`, at any depth, is zero or more.
fn non_negative(modifiers: &[ChosenModifier]) -> bool {
    modifiers
        .iter()
        .all(|modifier| !modifier.unit_price.is_negative() && non_negative(&modifier.modifiers))
}

/// Why a command is refused, as the model sees it: `CommandError`'s kinds, with the lines and
/// checks they name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    AlreadyCreated,
    WrongLocation,
    OrderClosed,
    NoChange,
    LineExists(Id<Line>),
    UnknownLine(Id<Line>),
    LineNotPending(Id<Line>),
    LineNotFired(Id<Line>),
    LineNotLive(Id<Line>),
    AlreadyComped(Id<Line>),
    WrongCurrency,
    WrongUnit(Id<Line>),
    HasLiveLines,
    LineWasFired(Id<Line>),
    CheckExists(Id<Check>),
    UnknownCheck(Id<Check>),
    CheckClosed(Id<Check>),
    CheckOpen(Id<Check>),
    LineOnClosedCheck(Id<Line>),
    NothingToClose,
    NothingToReopen,
    /// The event wouldn't satisfy its schema, such as a negative price or a zero quantity.
    Invalid,
}

fn refusal_of(error: &CommandError) -> Refusal {
    match *error {
        CommandError::AlreadyCreated => Refusal::AlreadyCreated,
        CommandError::WrongLocation => Refusal::WrongLocation,
        CommandError::OrderClosed => Refusal::OrderClosed,
        CommandError::NoChange => Refusal::NoChange,
        CommandError::LineExists(line) => Refusal::LineExists(line),
        CommandError::UnknownLine(line) => Refusal::UnknownLine(line),
        CommandError::LineNotPending(line) => Refusal::LineNotPending(line),
        CommandError::LineNotFired(line) => Refusal::LineNotFired(line),
        CommandError::LineNotLive(line) => Refusal::LineNotLive(line),
        CommandError::AlreadyComped(line) => Refusal::AlreadyComped(line),
        CommandError::WrongCurrency => Refusal::WrongCurrency,
        CommandError::WrongUnit(line) => Refusal::WrongUnit(line),
        CommandError::HasLiveLines => Refusal::HasLiveLines,
        CommandError::LineWasFired(line) => Refusal::LineWasFired(line),
        CommandError::CheckExists(check) => Refusal::CheckExists(check),
        CommandError::UnknownCheck(check) => Refusal::UnknownCheck(check),
        CommandError::CheckClosed(check) => Refusal::CheckClosed(check),
        CommandError::CheckOpen(check) => Refusal::CheckOpen(check),
        CommandError::LineOnClosedCheck(line) => Refusal::LineOnClosedCheck(line),
        CommandError::NothingToClose => Refusal::NothingToClose,
        CommandError::NothingToReopen => Refusal::NothingToReopen,
        CommandError::Invalid(_) => Refusal::Invalid,
        ref other => panic!("no command here should be refused with {other:?}"),
    }
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
        Model {
            info,
            lines: Vec::new(),
            status: OrderStatus::Active,
            checks: vec![(main_check(), None)],
        }
    }

    /// The model of `order` as it stands, however its events came together: for commands on a
    /// view that concurrent events were merged into.
    fn of(order: &Order) -> Model {
        Model {
            info: order.info().unwrap().clone(),
            lines: lines_of(order),
            status: order.status().clone(),
            checks: checks_of(order)
                .into_iter()
                .map(|(check, _, charged)| (check, charged))
                .collect(),
        }
    }

    fn exists(&self, check: Id<Check>) -> bool {
        self.checks.iter().any(|(known, _)| *known == check)
    }

    fn is_open(&self, check: Id<Check>) -> bool {
        self.checks.iter().any(|(known, charged)| *known == check && charged.is_none())
    }

    /// A live line with a part on a closed check: what it costs is frozen.
    fn frozen(&self, line: &ModelLine) -> bool {
        line.live() && line.allocation.iter().any(|share| !self.is_open(share.check))
    }

    /// The live lines with a part on `check`.
    fn on(&self, check: Id<Check>) -> Vec<Id<Line>> {
        let on =
            |line: &&ModelLine| line.live() && line.allocation.iter().any(|s| s.check == check);
        self.lines.iter().filter(on).map(|line| line.id).collect()
    }

    fn line(&self, id: Id<Line>) -> Option<&ModelLine> {
        self.lines.iter().find(|line| line.id == id)
    }

    fn line_mut(&mut self, id: Id<Line>) -> &mut ModelLine {
        self.lines.iter_mut().find(|line| line.id == id).unwrap()
    }

    /// Whether the model allows `action` from `from`. Closing a check goes through checkout
    /// once the device has paid what it owes: an open check with live lines closes.
    fn allows(&self, action: &Action, from: Id<Location>) -> bool {
        match action {
            Action::CloseCheck(check) => {
                from == self.info.location
                    && self.status == OrderStatus::Active
                    && self.is_open(*check)
                    && !self.on(*check).is_empty()
            }
            Action::Order(command) => self.refusal(command, from).is_none(),
        }
    }

    /// Why the model refuses `command` from `from`: the first rule it breaks, taking the rules
    /// in the order the kernel checks them (the order and the device's location, then the
    /// command's own rules, then the event's schema), or `None` if it is allowed.
    fn refusal(&self, command: &OrderCommand, from: Id<Location>) -> Option<Refusal> {
        if let OrderCommand::Create(_) = command {
            return Some(Refusal::AlreadyCreated);
        }
        if from != self.info.location {
            return Some(Refusal::WrongLocation);
        }
        // A closed order takes only a reopening; an active one, when a check is closed.
        match (&self.status, command) {
            (OrderStatus::Closed, OrderCommand::Reopen(_)) => None,
            (OrderStatus::Active, _) => self.refused(command).err(),
            _ => Some(Refusal::OrderClosed),
        }
    }

    /// The command's own rules, on an active order.
    fn refused(&self, command: &OrderCommand) -> Result<(), Refusal> {
        let line = |id: Id<Line>| self.line(id).ok_or(Refusal::UnknownLine(id));
        let pending = |id: Id<Line>| {
            line(id).and_then(|line| {
                (line.status == LineStatus::Pending)
                    .then_some(line)
                    .ok_or(Refusal::LineNotPending(id))
            })
        };
        let unfrozen = |line: &ModelLine| {
            if self.frozen(line) { Err(Refusal::LineOnClosedCheck(line.id)) } else { Ok(()) }
        };
        let first_closed =
            self.checks.iter().find(|(_, charged)| charged.is_some()).map(|(check, _)| *check);
        match command {
            OrderCommand::Create(_) => Err(Refusal::AlreadyCreated),
            OrderCommand::Reopen(_) => first_closed.map(|_| ()).ok_or(Refusal::NothingToReopen),
            OrderCommand::ChangeAttributes(changed) => {
                let current = &self.info;
                let changes = *changed != AttributesChanged::default()
                    && changed.mode.is_none_or(|mode| mode != current.mode)
                    && differs(current.revenue_center.as_ref(), changed.revenue_center.as_ref())
                    && differs(current.table.as_ref(), changed.table.as_ref())
                    && differs(current.guest_count.as_ref(), changed.guest_count.as_ref())
                    && differs(current.customer.as_ref(), changed.customer.as_ref())
                    && differs(current.owner.as_ref(), changed.owner.as_ref());
                if changes { Ok(()) } else { Err(Refusal::NoChange) }
            }
            OrderCommand::AddLine(added) => {
                let price = added.item.unit_price;
                if self.line(added.line).is_some() {
                    Err(Refusal::LineExists(added.line))
                } else if price.currency() != usd() || !priced_in(&added.modifiers, usd()) {
                    Err(Refusal::WrongCurrency)
                } else if price.is_negative()
                    || !non_negative(&added.modifiers)
                    || !added.quantity.is_positive()
                {
                    Err(Refusal::Invalid)
                } else {
                    Ok(())
                }
            }
            OrderCommand::ChangeLine(changed) => {
                self.change_refused(pending(changed.line)?, changed)
            }
            OrderCommand::RemoveLine(id) => unfrozen(pending(*id)?),
            OrderCommand::FireLines(lines) => {
                lines.iter().try_for_each(|id| pending(id).map(|_| ()))
            }
            OrderCommand::VoidLine { line: id, .. } => {
                let found = line(*id)?;
                if found.status != LineStatus::Fired {
                    return Err(Refusal::LineNotFired(*id));
                }
                unfrozen(found)
            }
            OrderCommand::CompLine { line: id, .. } => {
                let found = line(*id)?;
                if !found.live() {
                    Err(Refusal::LineNotLive(*id))
                } else if found.comp.is_some() {
                    Err(Refusal::AlreadyComped(*id))
                } else {
                    unfrozen(found)
                }
            }
            // A closed check was paid: reopen the order first.
            OrderCommand::Void(_) => {
                first_closed.map_or(Ok(()), |check| Err(Refusal::CheckClosed(check)))
            }
            // Nothing may have been made: every line was removed before it was fired.
            OrderCommand::Abandon => {
                if self.lines.iter().any(ModelLine::live) {
                    Err(Refusal::HasLiveLines)
                } else if let Some(fired) = self.lines.iter().find(|line| line.fired) {
                    Err(Refusal::LineWasFired(fired.id))
                } else {
                    first_closed.map_or(Ok(()), |check| Err(Refusal::CheckClosed(check)))
                }
            }
            OrderCommand::OpenCheck(check) => {
                if self.exists(*check) {
                    Err(Refusal::CheckExists(*check))
                } else {
                    Ok(())
                }
            }
            OrderCommand::AllocateLines(allocated) => self.allocation_refused(allocated),
            // Something is live, and every check holding a live line is closed.
            OrderCommand::Close => {
                if !self.lines.iter().any(ModelLine::live) {
                    return Err(Refusal::NothingToClose);
                }
                let open = self
                    .checks
                    .iter()
                    .find(|(check, charged)| charged.is_none() && !self.on(*check).is_empty());
                open.map_or(Ok(()), |(check, _)| Err(Refusal::CheckOpen(*check)))
            }
        }
    }

    /// An allocation: every line named is live, to existing, open checks, not frozen, and gets a
    /// new allocation.
    fn allocation_refused(&self, allocated: &LinesAllocated) -> Result<(), Refusal> {
        for (id, shares) in allocated_lines(allocated) {
            let found = self.line(id).ok_or(Refusal::UnknownLine(id))?;
            if !found.live() {
                return Err(Refusal::LineNotLive(id));
            }
            for share in &shares {
                if !self.exists(share.check) {
                    return Err(Refusal::UnknownCheck(share.check));
                }
                if !self.is_open(share.check) {
                    return Err(Refusal::CheckClosed(share.check));
                }
            }
            if self.frozen(found) {
                return Err(Refusal::LineOnClosedCheck(id));
            }
            if found.allocation == shares {
                return Err(Refusal::NoChange);
            }
        }
        Ok(())
    }

    /// A pending line's change: every field given changes it; a quantity stays in the line's
    /// unit, and modifiers in the order's currency; what the line costs is not frozen; and the
    /// quantity stays positive, and the modifiers' prices zero or more.
    fn change_refused(&self, line: &ModelLine, changed: &LineChanged) -> Result<(), Refusal> {
        let changes = *changed != LineChanged::to(line.id)
            && changed.quantity.is_none_or(|quantity| quantity != line.quantity)
            && changed.modifiers.as_ref().is_none_or(|modifiers| *modifiers != line.modifiers)
            && differs(line.seat.as_ref(), changed.seat.as_ref())
            && differs(line.course.as_ref(), changed.course.as_ref())
            && differs(line.notes.as_ref(), changed.notes.as_ref());
        // Seats, courses and notes don't change what a line costs.
        let costs = changed.quantity.is_some() || changed.modifiers.is_some();
        if !changes {
            Err(Refusal::NoChange)
        } else if changed.quantity.is_some_and(|quantity| quantity.unit() != line.quantity.unit()) {
            Err(Refusal::WrongUnit(line.id))
        } else if changed.modifiers.as_ref().is_some_and(|modifiers| !priced_in(modifiers, usd())) {
            Err(Refusal::WrongCurrency)
        } else if costs && self.frozen(line) {
            Err(Refusal::LineOnClosedCheck(line.id))
        } else if changed.quantity.is_some_and(|quantity| !quantity.is_positive())
            || changed.modifiers.as_ref().is_some_and(|modifiers| !non_negative(modifiers))
        {
            Err(Refusal::Invalid)
        } else {
            Ok(())
        }
    }

    /// Applies an accepted action, whose event had identifier `event`.
    fn apply(&mut self, action: &Action, event: Id<Event>) {
        let command = match action {
            Action::CloseCheck(check) => {
                let mut charged = self.on(*check);
                charged.sort();
                for (known, closed) in &mut self.checks {
                    if known == check {
                        *closed = Some(charged.clone());
                    }
                }
                return;
            }
            Action::Order(command) => command,
        };
        match command {
            OrderCommand::Create(_) => {}
            // A new line goes to the first open check, or to a new one with the event's
            // identifier.
            OrderCommand::AddLine(added) => {
                let open = self.checks.iter().find(|(_, charged)| charged.is_none());
                let check = if let Some((check, _)) = open {
                    *check
                } else {
                    self.checks.push((event.cast(), None));
                    event.cast()
                };
                let mut line = ModelLine::new(added);
                line.allocation = vec![whole(check)];
                self.lines.push(line);
            }
            OrderCommand::Close => self.status = OrderStatus::Closed,
            OrderCommand::Reopen(_) => {
                self.status = OrderStatus::Active;
                for (_, charged) in &mut self.checks {
                    *charged = None;
                }
            }
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
            OrderCommand::OpenCheck(check) => self.checks.push((*check, None)),
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

/// A device working on its own view of the order, and on the payments it took.
#[derive(Clone)]
struct Working {
    device: u64,
    view: Order,
    payments: Vec<Payment>,
    seq: u64,
    hlc: u64,
    lines_minted: u64,
    checks_minted: u64,
    payments_minted: u64,
    log: Vec<(EventMeta, OrderEvent)>,
}

impl Working {
    fn new(device: u64, view: Order, hlc: u64) -> Working {
        Working {
            device,
            view,
            payments: Vec::new(),
            seq: 0,
            hlc,
            lines_minted: 0,
            checks_minted: 0,
            payments_minted: 0,
            log: Vec::new(),
        }
    }

    /// An identifier for the device's next line, which no other device uses.
    fn fresh(&self) -> Id<Line> {
        id((self.device << 16) + 0x1000 + self.lines_minted)
    }

    /// An identifier for the device's next check, which no other device uses.
    fn fresh_check(&self) -> Id<Check> {
        id((self.device << 16) + 0x2000 + self.checks_minted)
    }

    /// An identifier for the device's next payment, which no other device uses.
    fn fresh_payment(&self) -> Id<Payment> {
        id((self.device << 16) + 0x3000 + self.payments_minted)
    }

    fn next_meta(&mut self, from: Id<Location>, delay: u64) -> EventMeta {
        self.seq += 1;
        self.hlc += 1 + delay;
        meta_at(self.device, self.seq, self.hlc, from)
    }

    /// Runs a step; returns whether its action was accepted.
    fn step(&mut self, step: &Step) -> bool {
        let (action, from) = action(&self.view, step, self.fresh(), self.fresh_check());
        self.run(action, from, u64::from(step.amount % 3))
    }

    fn run(&mut self, action: Action, from: Id<Location>, delay: u64) -> bool {
        let command = match action {
            Action::CloseCheck(check) => return self.settle(check, from, delay),
            Action::Order(command) => command,
        };
        let adds = matches!(command, OrderCommand::AddLine(_));
        let opens = matches!(command, OrderCommand::OpenCheck(_));
        let Ok(event) = self.view.decide(from, command) else { return false };
        if adds {
            self.lines_minted += 1;
        }
        if opens {
            self.checks_minted += 1;
        }
        self.record(event, from, delay);
        true
    }

    fn record(&mut self, event: OrderEvent, from: Id<Location>, delay: u64) {
        let meta = self.next_meta(from, delay);
        self.view.apply(&meta, &event);
        self.log.push((meta, event));
    }

    /// Pays what `check` owes, in cash, and closes it through checkout: the payment's events go
    /// to the device's payments, and the close to the order. Returns whether the check closed.
    fn settle(&mut self, check: Id<Check>, from: Id<Location>, delay: u64) -> bool {
        let rules = Rules::untaxed();
        let version = RulesVersion::from_bytes([1; 32]);
        let Ok(balance) = Checkout::new(&self.view, &self.payments, &rules, version).balance(check)
        else {
            return false;
        };
        if balance.due.is_positive() {
            let id = self.fresh_payment();
            let checkout = Checkout::new(&self.view, &self.payments, &rules, version);
            let Ok(started) = checkout.start_payment(from, id, check, Tender::Cash, balance.due)
            else {
                return false;
            };
            self.payments_minted += 1;
            let mut payment = Payment::new(id);
            let meta = self.next_meta(from, delay);
            payment.apply(&meta, &started);
            let cash = CashTendered { tendered: balance.due, rounding: None };
            let capture = PaymentCaptured {
                amount: balance.due,
                tip: None,
                reference: None,
                cash: Some(cash),
            };
            let captured = payment.decide(from, PaymentCommand::Capture(capture)).unwrap();
            let meta = self.next_meta(from, 0);
            payment.apply(&meta, &captured);
            self.payments.push(payment);
        }
        let checkout = Checkout::new(&self.view, &self.payments, &rules, version);
        let Ok(event) = checkout.close_check(from, check) else { return false };
        self.record(event, from, delay);
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

/// What devices acting at once left: their events merged in canonical order, the order folded
/// from them, every payment they took, and the latest reading of their clocks.
struct Merged {
    order: Order,
    events: Vec<(EventMeta, OrderEvent)>,
    payments: Vec<Payment>,
    latest: u64,
}

/// Device 1 creates the order and works through `prefix`; then devices 2 and on each work
/// through their steps at once, from the view device 1 left.
fn merge(prefix: &[Step], devices: &[Vec<Step>]) -> Merged {
    let mut first = Working::new(1, Order::new(order_id()), 1_000);
    assert!(first.run(Action::Order(OrderCommand::Create(created())), location(), 0));
    for step in prefix {
        first.step(step);
    }
    let start = first.hlc;
    let mut events = first.log.clone();
    let mut payments = first.payments.clone();
    let mut latest = start;
    for (number, steps) in (2..).zip(devices) {
        // Each device starts from the shared view, with the payments taken so far.
        let mut device = Working::new(number, first.view.clone(), start);
        device.payments.clone_from(&first.payments);
        for step in steps {
            device.step(step);
        }
        latest = latest.max(device.hlc);
        events.extend(device.log);
        payments.extend(device.payments.split_off(first.payments.len()));
    }
    let (order, events) = replica(events);
    Merged { order, events, payments, latest }
}

/// Commands on the whole order: ending, closing and reopening it.
fn order_wide() -> Vec<OrderCommand> {
    vec![
        OrderCommand::Void(reason(0)),
        OrderCommand::Abandon,
        OrderCommand::Close,
        OrderCommand::Reopen(reason(1)),
    ]
}

/// Commands to try on an order: the order-wide ones, and removing, firing, voiding, comping,
/// changing and moving each of its lines.
fn battery(order: &Order) -> Vec<OrderCommand> {
    let mut commands = order_wide();
    for line in order.lines() {
        let id = line.id();
        let mut quantity = LineChanged::to(id);
        quantity.quantity = Some(quantity_of(9, line.quantity().unit()));
        let mut seat = LineChanged::to(id);
        seat.seat = Some(Change::Set(NonZeroU16::new(9).unwrap()));
        commands.extend([
            OrderCommand::RemoveLine(id),
            OrderCommand::FireLines(IdSet::new([id]).unwrap()),
            OrderCommand::VoidLine { line: id, reason: reason(2) },
            OrderCommand::CompLine { line: id, reason: reason(3) },
            OrderCommand::ChangeLine(quantity),
            OrderCommand::ChangeLine(seat),
        ]);
        for check in order.checks() {
            let moved = Allocation { line: id, check: check.id(), shares: NonZeroU16::MIN };
            commands.push(OrderCommand::AllocateLines(LinesAllocated::new([moved]).unwrap()));
        }
    }
    commands
}

/// For an order command, checks that `order` refuses it exactly when `model` does, and for the
/// same reason, without recording it.
fn check_refusal(
    order: &Order,
    model: &Model,
    action: &Action,
    from: Id<Location>,
) -> Result<(), TestCaseError> {
    if let Action::Order(command) = action {
        let refused = order.decide(from, command.clone()).err().map(|error| refusal_of(&error));
        prop_assert_eq!(refused, model.refusal(command, from), "{:?} from {:?}", command, from);
    }
    Ok(())
}

/// Checks `commands` on `order`, from the order's location and another, against the model of
/// it, without recording them.
fn check_commands(order: &Order, commands: Vec<OrderCommand>) -> Result<(), TestCaseError> {
    let model = Model::of(order);
    for command in commands {
        let action = Action::Order(command);
        check_refusal(order, &model, &action, location())?;
        check_refusal(order, &model, &action, other_location())?;
    }
    Ok(())
}

/// Checks the order-wide commands on every state a merged order passed through, and the whole
/// battery on the last. A merge passes through states no single device reaches, such as a
/// closed check whose lines another device removed, and the rules must hold there too.
fn check_commands_on(merged: &Merged) -> Result<(), TestCaseError> {
    let mut order = Order::new(order_id());
    for (meta, event) in &merged.events {
        order.apply(meta, event);
        check_commands(&order, order_wide())?;
    }
    check_commands(&order, battery(&order))
}

/// A device carrying on with a merged order, knowing every payment: each of its actions is
/// accepted exactly when the model of the order it sees allows it, causes no conflict, and
/// leaves the order as the model predicts.
fn carry_on(merged: &Merged, steps: &[Step]) -> Result<(), TestCaseError> {
    let mut device = Working::new(9, merged.order.clone(), merged.latest);
    device.payments.clone_from(&merged.payments);
    for step in steps {
        let (action, from) = action(&device.view, step, device.fresh(), device.fresh_check());
        let mut model = Model::of(&device.view);
        check_refusal(&device.view, &model, &action, from)?;
        let allowed = model.allows(&action, from);
        let conflicts = device.view.conflicts().len();
        let accepted = device.run(action.clone(), from, 0);
        prop_assert_eq!(accepted, allowed, "{:?} from {:?}", action, from);
        if accepted {
            model.apply(&action, device.log.last().unwrap().0.event_id);
        }
        prop_assert_eq!(device.view.conflicts().len(), conflicts, "{:?}", action);
        prop_assert_eq!(lines_of(&device.view), model.lines.clone());
        prop_assert_eq!(device.view.status(), &model.status);
        prop_assert_eq!(checks_of(&device.view), with_numbers(&model.checks));
    }
    invariants(&device.view)
}

// ---------------------------------------------------------------------------------------------
// An independent model of the fold, as queries over the events' positions.

/// What the fold model predicts.
struct Expected {
    info: Option<OrderInfo>,
    lines: Vec<ModelLine>,
    status: OrderStatus,
    checks: Vec<ModelCheck>,
    conflicts: Vec<(Id<Event>, ConflictKind)>,
}

/// The lines an event refers to, other than by adding them or charging for them.
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

/// When the order is closed, voided or abandoned: queries over the positions of the events that
/// apply.
struct Timeline<'a> {
    events: &'a [(EventMeta, OrderEvent)],
    applying: &'a [usize],
    /// The first void or abandonment, where the order ends.
    end: Option<usize>,
}

impl Timeline<'_> {
    /// Whether the order ended before position `i`.
    fn ended(&self, i: usize) -> bool {
        self.end.is_some_and(|end| end < i)
    }

    /// Whether the order is closed just before position `i`: its last close or reopening before
    /// then, and before it ended, was a close.
    fn closed(&self, i: usize) -> bool {
        let last = self
            .applying
            .iter()
            .copied()
            .filter(|&j| j < i && self.end.is_none_or(|end| j < end))
            .rfind(|&j| {
                matches!(self.events[j].1, OrderEvent::Closed | OrderEvent::Reopened { .. })
            });
        last.is_some_and(|j| matches!(self.events[j].1, OrderEvent::Closed))
    }

    /// Whether the order is closed, voided or abandoned just before position `i`.
    fn inactive(&self, i: usize) -> bool {
        self.ended(i) || self.closed(i)
    }
}

/// A line's life: where it was born, and where its first removal or void took it off.
#[derive(Clone, Copy, Debug)]
struct Life {
    id: Id<Line>,
    birth: usize,
    terminal: Option<usize>,
}

impl Life {
    /// Whether the line is live just before position `i`.
    fn live(&self, i: usize) -> bool {
        self.birth < i && self.terminal.is_none_or(|terminal| i <= terminal)
    }
}

/// Each check, in the order opened, with the lines it was charged for once closed.
type Checks = Vec<Opened>;

/// What the pass over the events predicts for the checks and the lines' allocations.
struct Pass {
    checks: Checks,
    /// Each line's allocation.
    allocation: BTreeMap<Id<Line>, Vec<CheckShare>>,
    /// Just before each position, the lines with a part on a closed check.
    frozen: Vec<BTreeSet<Id<Line>>>,
}

fn exists(checks: &Checks, check: Id<Check>) -> bool {
    checks.iter().any(|(known, _)| *known == check)
}

fn open(checks: &Checks, check: Id<Check>) -> bool {
    checks.iter().any(|(known, charged)| *known == check && charged.is_none())
}

/// Whether `line`'s allocation gives it a part on `check`.
fn on(allocation: &BTreeMap<Id<Line>, Vec<CheckShare>>, line: Id<Line>, check: Id<Check>) -> bool {
    allocation.get(&line).is_some_and(|shares| shares.iter().any(|share| share.check == check))
}

/// Where a line, or its part, goes when it has nowhere else: the first open check that doesn't
/// hold a part of it (`holding`), or else a new check with the identifier of the event that sent
/// it there, unless a check already has that identifier.
fn place(checks: &mut Checks, holding: &[CheckShare], event: Id<Event>) -> Option<Id<Check>> {
    let free = checks.iter().find(|(check, charged)| {
        charged.is_none() && !holding.iter().any(|share| share.check == *check)
    });
    if let Some((check, _)) = free {
        return Some(*check);
    }
    let fresh: Id<Check> = event.cast();
    if exists(checks, fresh) {
        return None;
    }
    checks.push((fresh, None));
    Some(fresh)
}

/// The checks and the lines' allocations, as the pass over the events leaves them so far.
struct Ledger<'a> {
    lives: &'a [Life],
    currency: Currency,
    checks: Checks,
    /// Each line's allocation.
    allocation: BTreeMap<Id<Line>, Vec<CheckShare>>,
}

impl Ledger<'_> {
    fn life(&self, line: Id<Line>) -> Option<&Life> {
        self.lives.iter().find(|life| life.id == line)
    }

    /// An allocation at position `i`, whose event is `at`.
    fn allocate(
        &mut self,
        i: usize,
        at: Id<Event>,
        allocated: &LinesAllocated,
        conflicts: &mut Vec<(Id<Event>, ConflictKind)>,
    ) {
        let mut unknown = BTreeSet::new();
        for (line, shares) in allocated_lines(allocated) {
            if !self.life(line).is_some_and(|life| life.live(i)) {
                continue;
            }
            let missing =
                shares.iter().map(|share| share.check).find(|&check| !exists(&self.checks, check));
            if let Some(check) = missing {
                unknown.insert(check);
                continue;
            }
            let touches_closed = self.allocation[&line]
                .iter()
                .chain(&shares)
                .any(|share| !open(&self.checks, share.check));
            if touches_closed {
                conflicts.push((at, ConflictKind::AllocatedOnClosedCheck(line)));
            } else {
                self.allocation.insert(line, shares);
            }
        }
        conflicts.extend(unknown.into_iter().map(|check| (at, ConflictKind::UnknownCheck(check))));
    }

    /// A check's close at position `i`, whose event is `at`.
    fn close(
        &mut self,
        i: usize,
        at: Id<Event>,
        closed: &CheckClosed,
        conflicts: &mut Vec<(Id<Event>, ConflictKind)>,
    ) {
        let check = closed.check;
        let refused = if !exists(&self.checks, check) {
            Some(ConflictKind::UnknownCheck(check))
        } else if !open(&self.checks, check) {
            Some(ConflictKind::DuplicateClose(check))
        } else if closed.total.currency() != self.currency {
            Some(ConflictKind::CheckCurrencyMismatch(check))
        } else {
            None
        };
        if let Some(kind) = refused {
            conflicts.push((at, kind));
            return;
        }
        let charged: Vec<Id<Line>> = closed.lines.iter().map(|charge| charge.line).collect();
        for &line in &charged {
            let kind = match self.life(line) {
                Some(life) if life.birth < i => {
                    if life.live(i) && on(&self.allocation, line, check) {
                        continue;
                    }
                    ConflictKind::ChargedOffCheck(line)
                }
                _ => ConflictKind::UnknownLine(line),
            };
            conflicts.push((at, kind));
        }
        for (known, lines) in &mut self.checks {
            if *known == check {
                *lines = Some(charged.clone());
            }
        }
        // The lines on the check that it wasn't charged for: their parts move.
        for life in self.lives.iter().filter(|life| life.live(i)) {
            if !on(&self.allocation, life.id, check) || charged.contains(&life.id) {
                continue;
            }
            let holding = self.allocation[&life.id].clone();
            if let Some(target) = place(&mut self.checks, &holding, at) {
                let mut moved: Vec<CheckShare> = holding
                    .iter()
                    .map(|share| {
                        if share.check == check {
                            CheckShare { check: target, ..*share }
                        } else {
                            *share
                        }
                    })
                    .collect();
                moved.sort_by_key(|share| share.check);
                self.allocation.insert(life.id, moved);
            }
            conflicts.push((at, ConflictKind::LeftOffCheck(life.id)));
        }
    }
}

/// One pass over the events that apply, for the checks: which exist, which are closed and what
/// they were charged for, and where each line is allocated. Checks and allocations move
/// together (a close moves the lines it left off, and a line added when every check is closed
/// opens one), so the model follows them in order; the rest of the model queries the result.
fn pass_checks(
    events: &[(EventMeta, OrderEvent)],
    timeline: &Timeline<'_>,
    lives: &[Life],
    currency: Currency,
    conflicts: &mut Vec<(Id<Event>, ConflictKind)>,
) -> Pass {
    let mut ledger =
        Ledger { lives, currency, checks: vec![(main_check(), None)], allocation: BTreeMap::new() };
    let mut frozen = vec![BTreeSet::new(); events.len()];
    for &i in timeline.applying {
        let at = events[i].0.event_id;
        frozen[i] = ledger
            .allocation
            .iter()
            .filter(|(_, shares)| shares.iter().any(|share| !open(&ledger.checks, share.check)))
            .map(|(line, _)| *line)
            .collect();
        match &events[i].1 {
            OrderEvent::CheckOpened { check } => {
                if exists(&ledger.checks, *check) {
                    conflicts.push((at, ConflictKind::DuplicateCheck(*check)));
                } else {
                    ledger.checks.push((*check, None));
                }
            }
            OrderEvent::LineAdded(added) if lives.iter().any(|life| life.birth == i) => {
                let check = place(&mut ledger.checks, &[], at).unwrap_or(main_check());
                ledger.allocation.insert(added.line, vec![whole(check)]);
            }
            OrderEvent::LinesAllocated(allocated) => ledger.allocate(i, at, allocated, conflicts),
            OrderEvent::CheckClosed(closed) => ledger.close(i, at, closed, conflicts),
            OrderEvent::Closed if !timeline.inactive(i) => {
                for (check, charged) in &ledger.checks {
                    let holds = lives
                        .iter()
                        .any(|life| life.live(i) && on(&ledger.allocation, life.id, *check));
                    if charged.is_none() && holds {
                        conflicts.push((at, ConflictKind::ClosedWithOpenCheck(*check)));
                    }
                }
            }
            OrderEvent::Reopened { .. } if !timeline.ended(i) => {
                for (_, charged) in &mut ledger.checks {
                    *charged = None;
                }
            }
            _ => {}
        }
    }
    Pass { checks: ledger.checks, allocation: ledger.allocation, frozen }
}

/// What the model predicts for one line.
struct History {
    /// The line's final state, but for its allocation, which the pass over the checks predicts.
    line: ModelLine,
    /// The position of the first event that fired it, even after it was taken off.
    sent: Option<usize>,
}

/// What the model predicts for the line whose life is `life`. The conflicts its events cause go
/// into `conflicts`.
///
/// `applying` lists the positions of the events that apply to the order: after its creation,
/// and at its location. `frozen` gives, just before each position, the lines with a part on a
/// closed check.
fn expect_line(
    events: &[(EventMeta, OrderEvent)],
    timeline: &Timeline<'_>,
    life: Life,
    currency: Currency,
    frozen: &[BTreeSet<Id<Line>>],
    conflicts: &mut Vec<(Id<Event>, ConflictKind)>,
) -> History {
    let at = |i: usize| events[i].0.event_id;
    let OrderEvent::LineAdded(added) = &events[life.birth].1 else { unreachable!() };
    let id = added.line;
    let later: Vec<usize> = timeline
        .applying
        .iter()
        .copied()
        .filter(|&i| i > life.birth && referred(&events[i].1).contains(&id))
        .collect();
    let terminal = life.terminal;
    // The line is live until its first removal or void.
    let live = |i: usize| terminal.is_none_or(|terminal| i < terminal);
    let frozen_at = |i: usize| frozen[i].contains(&id);
    let fires = |i: &usize| matches!(events[*i].1, OrderEvent::LinesFired { .. });
    let first_fire = later.iter().copied().find(|&i| live(i) && fires(&i));
    let sent = later.iter().copied().find(fires);
    let mut line = ModelLine::new(added);
    line.fired = sent.is_some();
    for &i in &later {
        let found: Vec<ConflictKind> = match &events[i].1 {
            OrderEvent::LineChanged(_) if !live(i) => vec![ConflictKind::ChangedAfterRemoval(id)],
            OrderEvent::LineChanged(changed)
                if changed.quantity.is_some_and(|q| q.unit() != added.quantity.unit()) =>
            {
                vec![ConflictKind::UnitMismatch(id)]
            }
            OrderEvent::LineChanged(changed)
                if changed.modifiers.as_ref().is_some_and(|m| !priced_in(m, currency)) =>
            {
                vec![ConflictKind::CurrencyMismatch(id)]
            }
            OrderEvent::LineChanged(changed) => {
                line.change(changed);
                let mut found = Vec::new();
                if first_fire.is_some_and(|fire| fire < i) {
                    found.push(ConflictKind::ChangedAfterFire(id));
                }
                // What a line costs is frozen on a closed check; its seat, course and notes aren't.
                let costs = changed.quantity.is_some() || changed.modifiers.is_some();
                if frozen_at(i) && costs {
                    found.push(ConflictKind::ChangedOnClosedCheck(id));
                }
                found
            }
            OrderEvent::LinesFired { .. } if !live(i) => vec![ConflictKind::FiredAfterRemoval(id)],
            OrderEvent::LineComped { .. } if !live(i) => vec![ConflictKind::CompedAfterRemoval(id)],
            // The first comp wins.
            OrderEvent::LineComped { reason, .. } if line.comp.is_none() => {
                line.comp = Some(reason.clone());
                if frozen_at(i) { vec![ConflictKind::ChangedOnClosedCheck(id)] } else { Vec::new() }
            }
            _ => Vec::new(),
        };
        conflicts.extend(found.into_iter().map(|kind| (at(i), kind)));
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
    // A removal or void of a line on a closed check.
    if let Some(terminal) = terminal.filter(|&terminal| frozen_at(terminal)) {
        conflicts.push((at(terminal), ConflictKind::ChangedOnClosedCheck(id)));
    }
    if timeline.inactive(life.birth) {
        conflicts.push((at(life.birth), ConflictKind::AddedToClosedOrder(id)));
    }
    if let Some(fire) = first_fire.filter(|&fire| timeline.inactive(fire)) {
        conflicts.push((at(fire), ConflictKind::FiredOnClosedOrder(id)));
    }
    History { line, sent }
}

/// Each line's life, in the order the lines were born, as queries over the events that apply;
/// with the attribute changes applied to `info`, and the conflicts of adding lines and of
/// referring to lines the order doesn't have.
fn lives_of(
    timeline: &Timeline<'_>,
    currency: Currency,
    info: &mut OrderInfo,
    conflicts: &mut Vec<(Id<Event>, ConflictKind)>,
) -> Vec<Life> {
    let (events, applying) = (timeline.events, timeline.applying);
    let at = |i: usize| events[i].0.event_id;
    // A line is born at the first applying addition of its identifier in the order's currency.
    let born = |line: Id<Line>| {
        applying.iter().copied().find(|&i| {
            matches!(&events[i].1, OrderEvent::LineAdded(added)
                if added.line == line && added.item.unit_price.currency() == currency)
        })
    };
    let mut lives = Vec::new();
    for &i in applying {
        match &events[i].1 {
            OrderEvent::LineAdded(added) => match born(added.line) {
                Some(birth) if birth == i => {
                    // It is taken off by its first removal or void.
                    let takes_off = |j: &usize| {
                        *j > i
                            && matches!(&events[*j].1,
                                OrderEvent::LineRemoved { line } | OrderEvent::LineVoided { line, .. }
                                    if *line == added.line)
                    };
                    let terminal = applying.iter().copied().find(takes_off);
                    lives.push(Life { id: added.line, birth: i, terminal });
                }
                Some(birth) if birth < i => {
                    conflicts.push((at(i), ConflictKind::DuplicateLine(added.line)));
                }
                _ => conflicts.push((at(i), ConflictKind::CurrencyMismatch(added.line))),
            },
            OrderEvent::AttributesChanged(changed) => change_attributes(info, changed),
            event => {
                for line in referred(event) {
                    if born(line).is_none_or(|birth| birth > i) {
                        conflicts.push((at(i), ConflictKind::UnknownLine(line)));
                    }
                }
            }
        }
    }
    lives
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
    let end = applying
        .iter()
        .copied()
        .find(|&i| matches!(events[i].1, OrderEvent::Voided { .. } | OrderEvent::Abandoned));
    let timeline = Timeline { events, applying: &applying, end };
    let lives = lives_of(&timeline, currency, &mut info, &mut conflicts);
    let pass = pass_checks(events, &timeline, &lives, currency, &mut conflicts);
    let mut lines = Vec::new();
    // Whether, when the order ended, a line was live or had been fired: something was ordered.
    let mut ordered_at_end = false;
    for &life in &lives {
        let mut history =
            expect_line(events, &timeline, life, currency, &pass.frozen, &mut conflicts);
        history.line.allocation.clone_from(&pass.allocation[&life.id]);
        ordered_at_end |= end.is_some_and(|end| {
            life.birth < end
                && (life.terminal.is_none_or(|terminal| terminal > end)
                    || history.sent.is_some_and(|sent| sent < end))
        });
        lines.push(history.line);
    }
    let status = match end.map(|end| &events[end].1) {
        Some(OrderEvent::Voided { reason }) => OrderStatus::Voided(reason.clone()),
        Some(_) => OrderStatus::Abandoned,
        None if timeline.closed(events.len()) => OrderStatus::Closed,
        None => OrderStatus::Active,
    };
    if let Some(end) = end.filter(|_| status == OrderStatus::Abandoned && ordered_at_end) {
        conflicts.push((at(end), ConflictKind::AbandonedWithLines));
    }
    Expected { info: Some(info), lines, status, checks: with_numbers(&pass.checks), conflicts }
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

/// Whether a check was opened with the identifier of one of `events`, as only a forged event
/// would: a new check can't then take the identifier, and a line can be left on a closed check.
fn forged(events: &[(EventMeta, OrderEvent)]) -> bool {
    events.iter().any(|(_, event)| {
        matches!(event, OrderEvent::CheckOpened { check }
            if events.iter().any(|(meta, _)| meta.event_id.cast() == *check))
    })
}

/// Checks what must hold of any order, whatever its events: lines are unique, priced in the
/// order's currency and of positive quantity, and nothing applies before the order is created.
fn invariants(order: &Order) -> Result<(), TestCaseError> {
    invariants_with(order, true)
}

/// The same, and a live line's parts on closed checks charged by them only when `charged`:
/// forged identifiers can leave a line on a closed check.
fn invariants_with(order: &Order, charged: bool) -> Result<(), TestCaseError> {
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
    checks_hold_the_lines(order, charged)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Checks the order's checks: numbered from 1, the main check first; every live line allocated
/// to checks that exist, in ascending order of check, in shares in lowest terms, and charged by
/// each closed check it has a part on; and each check's basket holding its share of each line
/// allocated to it, the checks' shares of a line adding up to the whole line.
fn checks_hold_the_lines(order: &Order, charged: bool) -> Result<(), TestCaseError> {
    let checks = checks_of(order);
    let ids: BTreeSet<Id<Check>> = checks.iter().map(|(check, _, _)| *check).collect();
    prop_assert_eq!(ids.len(), checks.len());
    let numbers: Vec<u32> = checks.iter().map(|(_, number, _)| *number).collect();
    prop_assert_eq!(numbers, (1..).take(checks.len()).collect::<Vec<u32>>());
    prop_assert!(order.check_basket(id(0xFFFF_FFFF)).is_none());
    if order.info().is_none() {
        prop_assert!(checks.is_empty() && order.check_basket(main_check()).is_none());
        return Ok(());
    }
    prop_assert_eq!(checks.first().map(|(check, _, _)| *check), Some(main_check()));
    for check in order.checks() {
        prop_assert!(check.closed().is_none_or(|closed| closed.check == check.id()));
    }
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
        for share in allocation.iter().filter(|_| charged) {
            let closed = order.check(share.check).and_then(Check::closed);
            prop_assert!(closed.is_none_or(|closed| closed.line(line.id()).is_some()));
        }
        prop_assert_eq!(
            order.is_frozen(line),
            allocation.iter().any(|share| order.check(share.check).is_some_and(|c| !c.is_open()))
        );
    }
    let rules = Rules::untaxed();
    let mut parts = vec![0_i128; live.len()];
    for &(check, _, _) in &checks {
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

/// A close of one of the few checks, charging one to three of the few lines, each for what it
/// costs or comped, and sometimes taxed: mostly in dollars, sometimes in euros.
fn fold_closed() -> impl Strategy<Value = CheckClosed> {
    let charges = prop::collection::btree_map(fold_line(), (any::<bool>(), 0_i64..2), 1..=3);
    (fold_check(), charges, fold_currency()).prop_map(|(check, charges, currency)| {
        let money = |minor| Money::from_minor(minor, currency);
        let lines: Vec<LineCharge> = charges
            .into_iter()
            .map(|(line, (comped, tax))| LineCharge {
                line,
                gross: money(450),
                net: money(if comped { 0 } else { 450 }),
                tax: money(if comped { 0 } else { tax * 40 }),
            })
            .collect();
        let net = Money::sum(currency, lines.iter().map(|line| line.net)).unwrap();
        let tax = Money::sum(currency, lines.iter().map(|line| line.tax)).unwrap();
        let taxes = if tax.is_positive() {
            vec![keel_domain::order::TaxCharge { tax: id(0x900), taxable: net, amount: tax }]
        } else {
            Vec::new()
        };
        let total = net.checked_add(tax).unwrap();
        CheckClosed {
            check,
            rules_version: RulesVersion::from_bytes([1; 32]),
            lines,
            taxes,
            total,
            payments: total.is_positive().then(|| IdSet::new([id(0xB1)]).unwrap()),
        }
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
        3 => fold_closed().prop_map(OrderEvent::CheckClosed),
        1 => Just(OrderEvent::Closed),
        1 => any::<u8>().prop_map(|n| OrderEvent::Reopened { reason: reason(n) }),
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

/// An event aimed at closing: mostly closes of checks, with allocations, changes, removals,
/// voids and comps of lines between them, lines added, and the order closed, reopened or
/// voided. Rarely, a forgery: see `forge`.
fn any_closing_event() -> impl Strategy<Value = OrderEvent> {
    prop_oneof![
        6 => fold_closed().prop_map(OrderEvent::CheckClosed),
        3 => fold_allocated().prop_map(OrderEvent::LinesAllocated),
        2 => fold_added().prop_map(OrderEvent::LineAdded),
        2 => fold_changed().prop_map(OrderEvent::LineChanged),
        1 => fold_line().prop_map(|line| OrderEvent::LineRemoved { line }),
        1 => (fold_line(), any::<u8>())
            .prop_map(|(line, n)| OrderEvent::LineVoided { line, reason: reason(n) }),
        1 => fold_line().prop_map(|line| OrderEvent::LinesFired { lines: IdSet::new([line]).unwrap() }),
        1 => (fold_line(), any::<u8>())
            .prop_map(|(line, n)| OrderEvent::LineComped { line, reason: reason(n) }),
        1 => fold_check().prop_map(|check| OrderEvent::CheckOpened { check }),
        2 => Just(OrderEvent::Closed),
        2 => any::<u8>().prop_map(|n| OrderEvent::Reopened { reason: reason(n) }),
        1 => any::<u8>().prop_map(|n| OrderEvent::Voided { reason: reason(n) }),
        1 => Just(OrderEvent::CheckOpened { check: forgery() }),
    ]
}

/// Marks where `forge` puts a forged check.
fn forgery() -> Id<Check> {
    id(0xF0F0)
}

/// Replaces each forgery marker with a check opened and closed under the identifier of the next
/// event that may open a check, a line's addition or another check's close, as only a forged
/// event would: that event then finds the identifier taken. The events are numbered from 1 by
/// `numbered`, so their identifiers are known in advance.
fn forge(events: Vec<OrderEvent>) -> Vec<OrderEvent> {
    let marked = |event: &OrderEvent| matches!(event, OrderEvent::CheckOpened { check } if *check == forgery());
    let mut expanded: Vec<OrderEvent> = Vec::new();
    for event in events {
        if marked(&event) {
            expanded.push(event.clone());
        }
        expanded.push(event);
    }
    let opens =
        |event: &OrderEvent| matches!(event, OrderEvent::LineAdded(_) | OrderEvent::CheckClosed(_));
    let mut j = 0;
    while j < expanded.len() {
        if marked(&expanded[j]) {
            let target =
                (j + 2..expanded.len()).find(|&t| opens(&expanded[t]) && !marked(&expanded[t]));
            // Event `t`, counted from zero, is numbered `t + 1`.
            let check =
                target.map_or(id(0xF0F1), |t| id((1 << 24) + u64::try_from(t).unwrap() + 1));
            let charge = LineCharge {
                line: id(1),
                gross: Money::from_minor(450, usd()),
                net: Money::from_minor(450, usd()),
                tax: Money::from_minor(0, usd()),
            };
            let closed = CheckClosed {
                check,
                rules_version: RulesVersion::from_bytes([1; 32]),
                lines: vec![charge],
                taxes: Vec::new(),
                total: Money::from_minor(450, usd()),
                payments: Some(IdSet::new([id(0xB1)]).unwrap()),
            };
            expanded[j] = OrderEvent::CheckOpened { check };
            expanded[j + 1] = OrderEvent::CheckClosed(closed);
            j += 2;
        } else {
            j += 1;
        }
    }
    expanded
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

    /// The same for closing: an order with lines and checks, then closes of its checks, some of
    /// them charging for lines they don't hold or leaving out lines they do, between changes to
    /// the lines, allocations, and the order closed, reopened and voided.
    #[test]
    fn the_fold_matches_the_model_on_closing(
        added in prop::collection::vec(fold_added(), 1..=4),
        opened in prop::collection::btree_set((1_u64..=3).prop_map(|n| id::<Check>(0xC0 + n)), 0..=3),
        allocated in prop::collection::vec(fold_allocated(), 0..=2),
        after in prop::collection::vec(any_closing_event(), 0..24),
    ) {
        let created = OrderEvent::Created(OrderCreated { currency: usd(), ..created() });
        let events: Vec<OrderEvent> = core::iter::once(created)
            .chain(added.into_iter().map(OrderEvent::LineAdded))
            .chain(opened.into_iter().map(|check| OrderEvent::CheckOpened { check }))
            .chain(allocated.into_iter().map(OrderEvent::LinesAllocated))
            .chain(after)
            .collect();
        let events = numbered(forge(events).into_iter().map(|event| (false, event)).collect());
        let order = folded(&events)?;
        check(&order, &events)?;
        invariants_with(&order, !forged(&events))?;
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

    /// One device's commands, faulty ones included, are refused exactly when the model refuses
    /// them, and for the same reason; its closes of checks through checkout, once it has paid
    /// what they owe, are accepted exactly when the model allows them. Accepted actions never
    /// cause a conflict, and leave the order as the model predicts.
    #[test]
    fn commands_do_what_the_model_says(steps in prop::collection::vec(any_step(), 1..60)) {
        let mut device = Working::new(1, Order::new(order_id()), 1_000);
        prop_assert!(device.run(Action::Order(OrderCommand::Create(created())), location(), 0));
        let mut model = Model::new(info_of(&device.log[0].0, &created()));
        for step in &steps {
            let (action, from) = action(&device.view, step, device.fresh(), device.fresh_check());
            check_refusal(&device.view, &model, &action, from)?;
            let allowed = model.allows(&action, from);
            let accepted = device.run(action.clone(), from, 0);
            prop_assert_eq!(accepted, allowed, "{:?} from {:?}", action, from);
            if accepted {
                model.apply(&action, device.log.last().unwrap().0.event_id);
            }
            prop_assert!(device.view.conflicts().is_empty());
            prop_assert_eq!(device.view.info(), Some(&model.info));
            prop_assert_eq!(lines_of(&device.view), model.lines.clone());
            prop_assert_eq!(device.view.status(), &model.status);
            prop_assert_eq!(device.view.stage(), stage_of(&model.lines));
            prop_assert_eq!(checks_of(&device.view), with_numbers(&model.checks));
        }
        invariants(&device.view)?;
    }

    /// Devices acting at once on stale views of the order: once their events are merged in
    /// canonical order, the order is exactly what the fold model predicts, conflicts included,
    /// and commands on it are accepted exactly when the model allows them.
    #[test]
    fn concurrent_devices_converge_as_the_model_says(
        prefix in prop::collection::vec(any_step(), 0..12),
        devices in prop::collection::vec(prop::collection::vec(any_step(), 0..16), 2..=3),
    ) {
        let merged = merge(&prefix, &devices);
        check(&merged.order, &merged.events)?;
        invariants(&merged.order)?;
        check_commands_on(&merged)?;
    }

    /// Devices racing on one table once lines are added, each playing a part: closing checks,
    /// firing lines, or removing them and ending the order. The merged order is what the fold
    /// model predicts; commands on it and on every state it passed through, which only races
    /// put to the test (abandoning an order whose closed check lost its lines to a removal,
    /// say), are accepted exactly when the model allows them; and a device carrying on with it
    /// does what the model says.
    #[test]
    fn commands_on_merged_orders_do_what_the_model_says(
        prefix in prop::collection::vec(racing_step(0), 1..4),
        devices in prop::collection::vec(racer(), 2..=3),
        after in prop::collection::vec(any_step(), 0..12),
    ) {
        let merged = merge(&prefix, &devices);
        check(&merged.order, &merged.events)?;
        invariants(&merged.order)?;
        check_commands_on(&merged)?;
        carry_on(&merged, &after)?;
    }
}
