//! Known answers for ownership (ADR-0021): the fold, the commands' checks, the hub's answers,
//! and the payloads' rules.

use core::num::NonZeroU16;

use keel_events::cbor::{Map, Value};
use keel_events::envelope::{Actor, Device, Event, Location, Payload};
use keel_types::{Currency, Hlc, Id, Money, Quantity, Unit};

use super::*;
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::{CatalogVersion, Change, IdSet, Name, PayloadError, ReasonCode, RulesVersion};
use crate::order::{
    Allocation, AttributesChanged, Channel, Check, CheckClosed, CommandError, ConflictKind,
    ItemSnapshot, LineAdded, LineChanged, LineStatus, LinesAllocated, Mode, OrderCommand,
    OrderCreated, OrderStatus, Reason,
};
use crate::schema::{DecodeError, DomainEvent};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn device(n: u64) -> Id<Device> {
    id(n)
}

fn location() -> Id<Location> {
    id(0x100)
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

fn lease(n: u64) -> Lease {
    Lease::new(n).unwrap()
}

fn epoch() -> Epoch {
    Epoch::new(3).unwrap()
}

fn created() -> OrderCreated {
    OrderCreated {
        channel: Channel::Pos,
        mode: Mode::DineIn,
        currency: Currency::from_code("USD").unwrap(),
        revenue_center: None,
        table: None,
        guest_count: None,
        customer: None,
        owner: None,
    }
}

fn added(line: u64) -> LineAdded {
    LineAdded {
        line: id(line),
        item: ItemSnapshot {
            variant: id(0x400),
            catalog_version: CatalogVersion::from_bytes([7; 32]),
            name: Name::new("Flat white").unwrap(),
            tax_category: id(0x500),
            unit_price: Money::from_minor(450, Currency::from_code("USD").unwrap()),
        },
        quantity: Quantity::from_whole(1, Unit::Each).unwrap(),
        modifiers: Vec::new(),
        seat: None,
        course: None,
        notes: None,
    }
}

fn granted(request: Id<Event>, to: u64, n: u64) -> OrderEvent {
    OrderEvent::OwnershipGranted(OwnershipGranted {
        request,
        device: device(to),
        lease: lease(n),
        epoch: epoch(),
    })
}

/// An order's events, recorded by several devices, applied in the order given, as a replica
/// folding them in canonical order would. Device 1 creates the order; the hub is device 9.
struct Story {
    order: Order,
    events: u64,
}

impl Story {
    fn new() -> Story {
        let mut story = Story { order: Order::new(id(0xA)), events: 0 };
        story.by(1, &OrderEvent::Created(created()));
        story
    }

    fn by(&mut self, n: u64, event: &OrderEvent) -> Id<Event> {
        self.events = self.events.checked_add(1).unwrap();
        let meta = EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location: location(),
            origin_device: device(n),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_790_600_000_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-10-01".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        };
        self.order.apply(&meta, event);
        meta.event_id
    }

    fn owner(&self) -> (Id<Device>, u64) {
        let ownership = self.order.ownership().unwrap();
        (ownership.device, ownership.lease.get())
    }

    fn kinds(&self) -> Vec<ConflictKind> {
        self.order.conflicts().iter().map(|conflict| conflict.kind).collect()
    }

    fn waiting(&self) -> Vec<(Id<Event>, Id<Device>, u64)> {
        let requests = self.order.requests().iter();
        requests.map(|request| (request.event, request.device, request.lease.get())).collect()
    }
}

#[test]
fn the_device_that_creates_an_order_owns_it_under_the_first_lease() {
    assert_eq!(Order::new(id(0xA)).ownership(), None);
    let story = Story::new();
    assert_eq!(story.owner(), (device(1), 0));
    assert_eq!(story.waiting(), []);
    assert_eq!(story.kinds(), []);
}

#[test]
fn a_grant_from_the_current_lease_gives_the_order_away() {
    let mut story = Story::new();
    let request = story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    assert_eq!(story.waiting(), [(request, device(2), 0)]);
    assert_eq!(story.owner(), (device(1), 0));
    story.by(9, &granted(request, 2, 1));
    assert_eq!(story.owner(), (device(2), 1));
    assert_eq!(story.waiting(), []);
    // And on: the next grant replaces lease 1.
    let back = story.by(1, &OrderEvent::OwnershipRequested { lease: lease(1) });
    story.by(9, &granted(back, 1, 2));
    assert_eq!(story.owner(), (device(1), 2));
    assert_eq!(story.kinds(), []);
}

#[test]
fn a_grant_from_a_lease_that_moved_on_doesnt_apply() {
    let mut story = Story::new();
    let request = story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    // An island's override folds first: the hub hadn't heard of it.
    story.by(3, &OrderEvent::OwnershipOverridden { lease: lease(0), reason: reason("island") });
    let stale = story.by(9, &granted(request, 2, 1));
    assert_eq!(story.owner(), (device(3), 1));
    assert_eq!(story.waiting(), [], "the grant still answers the request");
    assert_eq!(story.kinds(), [ConflictKind::Overridden(device(1)), ConflictKind::StaleGrant]);
    assert_eq!(story.order.conflicts()[1].event, stale);
    // A grant that skips a lease doesn't apply either.
    let mut story = Story::new();
    let request = story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    story.by(9, &granted(request, 2, 2));
    assert_eq!(story.owner(), (device(1), 0));
    assert_eq!(story.kinds(), [ConflictKind::StaleGrant]);
}

#[test]
fn a_refusal_answers_the_request_and_changes_nothing() {
    let mut story = Story::new();
    let request = story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    let refusal = Refusal::PaymentInProgress;
    story.by(9, &OrderEvent::OwnershipRefused { request, refusal });
    assert_eq!(story.owner(), (device(1), 0));
    assert_eq!(story.waiting(), []);
    assert_eq!(story.kinds(), []);
}

#[test]
fn an_override_takes_the_order_and_is_flagged_and_a_stale_one_doesnt() {
    let mut story = Story::new();
    story.by(2, &OrderEvent::OwnershipOverridden { lease: lease(0), reason: reason("island") });
    assert_eq!(story.owner(), (device(2), 1));
    assert_eq!(story.kinds(), [ConflictKind::Overridden(device(1))]);
    // Another island overrode the lease the first replaced.
    story.by(3, &OrderEvent::OwnershipOverridden { lease: lease(0), reason: reason("island") });
    assert_eq!(story.owner(), (device(2), 1));
    assert_eq!(story.kinds(), [ConflictKind::Overridden(device(1)), ConflictKind::StaleOverride]);
    // One from a lease not yet reached doesn't apply either.
    story.by(3, &OrderEvent::OwnershipOverridden { lease: lease(5), reason: reason("island") });
    assert_eq!(story.owner(), (device(2), 1));
    assert_eq!(story.kinds().last(), Some(&ConflictKind::StaleOverride));
}

#[test]
fn an_answer_can_fold_before_its_request() {
    // A hub whose clock is behind the requester's answers before the request in canonical
    // order: the request never waits, and the hub doesn't answer it again.
    let mut story = Story::new();
    let request: Id<Event> = id(0xE003);
    story.by(9, &OrderEvent::OwnershipRefused { request, refusal: Refusal::LeaseMoved });
    assert_eq!(story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) }), request);
    assert_eq!(story.waiting(), []);
    assert_eq!(story.order.answers(false, epoch()), []);
    // A grant before its request still gives the order away, by its lease.
    let mut story = Story::new();
    story.by(9, &granted(id(0xE003), 2, 1));
    story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    assert_eq!(story.owner(), (device(2), 1));
    assert_eq!(story.waiting(), []);
}

