//! What devices do: ring orders and take cash through `keel-domain`'s commands and checkout, each
//! decided against the device's own view of the order, in one write with the events it makes,
//! as a register would. A move the device's view refuses is skipped, but for one that needs the
//! order and another device owns it (ADR-0021): the device asks the hub for the order instead,
//! or, cut off from the hub, takes it on a manager's word.

use core::num::NonZeroU16;

use keel_domain::checkout::{Checkout, CheckoutError};
use keel_domain::codec::{CatalogVersion, Change, IdSet, Name, ReasonCode, RulesVersion};
use keel_domain::order::{
    Allocation, AttributesChanged, Channel, Check, CommandError, ItemSnapshot, Line, LineAdded,
    LineChanged, LineStatus, LinesAllocated, Mode, Order, OrderCommand, OrderCreated, Reason,
};
use keel_domain::payment::{CashTendered, Payment, PaymentCaptured, PaymentCommand, Tender};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{Actor, Aggregate, Device, StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::Signer;
use keel_events::log::EventDraft;
use keel_pricing::Rules;
use keel_store::{OrderState, StoreError, Writing};
use keel_types::{
    Currency, Entropy, Id, IdGenerator, Money, Quantity, SeededEntropy, Timestamp, Unit,
};

use crate::node::{SimStore, here, id};
use crate::rng::Rng;

/// A move a device makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Move {
    /// Opens a new order.
    Create,
    /// Adds a line to an active order.
    AddLine,
    /// Changes an order's guest count.
    ChangeAttributes,
    /// Changes a line's quantity.
    ChangeLine,
    /// Removes a line not yet fired.
    RemoveLine,
    /// Fires the lines waiting to be.
    FireLines,
    /// Voids a fired line.
    VoidLine,
    /// Comps a line.
    CompLine,
    /// Opens another check on an order.
    OpenCheck,
    /// Moves a line to a check.
    Allocate,
    /// Starts a cash payment of what a check owes.
    StartPayment,
    /// Captures a cash payment started.
    CapturePayment,
    /// Closes a check its payments cover.
    CloseCheck,
    /// Closes an order whose checks are closed.
    CloseOrder,
    /// Reopens a closed order.
    Reopen,
    /// Voids an order.
    VoidOrder,
    /// Abandons an order nothing of which was fired.
    Abandon,
    /// Asks the hub for an order another device owns: chosen, or made in place of a move that
    /// needs the order.
    RequestOwnership,
    /// Takes an order another device owns on a manager's word: made, by a device cut off from
    /// the hub, in place of a move that needs the order.
    OverrideOwnership,
}

impl Move {
    /// Every move a device chooses, and how many times in a hundred it does.
    const WEIGHTED: [(Move, u64); 18] = [
        (Move::Create, 12),
        (Move::AddLine, 19),
        (Move::ChangeAttributes, 5),
        (Move::ChangeLine, 5),
        (Move::RemoveLine, 4),
        (Move::FireLines, 8),
        (Move::VoidLine, 3),
        (Move::CompLine, 2),
        (Move::OpenCheck, 4),
        (Move::Allocate, 5),
        (Move::StartPayment, 7),
        (Move::CapturePayment, 7),
        (Move::CloseCheck, 6),
        (Move::CloseOrder, 4),
        (Move::Reopen, 2),
        (Move::VoidOrder, 2),
        (Move::Abandon, 2),
        (Move::RequestOwnership, 3),
    ];

    fn pick(rng: &mut Rng) -> Move {
        let total: u64 = Move::WEIGHTED.iter().map(|(_, weight)| weight).sum();
        let mut roll = rng.below(total);
        for (r#move, weight) in Move::WEIGHTED {
            if roll < weight {
                return r#move;
            }
            roll = roll.saturating_sub(weight);
        }
        Move::AddLine
    }
}

/// The device that makes a move: which it is, and whether it is cut off from the hub, which the
/// simulator knows and, until heartbeats come (ADR-0021), the device doesn't.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Station {
    pub(crate) device: Id<Device>,
    pub(crate) island: bool,
}

/// What became of a move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Made {
    /// It appended these events.
    Appended(Vec<SignedEvent>),
    /// The device's view refused it.
    Refused,
    /// There was nothing to make it on.
    Nothing,
}

/// Why a move failed: the store, or something the simulation should never do.
#[derive(Debug)]
pub(crate) enum WorkError {
    Store(StoreError),
    Other(String),
}

impl From<StoreError> for WorkError {
    fn from(error: StoreError) -> WorkError {
        WorkError::Store(error)
    }
}

fn other(error: impl core::fmt::Debug) -> WorkError {
    WorkError::Other(format!("{error:?}"))
}

/// How many of the latest orders a move on any order picks from, half the time: so that devices
/// contend for the same orders, as servers do for the tables being served.
const HOT: usize = 2;

/// The prices of the menu's items, in cents.
const PRICES: [i64; 4] = [350, 450, 1200, 2_850];

/// The rules the location prices with: no taxes.
fn rules_version() -> RulesVersion {
    RulesVersion::from_bytes([9; 32])
}

/// Makes one move on `store` at physical time `now`, as `station`: returns the move and what
/// became of it. Seven times in ten the device takes the next step of the sale it is working
/// on, its `focus`: a sale runs from creating the order through its lines to paying and closing
/// it. Otherwise it makes any move on any order, as another server would, half the time on one
/// of the latest few, so that devices change the same orders concurrently, and ask for each
/// other's orders.
pub(crate) fn work(
    store: &mut SimStore,
    ids: &mut IdGenerator<SeededEntropy>,
    rng: &mut Rng,
    focus: &mut Option<Id<Order>>,
    station: Station,
    now: Timestamp,
) -> Result<(Move, Made), WorkError> {
    let active = store.orders(OrderState::Active)?;
    if focus.is_some_and(|order| !active.iter().any(|summary| summary.id == order)) {
        *focus = None;
    }
    let (r#move, order) = if rng.chance(700) {
        match *focus {
            Some(order) => (next_step(store, order, rng)?, Some(order)),
            None => (Move::Create, None),
        }
    } else {
        let r#move = Move::pick(rng);
        // Oldest first: by when they were created.
        let mut orders: Vec<Id<Order>> = match r#move {
            Move::Reopen => {
                store.orders(OrderState::Closed)?.into_iter().map(|order| order.id).collect()
            }
            // An order another device owns, as this one sees it.
            Move::RequestOwnership => active
                .iter()
                .filter(|order| order.ownership.is_some_and(|o| o.device != station.device))
                .map(|order| order.id)
                .collect(),
            _ => active.iter().map(|order| order.id).collect(),
        };
        if rng.chance(500) {
            orders.drain(..orders.len().saturating_sub(HOT));
        }
        (r#move, rng.pick(&orders).copied())
    };
    Ok(match (r#move, order) {
        (Move::Create, _) => {
            let (made, created) = create(store, ids, rng, station, now)?;
            if matches!(made, Made::Appended(_)) {
                *focus = Some(created);
            }
            (r#move, made)
        }
        (_, None) => (r#move, Made::Nothing),
        (Move::StartPayment | Move::CapturePayment | Move::CloseCheck, Some(order)) => {
            checkout(store, order, r#move, station, ids, rng, now)?
        }
        (_, Some(order)) => order_move(store, order, r#move, station, ids, rng, now)?,
    })
}

/// The next step of the sale of `order`, as the device sees it: more lines until it has a few,
/// then capturing a payment started, closing a check its payments cover, starting a payment of
/// what a check owes, and closing the order once its checks are closed.
fn next_step(store: &SimStore, order: Id<Order>, rng: &mut Rng) -> Result<Move, WorkError> {
    let loaded = store.load(Order::new(order))?;
    if loaded.live_lines().count() < 2 || rng.chance(250) {
        return Ok(if rng.chance(250) { Move::FireLines } else { Move::AddLine });
    }
    let payments = store
        .payments_of(order)?
        .into_iter()
        .map(|payment| store.load(Payment::new(payment.id)))
        .collect::<Result<Vec<Payment>, StoreError>>()?;
    if payments.iter().any(Payment::is_unresolved) {
        return Ok(Move::CapturePayment);
    }
    let rules = Rules::untaxed();
    let checkout = Checkout::new(&loaded, payments.iter(), &rules, rules_version());
    let owing: Vec<Money> = loaded
        .checks()
        .iter()
        .filter(|check| check.is_open() && loaded.lines_on(check.id()).any(Line::is_live))
        .filter_map(|check| checkout.balance(check.id()).ok().map(|balance| balance.due))
        .collect();
    Ok(if owing.is_empty() {
        Move::CloseOrder
    } else if owing.iter().any(|due| !due.is_positive()) {
        Move::CloseCheck
    } else {
        Move::StartPayment
    })
}

/// Creates an order: what became of it, and the order's identifier.
fn create(
    store: &mut SimStore,
    ids: &mut IdGenerator<SeededEntropy>,
    rng: &mut Rng,
    station: Station,
    now: Timestamp,
) -> Result<(Made, Id<Order>), WorkError> {
    let order: Id<Order> = ids.generate(now).map_err(other)?;
    let table = 1_u64.saturating_add(rng.below(20));
    let created = OrderCreated {
        channel: Channel::Pos,
        mode: Mode::DineIn,
        currency: usd()?,
        revenue_center: None,
        table: Some(id(0x2000_u64.saturating_add(table))),
        guest_count: NonZeroU16::new(2),
        customer: None,
        owner: Some(id(0x300)),
    };
    let Ok(event) = Order::new(order).decide(here(), station.device, OrderCommand::Create(created))
    else {
        return Ok((Made::Refused, order));
    };
    let draft = draft(order.cast(), &event)?;
    let made = store.write(|w| Ok::<_, WorkError>(Made::Appended(vec![w.append(draft, now)?])))?;
    Ok((made, order))
}

/// Makes a move on order `order` in one write: loads it, decides, and appends the event. If the
/// move needs the order and another device owns it, asks for the order instead (see [`take`]).
fn order_move(
    store: &mut SimStore,
    order: Id<Order>,
    r#move: Move,
    station: Station,
    ids: &mut IdGenerator<SeededEntropy>,
    rng: &mut Rng,
    now: Timestamp,
) -> Result<(Move, Made), WorkError> {
    store.write(|w| {
        let loaded = w.load(Order::new(order))?;
        let Some(command) = command_for(&loaded, r#move, ids, rng, now)? else {
            return Ok((r#move, Made::Nothing));
        };
        match loaded.decide(here(), station.device, command) {
            Ok(event) => Ok((r#move, append(w, order.cast(), &event, now)?)),
            Err(CommandError::NotOwner(_)) => take(w, &loaded, station, now),
            Err(_) => Ok((r#move, Made::Refused)),
        }
    })
}

/// Asks the hub for order `order`, which another device owns, in place of a move that needs it;
/// or, cut off from the hub, takes it on a manager's word. Returns the move made instead, and
/// what became of it.
fn take<S: Signer, E: Entropy>(
    w: &mut Writing<'_, S, E>,
    order: &Order,
    station: Station,
    now: Timestamp,
) -> Result<(Move, Made), WorkError> {
    let (r#move, command) = if station.island {
        (Move::OverrideOwnership, OrderCommand::OverrideOwnership(reason("hub_unreachable")?))
    } else {
        (Move::RequestOwnership, OrderCommand::RequestOwnership)
    };
    match order.decide(here(), station.device, command) {
        Ok(event) => Ok((r#move, append(w, order.id().cast(), &event, now)?)),
        Err(_) => Ok((r#move, Made::Refused)),
    }
}

/// The command `r#move` makes on `order`, as the device sees it; `None` if there is nothing
/// to make it on.
fn command_for(
    order: &Order,
    r#move: Move,
    ids: &mut IdGenerator<SeededEntropy>,
    rng: &mut Rng,
    now: Timestamp,
) -> Result<Option<OrderCommand>, WorkError> {
    let with_status = |wanted: fn(&LineStatus) -> bool| -> Vec<Id<Line>> {
        order.lines().iter().filter(|line| wanted(line.status())).map(Line::id).collect()
    };
    let pending = with_status(|status| matches!(status, LineStatus::Pending));
    let fired = with_status(|status| matches!(status, LineStatus::Fired));
    let live: Vec<Id<Line>> = order.live_lines().map(Line::id).collect();
    let open: Vec<Id<Check>> =
        order.checks().iter().filter(|check| check.is_open()).map(Check::id).collect();
    Ok(match r#move {
        Move::AddLine => {
            let price = rng.pick(&PRICES).copied().unwrap_or(450);
            Some(OrderCommand::AddLine(line_added(ids.generate(now).map_err(other)?, price)?))
        }
        Move::ChangeAttributes => {
            let guests = u16::try_from(rng.below(8)).unwrap_or(0).saturating_add(1);
            Some(OrderCommand::ChangeAttributes(AttributesChanged {
                guest_count: NonZeroU16::new(guests).map(Change::Set),
                ..AttributesChanged::default()
            }))
        }
        Move::ChangeLine => rng.pick(&live).map(|&line| {
            let units = rng.between(1, 4).saturating_mul(1_000_000);
            let mut change = LineChanged::to(line);
            change.quantity = Some(Quantity::from_micros(units, Unit::Each));
            OrderCommand::ChangeLine(change)
        }),
        Move::RemoveLine => rng.pick(&pending).map(|&line| OrderCommand::RemoveLine(line)),
        Move::FireLines => {
            let chosen: Vec<Id<Line>> =
                pending.iter().copied().filter(|_| rng.chance(700)).collect();
            if chosen.is_empty() {
                None
            } else {
                Some(OrderCommand::FireLines(IdSet::new(chosen).map_err(other)?))
            }
        }
        Move::VoidLine => match rng.pick(&fired) {
            Some(&line) => Some(OrderCommand::VoidLine { line, reason: reason("void")? }),
            None => None,
        },
        Move::CompLine => match rng.pick(&live) {
            Some(&line) => Some(OrderCommand::CompLine { line, reason: reason("comp")? }),
            None => None,
        },
        Move::OpenCheck => Some(OrderCommand::OpenCheck(ids.generate(now).map_err(other)?)),
        Move::Allocate => match (rng.pick(&live), rng.pick(&open)) {
            (Some(&line), Some(&check)) => {
                let allocation = Allocation { line, check, shares: NonZeroU16::MIN };
                Some(OrderCommand::AllocateLines(LinesAllocated::new([allocation]).map_err(other)?))
            }
            _ => None,
        },
        Move::CloseOrder => Some(OrderCommand::Close),
        Move::Reopen => Some(OrderCommand::Reopen(reason("reopen")?)),
        Move::VoidOrder => Some(OrderCommand::Void(reason("void")?)),
        Move::Abandon => Some(OrderCommand::Abandon),
        Move::RequestOwnership => Some(OrderCommand::RequestOwnership),
        // Made elsewhere: creating, checkout's moves, and an override, which is made only in
        // place of another move.
        Move::Create
        | Move::StartPayment
        | Move::CapturePayment
        | Move::CloseCheck
        | Move::OverrideOwnership => None,
    })
}

/// Makes a checkout move on order `order` in one write: loads it and its payments, decides with
/// checkout or the payment, and appends the event. Starting a payment and closing a check need
/// the order: if another device owns it, asks for the order instead (see [`take`]).
fn checkout(
    store: &mut SimStore,
    order: Id<Order>,
    r#move: Move,
    station: Station,
    ids: &mut IdGenerator<SeededEntropy>,
    rng: &mut Rng,
    now: Timestamp,
) -> Result<(Move, Made), WorkError> {
    let payment_ids: Vec<Id<Payment>> =
        store.payments_of(order)?.into_iter().map(|payment| payment.id).collect();
    store.write(|w| {
        let loaded = w.load(Order::new(order))?;
        let payments = payment_ids
            .iter()
            .map(|&payment| w.load(Payment::new(payment)))
            .collect::<Result<Vec<Payment>, StoreError>>()?;
        let rules = Rules::untaxed();
        let checkout = Checkout::new(&loaded, payments.iter(), &rules, rules_version());
        let open: Vec<Id<Check>> =
            loaded.checks().iter().filter(|check| check.is_open()).map(Check::id).collect();
        // The open checks with live lines, what each owes, and whether a payment on it waits.
        let owing: Vec<(Id<Check>, Money, bool)> = open
            .iter()
            .filter(|&&check| loaded.lines_on(check).any(Line::is_live))
            .filter_map(|&check| {
                let balance = checkout.balance(check).ok()?;
                Some((check, balance.due, balance.unresolved.is_empty()))
            })
            .collect();
        match r#move {
            Move::StartPayment => {
                let payable: Vec<(Id<Check>, Money)> = owing
                    .iter()
                    .filter(|(_, due, settled)| *settled && due.is_positive())
                    .map(|&(check, due, _)| (check, due))
                    .collect();
                let Some(&(check, due)) = rng.pick(&payable) else {
                    return Ok((r#move, Made::Nothing));
                };
                let payment: Id<Payment> = ids.generate(now).map_err(other)?;
                let started = checkout.start_payment(
                    here(),
                    station.device,
                    payment,
                    check,
                    Tender::Cash,
                    due,
                );
                match started {
                    Ok(event) => Ok((r#move, append(w, payment.cast(), &event, now)?)),
                    Err(CheckoutError::Order(CommandError::NotOwner(_))) => {
                        take(w, &loaded, station, now)
                    }
                    Err(_) => Ok((r#move, Made::Refused)),
                }
            }
            Move::CapturePayment => {
                let unresolved: Vec<&Payment> =
                    payments.iter().filter(|payment| payment.is_unresolved()).collect();
                let Some(payment) = rng.pick(&unresolved) else {
                    return Ok((r#move, Made::Nothing));
                };
                let Some(info) = payment.info() else { return Ok((r#move, Made::Nothing)) };
                let captured = PaymentCaptured {
                    amount: info.amount,
                    tip: None,
                    reference: None,
                    cash: Some(CashTendered { tendered: info.amount, rounding: None }),
                };
                match payment.decide(here(), PaymentCommand::Capture(captured)) {
                    Ok(event) => Ok((r#move, append(w, payment.id().cast(), &event, now)?)),
                    Err(_) => Ok((r#move, Made::Refused)),
                }
            }
            _ => {
                let covered: Vec<Id<Check>> = owing
                    .iter()
                    .filter(|(_, due, settled)| *settled && !due.is_positive())
                    .map(|&(check, _, _)| check)
                    .collect();
                let Some(&check) = rng.pick(&covered) else {
                    return Ok((r#move, Made::Nothing));
                };
                match checkout.close_check(here(), station.device, check) {
                    Ok(event) => Ok((r#move, append(w, order.cast(), &event, now)?)),
                    Err(CheckoutError::Order(CommandError::NotOwner(_))) => {
                        take(w, &loaded, station, now)
                    }
                    Err(_) => Ok((r#move, Made::Refused)),
                }
            }
        }
    })
}

fn append<D: DomainEvent, S: Signer, E: Entropy>(
    w: &mut Writing<'_, S, E>,
    stream: Id<Aggregate>,
    event: &D,
    now: Timestamp,
) -> Result<Made, WorkError> {
    Ok(Made::Appended(vec![w.append(draft(stream, event)?, now)?]))
}

/// A draft of `event`, on stream `stream` of its kind.
fn draft<D: DomainEvent>(stream: Id<Aggregate>, event: &D) -> Result<EventDraft, WorkError> {
    let (schema, payload) = event.encode().map_err(other)?;
    Ok(EventDraft {
        stream: StreamRef { kind: StreamKind::new(D::STREAM).map_err(other)?, id: stream },
        schema,
        business_date: "2026-09-30".parse().map_err(other)?,
        actor: Actor::TeamMember(id(0x300)),
        approval: None,
        causation: None,
        correlation: None,
        payload,
    })
}

fn usd() -> Result<Currency, WorkError> {
    Currency::from_code("USD").map_err(other)
}

fn line_added(line: Id<Line>, price: i64) -> Result<LineAdded, WorkError> {
    Ok(LineAdded {
        line,
        item: ItemSnapshot {
            variant: id(0x400),
            catalog_version: CatalogVersion::from_bytes([7; 32]),
            name: Name::new("Flat white").map_err(other)?,
            tax_category: id(0x500),
            unit_price: Money::from_minor(price, usd()?),
        },
        quantity: Quantity::from_micros(1_000_000, Unit::Each),
        modifiers: Vec::new(),
        seat: None,
        course: None,
        notes: None,
    })
}

fn reason(code: &str) -> Result<Reason, WorkError> {
    Ok(Reason { code: ReasonCode::new(code).map_err(other)?, note: None })
}