#[test]
fn an_answer_before_the_orders_creation_still_answers_its_request() {
    // A hub whose clock is far behind the devices' answers before even the order's creation in
    // canonical order. The grant can't apply there, but its request never waits, so the hub
    // doesn't answer it again at every write.
    let mut story = Story { order: Order::new(id(0xA)), events: 0 };
    let request: Id<Event> = id(0xE003);
    story.by(9, &granted(request, 2, 1));
    story.by(1, &OrderEvent::Created(created()));
    assert_eq!(story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) }), request);
    assert_eq!(story.owner(), (device(1), 0));
    assert_eq!(story.kinds(), [ConflictKind::BeforeCreation]);
    assert_eq!(story.waiting(), []);
    assert_eq!(story.order.answers(false, epoch()), []);
}

/// Line 1 moved to `check`.
fn moved(check: u64) -> LinesAllocated {
    LinesAllocated::new([Allocation { line: id(1), check: id(check), shares: NonZeroU16::MIN }])
        .unwrap()
}

/// One of each event only the owner may record.
fn owners_only() -> Vec<OrderEvent> {
    let notes = Some(Change::Set(crate::codec::Note::new("no salt").unwrap()));
    vec![
        OrderEvent::LineChanged(LineChanged { notes, ..LineChanged::to(id(1)) }),
        OrderEvent::LineRemoved { line: id(1) },
        OrderEvent::LineVoided { line: id(1), reason: reason("burnt") },
        OrderEvent::LineComped { line: id(1), reason: reason("birthday") },
        OrderEvent::Voided { reason: reason("walkout") },
        OrderEvent::Abandoned,
        OrderEvent::CheckOpened { check: id::<Check>(0xC1) },
        OrderEvent::LinesAllocated(moved(0xA)),
        OrderEvent::CheckClosed(CheckClosed {
            check: id(0xA),
            rules_version: RulesVersion::from_bytes([9; 32]),
            lines: Vec::new(),
            taxes: Vec::new(),
            total: Money::zero(Currency::from_code("USD").unwrap()),
            payments: None,
        }),
        OrderEvent::Closed,
        OrderEvent::Reopened { reason: reason("wrong_tender") },
    ]
}

#[test]
fn only_the_owners_structural_and_money_changes_go_unflagged() {
    for event in owners_only() {
        assert!(event.needs_ownership(), "{event:?}");
        let mut story = Story::new();
        story.by(1, &OrderEvent::LineAdded(added(1)));
        let before = story.order.clone();
        story.by(2, &event);
        let not_owner = ConflictKind::NotOwner { by: device(2), owner: device(1) };
        assert_eq!(story.kinds().first(), Some(&not_owner), "{event:?}");
        // The event still applies: the same as the owner recording it, but for the flag.
        let mut owners = Story { order: before, events: story.events.checked_sub(1).unwrap() };
        owners.by(1, &event);
        assert_eq!(story.order.lines(), owners.order.lines(), "{event:?}");
        assert_eq!(story.order.status(), owners.order.status(), "{event:?}");
        assert_eq!(story.order.checks(), owners.order.checks(), "{event:?}");
        assert_eq!(&story.kinds()[1..], owners.kinds(), "{event:?}");
    }
    // Anyone may add, fire and change attributes, and ask, override or answer.
    let mut story = Story::new();
    story.by(2, &OrderEvent::LineAdded(added(1)));
    story.by(3, &OrderEvent::LinesFired { lines: IdSet::new([id(1)]).unwrap() });
    let changed = AttributesChanged {
        guest_count: Some(Change::Set(NonZeroU16::new(4).unwrap())),
        ..Default::default()
    };
    story.by(2, &OrderEvent::AttributesChanged(changed));
    let request = story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    story.by(9, &OrderEvent::OwnershipRefused { request, refusal: Refusal::LeaseMoved });
    assert_eq!(story.kinds(), []);
    assert_eq!(*story.order.line(id(1)).unwrap().status(), LineStatus::Fired);
    // Before the order is created, or at another location, nothing is flagged: nothing applies.
    let mut stray = Story { order: Order::new(id(0xA)), events: 0 };
    stray.by(2, &OrderEvent::Closed);
    assert_eq!(stray.kinds(), [ConflictKind::BeforeCreation]);
}

/// A device's view of an order device 1 created, with line 1 on it, fired.
fn view() -> Story {
    let mut story = Story::new();
    story.by(1, &OrderEvent::LineAdded(added(1)));
    story.by(1, &OrderEvent::LinesFired { lines: IdSet::new([id(1)]).unwrap() });
    story
}

#[test]
fn commands_that_need_ownership_are_refused_to_other_devices() {
    let story = view();
    let owner_only = [
        OrderCommand::VoidLine { line: id(1), reason: reason("burnt") },
        OrderCommand::CompLine { line: id(1), reason: reason("birthday") },
        OrderCommand::Void(reason("walkout")),
        OrderCommand::Abandon,
        OrderCommand::OpenCheck(id(0xC1)),
        OrderCommand::AllocateLines(moved(0xA)),
        OrderCommand::Close,
        OrderCommand::Reopen(reason("wrong_tender")),
        OrderCommand::RemoveLine(id(1)),
        OrderCommand::ChangeLine(LineChanged {
            notes: Some(Change::Clear),
            ..LineChanged::to(id(1))
        }),
    ];
    for command in owner_only {
        assert!(command.needs_ownership(), "{command:?}");
        assert_eq!(
            story.order.decide(location(), device(2), command.clone()),
            Err(CommandError::NotOwner(device(1))),
            "{command:?}"
        );
    }
    // The owner gets past ownership to the command's own rules.
    let void = OrderCommand::VoidLine { line: id(1), reason: reason("burnt") };
    assert!(story.order.decide(location(), device(1), void).is_ok());
    // Anyone may add and fire lines, and change the order's attributes.
    let anyone = [
        OrderCommand::AddLine(added(2)),
        OrderCommand::ChangeAttributes(AttributesChanged {
            mode: Some(Mode::Takeout),
            ..Default::default()
        }),
    ];
    for command in anyone {
        assert!(!command.needs_ownership());
        assert!(story.order.decide(location(), device(2), command).is_ok());
    }
    // Location first, then ownership.
    let elsewhere = story.order.decide(id(0x101), device(2), OrderCommand::Close);
    assert_eq!(elsewhere, Err(CommandError::WrongLocation));
}

#[test]
fn a_device_asks_for_an_order_it_doesnt_own_once_at_a_time() {
    let mut story = view();
    let order = |story: &Story| story.order.clone();
    assert_eq!(
        order(&story).decide(location(), device(2), OrderCommand::RequestOwnership),
        Ok(OrderEvent::OwnershipRequested { lease: lease(0) })
    );
    assert_eq!(
        order(&story).decide(location(), device(1), OrderCommand::RequestOwnership),
        Err(CommandError::AlreadyOwner)
    );
    story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    assert_eq!(
        order(&story).decide(location(), device(2), OrderCommand::RequestOwnership),
        Err(CommandError::RequestPending)
    );
    // Another device may ask meanwhile; an override doesn't wait for anyone.
    assert!(order(&story).decide(location(), device(3), OrderCommand::RequestOwnership).is_ok());
    assert_eq!(
        order(&story).decide(location(), device(2), OrderCommand::OverrideOwnership(reason("x"))),
        Ok(OrderEvent::OwnershipOverridden { lease: lease(0), reason: reason("x") })
    );
    assert_eq!(
        order(&story).decide(location(), device(1), OrderCommand::OverrideOwnership(reason("x"))),
        Err(CommandError::AlreadyOwner)
    );
}

#[test]
fn a_closed_order_can_be_asked_for_to_reopen_it_and_an_ended_one_cant() {
    let mut closed = Story::new();
    closed.by(1, &OrderEvent::Closed);
    assert_eq!(*closed.order.status(), OrderStatus::Closed);
    let reopen = OrderCommand::Reopen(reason("wrong_tender"));
    assert_eq!(
        closed.order.decide(location(), device(2), reopen.clone()),
        Err(CommandError::NotOwner(device(1)))
    );
    assert!(closed.order.decide(location(), device(2), OrderCommand::RequestOwnership).is_ok());
    let takeover = OrderCommand::OverrideOwnership(reason("manager"));
    assert!(closed.order.decide(location(), device(2), takeover).is_ok());
    assert!(closed.order.decide(location(), device(1), reopen).is_ok());
    assert_eq!(
        closed.order.decide(id(0x101), device(2), OrderCommand::RequestOwnership),
        Err(CommandError::WrongLocation)
    );

    let mut voided = Story::new();
    voided.by(1, &OrderEvent::Voided { reason: reason("walkout") });
    for command in [OrderCommand::RequestOwnership, OrderCommand::OverrideOwnership(reason("x"))] {
        assert_eq!(
            voided.order.decide(location(), device(2), command),
            Err(CommandError::OrderClosed)
        );
    }
    let uncreated = Order::new(id(0xA));
    assert_eq!(
        uncreated.decide(location(), device(2), OrderCommand::RequestOwnership),
        Err(CommandError::NotCreated)
    );
}

#[test]
fn the_hub_grants_and_refuses_as_the_rules_say() {
    let refused = |request, refusal| OrderEvent::OwnershipRefused { request, refusal };
    // A request from the current lease, with no payment in progress: granted, under the next.
    let mut story = Story::new();
    let request = story.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    assert_eq!(story.order.answers(false, epoch()), [granted(request, 2, 1)]);
    // While a payment is in progress, refused.
    assert_eq!(story.order.answers(true, epoch()), [refused(request, Refusal::PaymentInProgress)]);
    // From a lease that has moved on, refused: whatever else holds.
    let mut moved = Story::new();
    let request = moved.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    moved.by(3, &OrderEvent::OwnershipOverridden { lease: lease(0), reason: reason("island") });
    assert_eq!(moved.order.answers(true, epoch()), [refused(request, Refusal::LeaseMoved)]);
    // From the device that already owns the order, refused.
    let mut own = Story::new();
    let request = own.by(1, &OrderEvent::OwnershipRequested { lease: lease(0) });
    assert_eq!(own.order.answers(false, epoch()), [refused(request, Refusal::AlreadyOwner)]);
    // Two requests from one lease: the first is granted, which moves the lease for the second.
    let mut two = Story::new();
    let first = two.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    let second = two.by(3, &OrderEvent::OwnershipRequested { lease: lease(0) });
    assert_eq!(
        two.order.answers(false, epoch()),
        [granted(first, 2, 1), refused(second, Refusal::LeaseMoved)]
    );
    // A request from the lease the first grant makes is granted after it.
    let mut chain = Story::new();
    let first = chain.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    let second = chain.by(3, &OrderEvent::OwnershipRequested { lease: lease(1) });
    assert_eq!(chain.order.answers(false, epoch()), [granted(first, 2, 1), granted(second, 3, 2)]);
    // Answered requests wait no more; no requests, no answers; no creation, no answers.
    for answer in chain.order.answers(false, epoch()) {
        chain.by(9, &answer);
    }
    assert_eq!(chain.order.answers(false, epoch()), []);
    assert_eq!(chain.owner(), (device(3), 2));
    let mut uncreated = Story { order: Order::new(id(0xA)), events: 0 };
    uncreated.by(2, &OrderEvent::OwnershipRequested { lease: lease(0) });
    assert_eq!(uncreated.order.answers(false, epoch()), []);
}

#[test]
fn leases_stay_in_range() {
    assert_eq!(Lease::FIRST.get(), 0);
    assert_eq!(Lease::FIRST.next(), Some(lease(1)));
    assert_eq!(Lease::MAX.get(), (1 << 63) - 1);
    assert_eq!(Lease::MAX.next(), None);
    assert_eq!(Lease::new(1 << 63), None);
}

#[test]
fn ownership_payloads_are_read_strictly() {
    let decode = |event: &OrderEvent| {
        let (schema, payload) = event.encode().unwrap();
        OrderEvent::decode(&schema, &payload)
    };
    let invalid = |field| Err(DecodeError::Malformed(PayloadError::Invalid(field)));
    // A grant never gives the first lease: it replaces one.
    let first = OrderEvent::OwnershipGranted(OwnershipGranted {
        request: id(1),
        device: device(2),
        lease: Lease::FIRST,
        epoch: epoch(),
    });
    assert_eq!(decode(&first), invalid("lease"));
    for event in [
        granted(id(1), 2, 1),
        OrderEvent::OwnershipRequested { lease: Lease::FIRST },
        OrderEvent::OwnershipRequested { lease: Lease::MAX },
        OrderEvent::OwnershipRefused { request: id(1), refusal: Refusal::AlreadyOwner },
        OrderEvent::OwnershipOverridden { lease: Lease::MAX, reason: reason("island") },
    ] {
        assert_eq!(decode(&event), Ok(event.clone()));
    }
    // Out of range, by hand: a lease past the largest, an epoch of 0, an unknown refusal.
    let schema = |event: &OrderEvent| event.encode().unwrap().0;
    let with = |event: &OrderEvent, key: u64, value: Value| {
        let entries = event.to_value().as_map().unwrap().clone().into_entries();
        let entries = entries.into_iter().map(|(field, old)| {
            if field == Value::Unsigned(key) { (field, value.clone()) } else { (field, old) }
        });
        let payload = Payload::new(&Value::Map(Map::from_entries(entries).unwrap())).unwrap();
        OrderEvent::decode(&schema(event), &payload)
    };
    let requested = OrderEvent::OwnershipRequested { lease: Lease::FIRST };
    let past = Value::Unsigned(1 << 63);
    assert_eq!(with(&requested, 1, past.clone()), invalid("lease"));
    let grant = granted(id(1), 2, 1);
    assert_eq!(with(&grant, 3, past), invalid("lease"));
    assert_eq!(with(&grant, 4, Value::Unsigned(0)), invalid("epoch"));
    let refusal = OrderEvent::OwnershipRefused { request: id(1), refusal: Refusal::LeaseMoved };
    assert_eq!(with(&refusal, 2, Value::Unsigned(3)), invalid("refusal"));
    assert_eq!(Refusal::ALL.iter().map(|refusal| refusal.code()).collect::<Vec<_>>(), [0, 1, 2]);
}
