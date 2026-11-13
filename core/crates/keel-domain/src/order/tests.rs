//! Known-answer tests for orders: the lifecycle, each conflict rule, skipped events, and each
//! command's checks, closing checks and orders included. Closing a check through its payments is
//! tested with checkout.

use core::num::{NonZeroU8, NonZeroU16};
use core::time::Duration;

use keel_events::cbor::Value;
use keel_events::envelope::{Actor, Event, Location, Payload, SchemaName, SchemaRef};
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_events::log::{EventDraft, LogConfig, LogHead, LogWriter};
use keel_types::{Currency, Hlc, Id, Money, Quantity, SeededEntropy, Timestamp, Unit};

use super::*;
use crate::aggregate::{Aggregate, EventMeta, fold};
use crate::codec::{
    CatalogVersion, Change, IdSet, Name, Note, PayloadError, ReasonCode, RulesVersion,
};
use crate::schema::{DecodeError, DomainEvent};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::from_code("USD").unwrap())
}

fn eur(minor: i64) -> Money {
    Money::from_minor(minor, Currency::from_code("EUR").unwrap())
}

fn each(count: i64) -> Quantity {
    Quantity::from_whole(count, Unit::Each).unwrap()
}

fn location() -> Id<Location> {
    id(0x100)
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

/// `shares` shares of `line` on `check`.
fn allocation(line: u64, check: u64, shares: u16) -> Allocation {
    Allocation { line: id(line), check: id(check), shares: NonZeroU16::new(shares).unwrap() }
}

/// `shares` shares of a line on `check`, as the line records them.
fn share(check: u64, shares: u16) -> CheckShare {
    CheckShare { check: id(check), shares: NonZeroU16::new(shares).unwrap() }
}

/// The lines numbered `lines`.
fn lines(lines: &[u64]) -> IdSet<Line> {
    IdSet::new(lines.iter().map(|&n| id(n))).unwrap()
}

fn created() -> OrderCreated {
    OrderCreated {
        channel: Channel::Pos,
        mode: Mode::DineIn,
        currency: Currency::from_code("USD").unwrap(),
        revenue_center: None,
        table: Some(id(0x200)),
        guest_count: NonZeroU16::new(2),
        customer: None,
        owner: Some(id(0x300)),
    }
}

fn modifier(n: u64, price: Money) -> ChosenModifier {
    ChosenModifier {
        modifier: id(n),
        name: Name::new("Oat milk").unwrap(),
        prefix: Prefix::Plain,
        quantity: NonZeroU8::MIN,
        placement: Placement::Whole,
        unit_price: price,
        modifiers: Vec::new(),
    }
}

fn added(line: u64, price: Money) -> LineAdded {
    LineAdded {
        line: id(line),
        item: ItemSnapshot {
            variant: id(0x400),
            catalog_version: CatalogVersion::from_bytes([7; 32]),
            name: Name::new("Flat white").unwrap(),
            tax_category: id(0x500),
            unit_price: price,
        },
        quantity: each(1),
        modifiers: vec![modifier(0x600, Money::zero(price.currency()))],
        seat: None,
        course: None,
        notes: None,
    }
}

/// A snapshot closing `check`, charging each line the amount given, untaxed, and settled by one
/// payment when the total is more than zero.
fn closed(check: u64, charges: &[(u64, i64)]) -> CheckClosed {
    let lines: Vec<LineCharge> = charges
        .iter()
        .map(|&(line, amount)| LineCharge {
            line: id(line),
            gross: usd(amount),
            net: usd(amount),
            tax: usd(0),
        })
        .collect();
    let total = usd(charges.iter().map(|&(_, amount)| amount).sum());
    CheckClosed {
        check: id(check),
        rules_version: RulesVersion::from_bytes([9; 32]),
        lines,
        taxes: Vec::new(),
        total,
        payments: total.is_positive().then(|| IdSet::new([id(0xB1)]).unwrap()),
    }
}

/// The checks' identifiers and numbers.
fn numbered(order: &Order) -> Vec<(Id<Check>, u32)> {
    order.checks().iter().map(|check| (check.id(), check.number().get())).collect()
}

/// Applies events to an order, each with fresh metadata, as a replica folding them in canonical
/// order would.
struct Script {
    order: Order,
    events: u64,
}

impl Script {
    fn new() -> Script {
        Script { order: Order::new(id(0xA)), events: 0 }
    }

    fn created() -> Script {
        let mut script = Script::new();
        script.apply(&OrderEvent::Created(created()));
        script
    }

    fn meta_at(&mut self, location: Id<Location>) -> EventMeta {
        self.events = self.events.checked_add(1).unwrap();
        EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location,
            origin_device: id(0xD),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_790_600_000_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-09-28".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        }
    }

    fn apply(&mut self, event: &OrderEvent) -> Id<Event> {
        self.apply_at(location(), event)
    }

    fn apply_at(&mut self, location: Id<Location>, event: &OrderEvent) -> Id<Event> {
        let meta = self.meta_at(location);
        self.order.apply(&meta, event);
        meta.event_id
    }

    fn kinds(&self) -> Vec<ConflictKind> {
        self.order.conflicts().iter().map(|conflict| conflict.kind).collect()
    }

    fn status(&self, line: u64) -> LineStatus {
        self.order.line(id(line)).unwrap().status().clone()
    }

    fn fire(&mut self, lines: &[u64]) -> Id<Event> {
        let lines = IdSet::new(lines.iter().map(|&line| id(line))).unwrap();
        self.apply(&OrderEvent::LinesFired { lines })
    }
}

#[test]
fn a_dinner_goes_through_its_life() {
    let mut script = Script::created();
    let info = script.order.info().unwrap().clone();
    assert_eq!(info.location, location());
    assert_eq!(
        (info.channel, info.mode, info.table, info.owner),
        (Channel::Pos, Mode::DineIn, Some(id(0x200)), Some(id(0x300)))
    );
    assert_eq!(script.order.stage(), Stage::Draft);

    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(1200))));
    assert_eq!(script.order.stage(), Stage::Open);
    let mut changed = LineChanged::to(id(2));
    changed.quantity = Some(each(2));
    changed.seat = Some(Change::Set(NonZeroU16::MIN));
    changed.notes = Some(Change::Set(Note::new("no salt").unwrap()));
    script.apply(&OrderEvent::LineChanged(changed));
    let line = script.order.line(id(2)).unwrap();
    assert_eq!(
        (line.quantity(), line.seat(), line.notes().map(Note::as_str)),
        (each(2), NonZeroU16::new(1), Some("no salt"))
    );

    script.fire(&[1, 2]);
    assert_eq!(script.order.stage(), Stage::Submitted);
    script.apply(&OrderEvent::LineVoided { line: id(1), reason: reason("kitchen_error") });
    script.apply(&OrderEvent::LineComped { line: id(2), reason: reason("birthday") });
    assert_eq!(script.status(1), LineStatus::Voided(reason("kitchen_error")));
    assert_eq!(script.order.line(id(2)).unwrap().comp(), Some(&reason("birthday")));
    assert_eq!(script.order.live_lines().map(Line::id).collect::<Vec<_>>(), [id(2)]);

    let attributes = AttributesChanged {
        table: Some(Change::Clear),
        mode: Some(Mode::Takeout),
        ..AttributesChanged::default()
    };
    script.apply(&OrderEvent::AttributesChanged(attributes));
    let info = script.order.info().unwrap();
    assert_eq!((info.table, info.mode), (None, Mode::Takeout));

    script.apply(&OrderEvent::Voided { reason: reason("walkout") });
    assert_eq!(*script.order.status(), OrderStatus::Voided(reason("walkout")));
    assert_eq!(script.kinds(), []);
    assert_eq!(script.order.id(), id(0xA));
    assert!(script.order.skipped().is_empty() && !script.order.needs_update());
}

#[test]
fn concurrent_edits_resolve_by_the_rules() {
    // Lines added on different devices are all kept; the last attribute change wins.
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(450))));
    for table in [0x201, 0x202] {
        let changed = AttributesChanged {
            table: Some(Change::Set(id(table))),
            ..AttributesChanged::default()
        };
        script.apply(&OrderEvent::AttributesChanged(changed));
    }
    assert_eq!(script.order.info().unwrap().table, Some(id(0x202)));
    assert_eq!(script.order.lines().len(), 2);

    // A change after a removal isn't applied: the removal wins.
    script.apply(&OrderEvent::LineRemoved { line: id(1) });
    let mut changed = LineChanged::to(id(1));
    changed.quantity = Some(each(3));
    let change = script.apply(&OrderEvent::LineChanged(changed));
    assert_eq!(script.order.line(id(1)).unwrap().quantity(), each(1));
    assert_eq!(
        script.order.conflicts(),
        [Conflict { event: change, kind: ConflictKind::ChangedAfterRemoval(id(1)) }]
    );

    // A change after a fire is applied, and reported.
    script.fire(&[2]);
    let mut changed = LineChanged::to(id(2));
    changed.course = Some(Change::Set(NonZeroU8::MIN));
    script.apply(&OrderEvent::LineChanged(changed));
    assert_eq!(script.order.line(id(2)).unwrap().course(), NonZeroU8::new(1));
    assert_eq!(script.kinds().last(), Some(&ConflictKind::ChangedAfterFire(id(2))));

    // A void wins over a later fire, which the kitchen must hear about.
    script.apply(&OrderEvent::LineVoided { line: id(2), reason: reason("wrong_item") });
    script.fire(&[2]);
    assert_eq!(script.status(2), LineStatus::Voided(reason("wrong_item")));
    assert_eq!(script.kinds().last(), Some(&ConflictKind::FiredAfterRemoval(id(2))));

    // The first void, removal or comp wins.
    script.apply(&OrderEvent::LineVoided { line: id(2), reason: reason("second_reason") });
    script.apply(&OrderEvent::LineRemoved { line: id(2) });
    assert_eq!(script.status(2), LineStatus::Voided(reason("wrong_item")));
    assert_eq!(script.order.conflicts().len(), 3);
}

#[test]
fn removal_and_firing_in_either_order_are_reported() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(100))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(100))));
    script.fire(&[1]);
    script.apply(&OrderEvent::LineRemoved { line: id(1) });
    script.apply(&OrderEvent::LineRemoved { line: id(2) });
    script.fire(&[2]);
    assert_eq!((script.status(1), script.status(2)), (LineStatus::Removed, LineStatus::Removed));
    assert_eq!(
        script.kinds(),
        [ConflictKind::RemovedAfterFire(id(1)), ConflictKind::FiredAfterRemoval(id(2))]
    );
    script.apply(&OrderEvent::LineComped { line: id(1), reason: reason("goodwill") });
    assert_eq!(script.kinds().last(), Some(&ConflictKind::CompedAfterRemoval(id(1))));
    assert_eq!(script.order.line(id(1)).unwrap().comp(), None);
}

#[test]
fn the_first_comp_wins_and_comped_lines_stay_live() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(100))));
    script.apply(&OrderEvent::LineComped { line: id(1), reason: reason("birthday") });
    script.apply(&OrderEvent::LineComped { line: id(1), reason: reason("goodwill") });
    let line = script.order.line(id(1)).unwrap();
    assert_eq!((line.comp(), line.is_live()), (Some(&reason("birthday")), true));
    assert_eq!(script.kinds(), []);
}

#[test]
fn events_that_would_break_the_order_are_not_applied() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(100))));

    // Another currency, a duplicate line, another unit, unknown lines.
    script.apply(&OrderEvent::LineAdded(added(2, eur(100))));
    let mut duplicate = added(1, usd(999));
    duplicate.quantity = each(5);
    script.apply(&OrderEvent::LineAdded(duplicate));
    let mut changed = LineChanged::to(id(1));
    changed.quantity = Some(Quantity::from_whole(1, Unit::Kilogram).unwrap());
    script.apply(&OrderEvent::LineChanged(changed));
    let mut changed = LineChanged::to(id(1));
    changed.modifiers = Some(vec![modifier(0x601, eur(50))]);
    script.apply(&OrderEvent::LineChanged(changed));
    let mut changed = LineChanged::to(id(1));
    changed.modifiers = Some(Vec::new());
    script.apply(&OrderEvent::LineChanged(changed));
    for event in [
        OrderEvent::LineChanged(LineChanged { quantity: Some(each(2)), ..LineChanged::to(id(9)) }),
        OrderEvent::LineRemoved { line: id(9) },
        OrderEvent::LineVoided { line: id(9), reason: reason("x") },
        OrderEvent::LineComped { line: id(9), reason: reason("x") },
    ] {
        script.apply(&event);
    }
    script.fire(&[9]);

    assert_eq!(script.order.lines().len(), 1);
    let line = script.order.line(id(1)).unwrap();
    assert_eq!(
        (line.item().unit_price, line.quantity(), line.modifiers()),
        (usd(100), each(1), &[][..])
    );
    assert_eq!(
        script.kinds(),
        [
            ConflictKind::CurrencyMismatch(id(2)),
            ConflictKind::DuplicateLine(id(1)),
            ConflictKind::UnitMismatch(id(1)),
            ConflictKind::CurrencyMismatch(id(1)),
            ConflictKind::UnknownLine(id(9)),
            ConflictKind::UnknownLine(id(9)),
            ConflictKind::UnknownLine(id(9)),
            ConflictKind::UnknownLine(id(9)),
            ConflictKind::UnknownLine(id(9)),
        ]
    );
}

#[test]
fn events_outside_the_order_are_not_applied() {
    let mut script = Script::new();
    script.apply(&OrderEvent::LineAdded(added(1, usd(100))));
    assert_eq!(script.kinds(), [ConflictKind::BeforeCreation]);
    assert!(script.order.info().is_none() && script.order.lines().is_empty());

    script.apply(&OrderEvent::Created(created()));
    let mut again = created();
    again.mode = Mode::Takeout;
    script.apply(&OrderEvent::Created(again));
    assert_eq!(script.order.info().unwrap().mode, Mode::DineIn);
    script.apply_at(id(0x101), &OrderEvent::LineAdded(added(1, usd(100))));
    assert!(script.order.lines().is_empty());
    assert_eq!(
        script.kinds(),
        [
            ConflictKind::BeforeCreation,
            ConflictKind::DuplicateCreation,
            ConflictKind::WrongLocation
        ]
    );
}

#[test]
fn closed_orders_report_new_work() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(100))));
    script.apply(&OrderEvent::Voided { reason: reason("walkout") });
    script.apply(&OrderEvent::LineAdded(added(2, usd(100))));
    script.fire(&[1, 2]);
    script.apply(&OrderEvent::Abandoned);
    assert_eq!(*script.order.status(), OrderStatus::Voided(reason("walkout")));
    assert_eq!(script.status(2), LineStatus::Fired);
    assert_eq!(
        script.kinds(),
        [
            ConflictKind::AddedToClosedOrder(id(2)),
            ConflictKind::FiredOnClosedOrder(id(1)),
            ConflictKind::FiredOnClosedOrder(id(2)),
        ]
    );

    let mut empty = Script::created();
    empty.apply(&OrderEvent::Abandoned);
    assert_eq!((empty.order.status(), empty.kinds()), (&OrderStatus::Abandoned, vec![]));
    let mut busy = Script::created();
    busy.apply(&OrderEvent::LineAdded(added(1, usd(100))));
    busy.apply(&OrderEvent::Abandoned);
    assert_eq!(busy.kinds(), [ConflictKind::AbandonedWithLines]);

    // Lines that were fired may have been made, even once they are voided or removed, or when the
    // fire came after the removal: abandoning the order hides that work, so it is reported.
    let mut made = Script::created();
    made.apply(&OrderEvent::LineAdded(added(1, usd(100))));
    made.fire(&[1]);
    made.apply(&OrderEvent::LineVoided { line: id(1), reason: reason("wrong_item") });
    made.apply(&OrderEvent::Abandoned);
    assert_eq!(made.kinds(), [ConflictKind::AbandonedWithLines]);
    let mut late = Script::created();
    late.apply(&OrderEvent::LineAdded(added(1, usd(100))));
    late.apply(&OrderEvent::LineRemoved { line: id(1) });
    late.fire(&[1]);
    late.apply(&OrderEvent::Abandoned);
    assert!(late.order.line(id(1)).unwrap().was_fired());
    assert_eq!(
        late.kinds(),
        [ConflictKind::FiredAfterRemoval(id(1)), ConflictKind::AbandonedWithLines]
    );
}

#[test]
fn an_order_is_priced_from_its_live_lines() {
    let mut script = Script::new();
    assert!(script.order.basket().is_none());
    script.apply(&OrderEvent::Created(created()));
    // Two lattes at 4.50, each with two oat milks at 0.50 that each have a syrup at 0.25:
    // 4.50 + 2 × (0.50 + 0.25) = 6.00 a latte, 12.00 for two.
    let mut latte = added(1, usd(450));
    latte.quantity = each(2);
    latte.modifiers = vec![ChosenModifier {
        quantity: NonZeroU8::new(2).unwrap(),
        modifiers: vec![modifier(0x601, usd(25))],
        ..modifier(0x600, usd(50))
    }];
    script.apply(&OrderEvent::LineAdded(latte));
    script.apply(&OrderEvent::LineAdded(added(2, usd(325))));
    script.apply(&OrderEvent::LineAdded(added(3, usd(999))));
    script.apply(&OrderEvent::LineComped { line: id(2), reason: reason("birthday") });
    script.apply(&OrderEvent::LineRemoved { line: id(3) });

    let basket = script.order.basket().unwrap();
    assert_eq!(basket.dining, keel_pricing::Dining::OnPremises);
    let live: Vec<Id<Line>> = script.order.live_lines().map(Line::id).collect();
    assert_eq!(live, [id(1), id(2)]);
    assert_eq!(basket.lines.len(), live.len());
    assert!(basket.lines[1].comped);

    let rules = keel_pricing::Rules {
        taxes: vec![keel_pricing::Tax {
            id: id(0x900),
            name: "Ten percent".to_owned(),
            rate: keel_types::Rate::from_basis_points(1_000),
            categories: vec![id(0x500)],
            dining: None,
        }],
        ..keel_pricing::Rules::untaxed()
    };
    let totals = keel_pricing::price(&basket, &rules).unwrap();
    // The latte is 12.00 with 1.20 of tax; the comped bagel costs nothing.
    assert_eq!(totals.lines[0].gross, usd(1_200));
    assert_eq!(totals.lines[1].comp, usd(325));
    assert_eq!(totals.total, usd(1_320));

    script.apply(&OrderEvent::AttributesChanged(AttributesChanged {
        mode: Some(Mode::Takeout),
        ..AttributesChanged::default()
    }));
    assert_eq!(script.order.basket().unwrap().dining, keel_pricing::Dining::ToGo);
}

#[test]
fn a_table_is_split_among_checks() {
    let mut script = Script::created();
    // The main check comes with the order, and has its identifier.
    assert_eq!(script.order.main_check(), id(0xA));
    script.apply(&OrderEvent::LineAdded(added(1, usd(1450))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(495))));
    script.apply(&OrderEvent::LineAdded(added(3, usd(3000))));
    assert_eq!(script.order.line(id(3)).unwrap().allocation(), [share(0xA, 1)]);

    // Seat 1's burger goes to check 2 and seat 2's fries to check 3, and the three checks share
    // the wine.
    script.apply(&OrderEvent::CheckOpened { check: id(0xC2) });
    script.apply(&OrderEvent::CheckOpened { check: id(0xC3) });
    let split = LinesAllocated::new([
        allocation(1, 0xC2, 1),
        allocation(2, 0xC3, 1),
        allocation(3, 0xA, 1),
        allocation(3, 0xC2, 1),
        allocation(3, 0xC3, 1),
    ])
    .unwrap();
    script.apply(&OrderEvent::LinesAllocated(split));
    assert_eq!(script.kinds(), []);
    let numbers: Vec<(Id<Check>, u32)> =
        script.order.checks().iter().map(|check| (check.id(), check.number().get())).collect();
    assert_eq!(numbers, [(id(0xA), 1), (id(0xC2), 2), (id(0xC3), 3)]);
    assert_eq!(
        script.order.line(id(3)).unwrap().allocation(),
        [share(0xA, 1), share(0xC2, 1), share(0xC3, 1)]
    );

    // Each check is priced as its own sale, with an 8.875% tax: a third of the wine is 10.00.
    // Check 1 is 10.00, taxed 0.8875, so 0.89; check 2 is 14.50 + 10.00 = 24.50, taxed 2.174375,
    // so 2.17; check 3 is 4.95 + 10.00 = 14.95, taxed 1.3268125, so 1.33.
    let rules = keel_pricing::Rules {
        taxes: vec![keel_pricing::Tax {
            id: id(0x900),
            name: "Sales tax".to_owned(),
            rate: keel_types::Rate::from_percent("8.875".parse().unwrap()).unwrap(),
            categories: vec![id(0x500)],
            dining: None,
        }],
        ..keel_pricing::Rules::untaxed()
    };
    let totals: Vec<(Money, Money)> = [0xA, 0xC2, 0xC3]
        .into_iter()
        .map(|check| {
            let basket = script.order.check_basket(id(check)).unwrap();
            let totals = keel_pricing::price(&basket, &rules).unwrap();
            (totals.gross, totals.total)
        })
        .collect();
    assert_eq!(totals, [(usd(1000), usd(1089)), (usd(2450), usd(2667)), (usd(1495), usd(1628))]);
    assert!(script.order.check_basket(id(0xC4)).is_none());

    // A new line goes to the main check.
    script.apply(&OrderEvent::LineAdded(added(4, usd(895))));
    assert_eq!(script.order.line(id(4)).unwrap().allocation(), [share(0xA, 1)]);
}

#[test]
fn concurrent_splits_resolve_by_the_rules() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(1000))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(500))));

    // An allocation to a check that doesn't exist yet doesn't apply to the lines it names, and
    // the check is reported once.
    let early = script
        .apply(&OrderEvent::LinesAllocated(LinesAllocated::moving(&lines(&[1, 2]), id(0xC2))));
    assert_eq!(script.kinds(), [ConflictKind::UnknownCheck(id(0xC2))]);
    assert_eq!(script.order.conflicts()[0].event, early);
    assert_eq!(script.order.line(id(1)).unwrap().allocation(), [share(0xA, 1)]);

    // A check opened twice keeps its first opening; the main check can't be opened again.
    script.apply(&OrderEvent::CheckOpened { check: id(0xC2) });
    script.apply(&OrderEvent::CheckOpened { check: id(0xC2) });
    script.apply(&OrderEvent::CheckOpened { check: id(0xA) });
    assert_eq!(script.order.checks().len(), 2);

    // Two devices split the same line differently: the later allocation wins.
    script.apply(&OrderEvent::LinesAllocated(LinesAllocated::moving(&lines(&[1]), id(0xC2))));
    let both = IdSet::new([id(0xA), id(0xC2)]).unwrap();
    script.apply(&OrderEvent::LinesAllocated(LinesAllocated::splitting(&lines(&[1]), &both)));
    assert_eq!(script.order.line(id(1)).unwrap().allocation(), [share(0xA, 1), share(0xC2, 1)]);

    // Allocating a removed line changes nothing; a line the order doesn't have is reported.
    script.apply(&OrderEvent::LineRemoved { line: id(2) });
    script.apply(&OrderEvent::LinesAllocated(LinesAllocated::moving(&lines(&[2, 9]), id(0xC2))));
    assert_eq!(script.order.line(id(2)).unwrap().allocation(), [share(0xA, 1)]);
    assert_eq!(
        script.kinds(),
        [
            ConflictKind::UnknownCheck(id(0xC2)),
            ConflictKind::DuplicateCheck(id(0xC2)),
            ConflictKind::DuplicateCheck(id(0xA)),
            ConflictKind::UnknownLine(id(9)),
        ]
    );
}

#[test]
fn a_closed_check_freezes_what_its_lines_cost() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(1000))));
    script.apply(&OrderEvent::LineAdded(added(3, usd(300))));
    script.apply(&OrderEvent::CheckOpened { check: id(0xC2) });
    // The two checks share line 2; line 3 is check 2's alone.
    let split = LinesAllocated::new([
        allocation(2, 0xA, 1),
        allocation(2, 0xC2, 1),
        allocation(3, 0xC2, 1),
    ])
    .unwrap();
    script.apply(&OrderEvent::LinesAllocated(split));
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450), (2, 500)])));
    assert_eq!(script.kinds(), []);
    let main = script.order.check(id(0xA)).unwrap();
    assert!(!main.is_open());
    assert_eq!(main.closed().unwrap().total, usd(950));
    let frozen = |script: &Script, n| script.order.is_frozen(script.order.line(id(n)).unwrap());
    assert!(frozen(&script, 1) && frozen(&script, 2) && !frozen(&script, 3));

    // Seats, courses and notes don't change what a line costs; a quantity does. Either applies.
    let notes = LineChanged {
        notes: Some(Change::Set(Note::new("no ice").unwrap())),
        ..LineChanged::to(id(1))
    };
    script.apply(&OrderEvent::LineChanged(notes));
    assert_eq!(script.kinds(), []);
    let more = LineChanged { quantity: Some(each(2)), ..LineChanged::to(id(1)) };
    script.apply(&OrderEvent::LineChanged(more));
    assert_eq!(script.order.line(id(1)).unwrap().quantity(), each(2));
    // A paid order is still prepared: firing is never frozen.
    script.fire(&[1, 2, 3]);
    // A comp and a void apply, and are reported: the snapshot stands.
    script.apply(&OrderEvent::LineComped { line: id(2), reason: reason("birthday") });
    script.apply(&OrderEvent::LineVoided { line: id(1), reason: reason("kitchen_error") });
    assert_eq!(script.order.line(id(2)).unwrap().comp(), Some(&reason("birthday")));
    assert_eq!(script.status(1), LineStatus::Voided(reason("kitchen_error")));
    // An allocation can't move a line off a closed check, or onto one.
    script.apply(&OrderEvent::LinesAllocated(LinesAllocated::moving(&lines(&[2]), id(0xC2))));
    script.apply(&OrderEvent::LinesAllocated(LinesAllocated::moving(&lines(&[3]), id(0xA))));
    assert_eq!(script.order.line(id(2)).unwrap().allocation(), [share(0xA, 1), share(0xC2, 1)]);
    assert_eq!(script.order.line(id(3)).unwrap().allocation(), [share(0xC2, 1)]);
    assert_eq!(script.order.check(id(0xA)).unwrap().closed().unwrap().total, usd(950));
    assert_eq!(
        script.kinds(),
        [
            ConflictKind::ChangedOnClosedCheck(id(1)),
            ConflictKind::ChangedOnClosedCheck(id(2)),
            ConflictKind::ChangedOnClosedCheck(id(1)),
            ConflictKind::AllocatedOnClosedCheck(id(2)),
            ConflictKind::AllocatedOnClosedCheck(id(3)),
        ]
    );
}

#[test]
fn lines_a_close_left_off_move_to_an_open_check() {
    let mut script = Script::created();
    for (line, price) in [(1, 450), (2, 300), (3, 1000), (4, 800)] {
        script.apply(&OrderEvent::LineAdded(added(line, usd(price))));
    }
    script.apply(&OrderEvent::CheckOpened { check: id(0xC2) });
    let split = LinesAllocated::new([
        allocation(3, 0xA, 1),
        allocation(3, 0xC2, 1),
        allocation(4, 0xC2, 1),
    ])
    .unwrap();
    script.apply(&OrderEvent::LinesAllocated(split));

    // A device closes the main check having seen only line 1 on it, charging it for line 4 too,
    // which another device moved to check 2, and for a line the order doesn't have.
    let close =
        script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450), (4, 800), (9, 100)])));
    // Line 2 moves to check 2. Check 2 already has a part of line 3, so the main check's part
    // goes to a new check, opened for it, with the close's identifier.
    let post: Id<Check> = close.cast();
    assert_eq!(numbered(&script.order), [(id(0xA), 1), (id(0xC2), 2), (post, 3)]);
    assert_eq!(script.order.line(id(2)).unwrap().allocation(), [share(0xC2, 1)]);
    assert_eq!(
        script.order.line(id(3)).unwrap().allocation(),
        [share(0xC2, 1), CheckShare { check: post, shares: NonZeroU16::MIN }]
    );
    assert_eq!(
        script.kinds(),
        [
            ConflictKind::ChargedOffCheck(id(4)),
            ConflictKind::UnknownLine(id(9)),
            ConflictKind::LeftOffCheck(id(2)),
            ConflictKind::LeftOffCheck(id(3)),
        ]
    );

    // A check closes once; a close of a check the order doesn't have, or in another currency,
    // isn't applied.
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450)])));
    script.apply(&OrderEvent::CheckClosed(closed(0xC9, &[(1, 450)])));
    let mut euros = closed(0xC2, &[(2, 300)]);
    euros.total = eur(300);
    script.apply(&OrderEvent::CheckClosed(euros));
    assert!(script.order.check(id(0xC2)).unwrap().is_open());
    assert_eq!(script.order.check(id(0xA)).unwrap().closed().unwrap().lines.len(), 3);
    assert_eq!(
        script.kinds()[4..],
        [
            ConflictKind::DuplicateClose(id(0xA)),
            ConflictKind::UnknownCheck(id(0xC9)),
            ConflictKind::CheckCurrencyMismatch(id(0xC2)),
        ]
    );
}

#[test]
fn new_lines_go_to_the_first_open_check() {
    let mut script = Script::created();
    script.apply(&OrderEvent::CheckOpened { check: id(0xC2) });
    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450)])));
    script.apply(&OrderEvent::LineAdded(added(2, usd(300))));
    assert_eq!(script.order.line(id(2)).unwrap().allocation(), [share(0xC2, 1)]);
    script.apply(&OrderEvent::CheckClosed(closed(0xC2, &[(2, 300)])));

    // Every check is closed: the next line gets a check of its own, with its addition's
    // identifier. The order is still active, so that isn't a conflict.
    let third = script.apply(&OrderEvent::LineAdded(added(3, usd(500))));
    let post: Id<Check> = third.cast();
    assert_eq!(
        script.order.line(id(3)).unwrap().allocation(),
        [CheckShare { check: post, shares: NonZeroU16::MIN }]
    );
    assert_eq!(numbered(&script.order)[2], (post, 3));
    assert_eq!(script.kinds(), []);

    // The order is closed with that check still open, and more is added: both are unpaid.
    script.apply(&OrderEvent::Closed);
    script.apply(&OrderEvent::LineAdded(added(4, usd(250))));
    assert_eq!(*script.order.status(), OrderStatus::Closed);
    assert_eq!(
        script.order.line(id(4)).unwrap().allocation(),
        [CheckShare { check: post, shares: NonZeroU16::MIN }]
    );
    assert_eq!(
        script.kinds(),
        [ConflictKind::ClosedWithOpenCheck(post), ConflictKind::AddedToClosedOrder(id(4))]
    );
}

#[test]
fn orders_close_and_reopen() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450)])));
    script.apply(&OrderEvent::Closed);
    script.apply(&OrderEvent::Closed);
    assert_eq!(*script.order.status(), OrderStatus::Closed);
    // Reopening reopens the order and every check; the snapshots are history.
    script.apply(&OrderEvent::Reopened { reason: reason("wrong_tender") });
    assert_eq!(*script.order.status(), OrderStatus::Active);
    assert!(script.order.checks().iter().all(Check::is_open));
    assert!(!script.order.is_frozen(script.order.line(id(1)).unwrap()));
    script.apply(&OrderEvent::LineRemoved { line: id(1) });

    // A closed order can be voided concurrently; nothing reopens or closes a voided order.
    script.apply(&OrderEvent::LineAdded(added(2, usd(450))));
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(2, 450)])));
    script.apply(&OrderEvent::Closed);
    script.apply(&OrderEvent::Voided { reason: reason("walkout") });
    script.apply(&OrderEvent::Reopened { reason: reason("wrong_tender") });
    script.apply(&OrderEvent::Closed);
    assert_eq!(*script.order.status(), OrderStatus::Voided(reason("walkout")));
    assert!(!script.order.check(id(0xA)).unwrap().is_open());
    assert_eq!(script.kinds(), []);
}

#[test]
fn forged_check_identifiers_leave_lines_where_they_were() {
    // Check 0xE006 has the identifier the sixth event will have.
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::CheckOpened { check: id(0xE006) });
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450)])));
    script.apply(&OrderEvent::CheckClosed(closed(0xE006, &[(1, 450)])));
    // Event 6: every check is closed, and the new check would need an identifier in use.
    script.apply(&OrderEvent::LineAdded(added(2, usd(300))));
    assert_eq!(script.order.line(id(2)).unwrap().allocation(), [share(0xA, 1)]);
    assert_eq!(script.order.checks().len(), 2);

    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(300))));
    script.apply(&OrderEvent::CheckOpened { check: id(0xE006) });
    script.apply(&OrderEvent::CheckClosed(closed(0xE006, &[(2, 300)])));
    // Event 6 closes the main check without line 2, which has nowhere to go.
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450)])));
    assert_eq!(script.order.line(id(2)).unwrap().allocation(), [share(0xA, 1)]);
    assert_eq!(
        script.kinds(),
        [ConflictKind::ChargedOffCheck(id(2)), ConflictKind::LeftOffCheck(id(2))]
    );
}

#[test]
fn undecodable_events_are_skipped_and_reported() {
    let mut order = Script::created().order;
    let meta = Script::new().meta_at(location());
    let newer = SchemaRef {
        name: SchemaName::new("order.line_added").unwrap(),
        version: 2.try_into().unwrap(),
    };
    order.skip(&meta, &newer, &DecodeError::UnknownSchema);
    assert!(order.needs_update());
    let malformed = DecodeError::Malformed(PayloadError::Missing("line"));
    order.skip(&meta, &newer, &malformed);
    assert_eq!(order.skipped().len(), 2);
    assert_eq!(order.skipped()[1].reason, malformed);
}

/// Signs `events` on one device, as its log writer would.
fn signed(
    events: &[OrderEvent],
    stream: Id<Order>,
    kind: &str,
) -> Vec<keel_events::event::SignedEvent> {
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
        .map(|event| {
            let (schema, payload) = event.encode().unwrap();
            let draft = EventDraft {
                stream: keel_events::envelope::StreamRef {
                    kind: keel_events::envelope::StreamKind::new(kind).unwrap(),
                    id: stream.cast(),
                },
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

#[test]
fn signed_events_fold_into_their_stream_only() {
    let events = [OrderEvent::Created(created()), OrderEvent::LineAdded(added(1, usd(450)))];
    let mut order = Order::new(id(0xA));
    for event in signed(&events, id(0xA), "order") {
        fold(&mut order, &event);
    }
    assert_eq!(order.lines().len(), 1);
    assert_eq!(order.conflicts(), []);

    // Events of another order, or of another kind of stream, are skipped.
    for event in
        signed(&events, id(0xB), "order").iter().chain(&signed(&events, id(0xA), "payment"))
    {
        fold(&mut order, event);
    }
    assert_eq!(order.skipped().len(), 4);
    assert!(order.skipped().iter().all(|skipped| skipped.reason == DecodeError::WrongStream));

    // An event whose payload doesn't fit its schema is skipped as malformed.
    let payload = Payload::new(&Value::Map(keel_events::cbor::Map::new())).unwrap();
    let schema = OrderEvent::LineRemoved { line: id(1) }.encode().unwrap().0;
    assert!(matches!(
        OrderEvent::decode(&schema, &payload),
        Err(DecodeError::Malformed(PayloadError::Missing("line")))
    ));
}

/// A device working on an order through commands. Every event its valid commands produce must
/// apply to its own view without a conflict.
struct Device {
    order: Order,
    script: Script,
    /// The last event it recorded.
    last: Option<OrderEvent>,
}

impl Device {
    fn new() -> Device {
        Device { order: Order::new(id(0xA)), script: Script::new(), last: None }
    }

    /// A device with an open order holding line 1, fired or not.
    fn with_line(fired: bool) -> Device {
        let mut device = Device::new();
        device.run(OrderCommand::Create(created())).unwrap();
        device.run(OrderCommand::AddLine(added(1, usd(100)))).unwrap();
        if fired {
            device.run(OrderCommand::FireLines(IdSet::new([id(1)]).unwrap())).unwrap();
        }
        device
    }

    fn run(&mut self, command: OrderCommand) -> Result<(), CommandError> {
        let event = self.order.decide(location(), id(0xD), command)?;
        self.record(&event);
        self.last = Some(event);
        Ok(())
    }

    /// Records an event, such as a check's close, which checkout decides.
    fn record(&mut self, event: &OrderEvent) {
        let before = self.order.conflicts().len();
        let meta = self.script.meta_at(location());
        self.order.apply(&meta, event);
        assert_eq!(self.order.conflicts().len(), before, "a valid command caused a conflict");
    }
}

#[test]
fn commands_need_an_active_order_at_the_devices_location() {
    let mut device = Device::new();
    assert_eq!(device.run(OrderCommand::Abandon), Err(CommandError::NotCreated));
    device.run(OrderCommand::Create(created())).unwrap();
    assert_eq!(device.run(OrderCommand::Create(created())), Err(CommandError::AlreadyCreated));
    assert_eq!(
        device.order.decide(id(0x101), id(0xD), OrderCommand::Abandon),
        Err(CommandError::WrongLocation)
    );
    device.run(OrderCommand::Void(reason("walkout"))).unwrap();
    assert_eq!(device.run(OrderCommand::Abandon), Err(CommandError::OrderClosed));
}

#[test]
fn added_lines_must_fit_the_order() {
    let mut device = Device::with_line(false);
    let refused = |line: LineAdded| Device::with_line(false).run(OrderCommand::AddLine(line));
    assert_eq!(refused(added(1, usd(100))), Err(CommandError::LineExists(id(1))));
    assert_eq!(refused(added(2, eur(100))), Err(CommandError::WrongCurrency));
    let mut euro_modifier = added(2, usd(100));
    euro_modifier.modifiers = vec![modifier(0x601, eur(0))];
    assert_eq!(refused(euro_modifier), Err(CommandError::WrongCurrency));
    let mut negative = added(2, usd(-1));
    negative.modifiers.clear();
    assert_eq!(refused(negative), Err(CommandError::Invalid(PayloadError::Invalid("unit price"))));
    let mut zero = added(2, usd(100));
    zero.quantity = each(0);
    assert_eq!(refused(zero), Err(CommandError::Invalid(PayloadError::Invalid("quantity"))));
    device.run(OrderCommand::AddLine(added(2, usd(100)))).unwrap();
}

#[test]
fn line_changes_must_change_something_in_the_lines_terms() {
    let mut device = Device::with_line(false);
    let change = |changed: LineChanged| OrderCommand::ChangeLine(changed);
    let quantity = |n| LineChanged { quantity: Some(each(n)), ..LineChanged::to(id(1)) };
    assert_eq!(device.run(change(quantity(1))), Err(CommandError::NoChange));
    assert_eq!(device.run(change(LineChanged::to(id(1)))), Err(CommandError::NoChange));
    let clear_nothing = LineChanged { seat: Some(Change::Clear), ..LineChanged::to(id(1)) };
    assert_eq!(device.run(change(clear_nothing)), Err(CommandError::NoChange));
    let kilos = Quantity::from_whole(1, Unit::Kilogram).unwrap();
    let kilos = LineChanged { quantity: Some(kilos), ..LineChanged::to(id(1)) };
    assert_eq!(device.run(change(kilos)), Err(CommandError::WrongUnit(id(1))));
    let euros =
        LineChanged { modifiers: Some(vec![modifier(0x601, eur(0))]), ..LineChanged::to(id(1)) };
    assert_eq!(device.run(change(euros)), Err(CommandError::WrongCurrency));
    let unknown = LineChanged { quantity: Some(each(3)), ..LineChanged::to(id(7)) };
    assert_eq!(device.run(change(unknown)), Err(CommandError::UnknownLine(id(7))));
    device.run(change(quantity(2))).unwrap();
    assert_eq!(device.order.line(id(1)).unwrap().quantity(), each(2));
}

#[test]
fn line_commands_follow_the_lines_life() {
    let fire =
        |lines: &[u64]| OrderCommand::FireLines(IdSet::new(lines.iter().map(|&n| id(n))).unwrap());
    let void = |n| OrderCommand::VoidLine { line: id(n), reason: reason("kitchen_error") };
    let comp = |n| OrderCommand::CompLine { line: id(n), reason: reason("birthday") };
    let change =
        OrderCommand::ChangeLine(LineChanged { quantity: Some(each(3)), ..LineChanged::to(id(1)) });

    // Pending lines are changed, removed or fired; not voided.
    let mut device = Device::with_line(false);
    assert_eq!(device.run(void(1)), Err(CommandError::LineNotFired(id(1))));
    device.run(OrderCommand::RemoveLine(id(1))).unwrap();
    assert_eq!(device.run(fire(&[1])), Err(CommandError::LineNotPending(id(1))));

    // Fired lines are voided or comped; not changed, removed or fired again.
    let mut device = Device::with_line(true);
    device.run(OrderCommand::AddLine(added(2, usd(100)))).unwrap();
    assert_eq!(device.run(fire(&[1, 2])), Err(CommandError::LineNotPending(id(1))));
    assert_eq!(
        device.run(OrderCommand::RemoveLine(id(1))),
        Err(CommandError::LineNotPending(id(1)))
    );
    assert_eq!(device.run(change), Err(CommandError::LineNotPending(id(1))));
    device.run(comp(1)).unwrap();
    assert_eq!(device.run(comp(1)), Err(CommandError::AlreadyComped(id(1))));
    device.run(void(1)).unwrap();
    assert_eq!(device.run(comp(1)), Err(CommandError::LineNotLive(id(1))));

    for command in [OrderCommand::RemoveLine(id(8)), void(8), comp(8), fire(&[8])] {
        assert_eq!(device.run(command), Err(CommandError::UnknownLine(id(8))));
    }
}

#[test]
fn attribute_changes_must_change_something() {
    let mut device = Device::with_line(false);
    let change = |changed: AttributesChanged| OrderCommand::ChangeAttributes(changed);
    let none = AttributesChanged::default;
    for unchanged in [
        none(),
        AttributesChanged { mode: Some(Mode::DineIn), ..none() },
        AttributesChanged { table: Some(Change::Set(id(0x200))), ..none() },
        AttributesChanged { customer: Some(Change::Clear), ..none() },
        AttributesChanged {
            table: Some(Change::Set(id(0x201))),
            owner: Some(Change::Set(id(0x300))),
            ..none()
        },
    ] {
        assert_eq!(device.run(change(unchanged)), Err(CommandError::NoChange));
    }
    let moved = AttributesChanged {
        table: Some(Change::Set(id(0x201))),
        guest_count: Some(Change::Clear),
        ..none()
    };
    device.run(change(moved)).unwrap();
    let info = device.order.info().unwrap();
    assert_eq!((info.table, info.guest_count), (Some(id(0x201)), None));
}

#[test]
fn orders_are_abandoned_only_before_anything_is_fired() {
    let mut device = Device::with_line(false);
    assert_eq!(device.run(OrderCommand::Abandon), Err(CommandError::HasLiveLines));
    device.run(OrderCommand::RemoveLine(id(1))).unwrap();
    device.run(OrderCommand::Abandon).unwrap();
    assert_eq!(*device.order.status(), OrderStatus::Abandoned);

    // Once a line was fired, the order was ordered: it is voided, not abandoned.
    let mut fired = Device::with_line(true);
    fired.run(OrderCommand::VoidLine { line: id(1), reason: reason("wrong_item") }).unwrap();
    assert_eq!(fired.run(OrderCommand::Abandon), Err(CommandError::LineWasFired(id(1))));
    fired.run(OrderCommand::Void(reason("walkout"))).unwrap();
    assert_eq!(*fired.order.status(), OrderStatus::Voided(reason("walkout")));
}

#[test]
fn checks_are_opened_once_and_lines_allocated_to_them() {
    let mut device = Device::with_line(false);
    let open = OrderCommand::OpenCheck;
    assert_eq!(device.run(open(id(0xA))), Err(CommandError::CheckExists(id(0xA))));
    device.run(open(id(0xC2))).unwrap();
    assert_eq!(device.run(open(id(0xC2))), Err(CommandError::CheckExists(id(0xC2))));

    let to = |numbers: &[u64], check: u64| {
        OrderCommand::AllocateLines(LinesAllocated::moving(&lines(numbers), id(check)))
    };
    assert_eq!(device.run(to(&[1], 0xA)), Err(CommandError::NoChange));
    assert_eq!(device.run(to(&[1], 0xC3)), Err(CommandError::UnknownCheck(id(0xC3))));
    assert_eq!(device.run(to(&[7], 0xC2)), Err(CommandError::UnknownLine(id(7))));
    device.run(to(&[1], 0xC2)).unwrap();
    assert_eq!(device.order.line(id(1)).unwrap().allocation(), [share(0xC2, 1)]);

    // Split evenly, or two parts to one: each line listed must get a new allocation.
    let both = IdSet::new([id(0xA), id(0xC2)]).unwrap();
    let evenly = OrderCommand::AllocateLines(LinesAllocated::splitting(&lines(&[1]), &both));
    device.run(evenly.clone()).unwrap();
    assert_eq!(device.run(evenly), Err(CommandError::NoChange));
    let thirds = LinesAllocated::new([allocation(1, 0xA, 2), allocation(1, 0xC2, 1)]).unwrap();
    device.run(OrderCommand::AllocateLines(thirds)).unwrap();
    assert_eq!(device.order.line(id(1)).unwrap().allocation(), [share(0xA, 2), share(0xC2, 1)]);

    // Removed and voided lines aren't allocated.
    device.run(OrderCommand::AddLine(added(2, usd(100)))).unwrap();
    device.run(OrderCommand::RemoveLine(id(2))).unwrap();
    assert_eq!(device.run(to(&[2], 0xC2)), Err(CommandError::LineNotLive(id(2))));
}

#[test]
fn closed_checks_freeze_their_lines_for_commands() {
    let mut device = Device::with_line(false);
    device.run(OrderCommand::AddLine(added(2, usd(100)))).unwrap();
    device.run(OrderCommand::OpenCheck(id(0xC2))).unwrap();
    device
        .run(OrderCommand::AllocateLines(LinesAllocated::moving(&lines(&[2]), id(0xC2))))
        .unwrap();
    device.record(&OrderEvent::CheckClosed(closed(0xA, &[(1, 100)])));

    // What line 1 costs, and where it is paid, can't change.
    let quantity = LineChanged { quantity: Some(each(2)), ..LineChanged::to(id(1)) };
    let modifiers = LineChanged { modifiers: Some(Vec::new()), ..LineChanged::to(id(1)) };
    let frozen = Err(CommandError::LineOnClosedCheck(id(1)));
    for command in [
        OrderCommand::ChangeLine(quantity),
        OrderCommand::ChangeLine(modifiers),
        OrderCommand::RemoveLine(id(1)),
        OrderCommand::CompLine { line: id(1), reason: reason("birthday") },
        OrderCommand::AllocateLines(LinesAllocated::moving(&lines(&[1]), id(0xC2))),
    ] {
        assert_eq!(device.run(command), frozen);
    }
    let closed_check = Err(CommandError::CheckClosed(id(0xA)));
    let onto = OrderCommand::AllocateLines(LinesAllocated::moving(&lines(&[2]), id(0xA)));
    assert_eq!(device.run(onto), closed_check);
    assert_eq!(device.run(OrderCommand::Void(reason("walkout"))), closed_check);
    assert_eq!(device.run(OrderCommand::Close), Err(CommandError::CheckOpen(id(0xC2))));

    // Its notes can change, and it is fired, but not voided.
    let notes = LineChanged {
        notes: Some(Change::Set(Note::new("no ice").unwrap())),
        ..LineChanged::to(id(1))
    };
    device.run(OrderCommand::ChangeLine(notes)).unwrap();
    device.run(OrderCommand::FireLines(lines(&[1, 2]))).unwrap();
    assert_eq!(device.run(OrderCommand::VoidLine { line: id(1), reason: reason("x") }), frozen);

    // Once every check holding a live line is closed, the order closes, and takes nothing more.
    device.run(OrderCommand::CompLine { line: id(2), reason: reason("birthday") }).unwrap();
    device.record(&OrderEvent::CheckClosed(closed(0xC2, &[(2, 0)])));
    device.run(OrderCommand::Close).unwrap();
    assert_eq!(*device.order.status(), OrderStatus::Closed);
    for command in [
        OrderCommand::AddLine(added(3, usd(100))),
        OrderCommand::Close,
        OrderCommand::Void(reason("walkout")),
    ] {
        assert_eq!(device.run(command), Err(CommandError::OrderClosed));
    }
    assert_eq!(
        device.order.decide(id(0x101), id(0xD), OrderCommand::Reopen(reason("wrong_tender"))),
        Err(CommandError::WrongLocation)
    );

    // Reopened, everything can change again.
    device.run(OrderCommand::Reopen(reason("wrong_tender"))).unwrap();
    device.run(OrderCommand::VoidLine { line: id(1), reason: reason("kitchen_error") }).unwrap();
    assert_eq!(
        device.run(OrderCommand::Reopen(reason("again"))),
        Err(CommandError::NothingToReopen)
    );
}

#[test]
fn orders_close_with_their_checks_and_reopen_when_something_closed() {
    let mut device = Device::new();
    device.run(OrderCommand::Create(created())).unwrap();
    assert_eq!(device.run(OrderCommand::Close), Err(CommandError::NothingToClose));
    assert_eq!(device.run(OrderCommand::Reopen(reason("x"))), Err(CommandError::NothingToReopen));
    device.run(OrderCommand::AddLine(added(1, usd(100)))).unwrap();
    assert_eq!(device.run(OrderCommand::Close), Err(CommandError::CheckOpen(id(0xA))));

    // A closed check can be reopened while the order is active; then the order can be voided.
    device.record(&OrderEvent::CheckClosed(closed(0xA, &[(1, 100)])));
    assert_eq!(device.run(OrderCommand::Abandon), Err(CommandError::HasLiveLines));
    // Even when a concurrent removal took the check's only line off, what it was paid stands:
    // the order isn't abandoned.
    let mut emptied = Device { order: device.order.clone(), script: Script::new(), last: None };
    let meta = emptied.script.meta_at(location());
    emptied.order.apply(&meta, &OrderEvent::LineRemoved { line: id(1) });
    assert_eq!(emptied.run(OrderCommand::Abandon), Err(CommandError::CheckClosed(id(0xA))));
    device.run(OrderCommand::Reopen(reason("wrong_tender"))).unwrap();
    device.run(OrderCommand::Void(reason("walkout"))).unwrap();
    assert_eq!(device.run(OrderCommand::Reopen(reason("x"))), Err(CommandError::OrderClosed));
}

#[test]
fn allocations_have_one_form() {
    // In order of line, then check, each line's shares in lowest terms.
    let allocated = LinesAllocated::new([
        allocation(2, 0xC2, 3),
        allocation(1, 0xC3, 4),
        allocation(1, 0xC2, 2),
    ])
    .unwrap();
    let kept: Vec<Allocation> = allocated.iter().copied().collect();
    assert_eq!(kept, [allocation(1, 0xC2, 1), allocation(1, 0xC3, 2), allocation(2, 0xC2, 1)]);
    let grouped: Vec<(Id<Line>, Vec<CheckShare>)> = allocated.lines().collect();
    assert_eq!(
        grouped,
        [(id(1), vec![share(0xC2, 1), share(0xC3, 2)]), (id(2), vec![share(0xC2, 1)])]
    );
    assert!(LinesAllocated::new([]).is_err());
    assert!(LinesAllocated::new([allocation(1, 0xC2, 1), allocation(1, 0xC2, 2)]).is_err());

    // A payload's allocations decode only in that form.
    let payload = |entries: &[(u64, u64, u64)]| {
        let items = entries
            .iter()
            .map(|&(line, check, shares)| {
                Value::Map(
                    keel_events::cbor::Map::from_entries([
                        (Value::Unsigned(1), Value::Bytes(id::<()>(line).to_bytes().to_vec())),
                        (Value::Unsigned(2), Value::Bytes(id::<()>(check).to_bytes().to_vec())),
                        (Value::Unsigned(3), Value::Unsigned(shares)),
                    ])
                    .unwrap(),
                )
            })
            .collect();
        let map = keel_events::cbor::Map::from_entries([(Value::Unsigned(1), Value::Array(items))]);
        Value::Map(map.unwrap())
    };
    let schema = OrderEvent::CheckOpened { check: id(1) }.schema();
    let schema = SchemaRef {
        name: SchemaName::new("order.lines_allocated").unwrap(),
        version: schema.to_ref().unwrap().version,
    };
    let decode = |entries: &[(u64, u64, u64)]| OrderEvent::from_value(&schema, &payload(entries));
    assert!(decode(&[(1, 0xC2, 1), (1, 0xC3, 2)]).is_ok());
    let invalid = Err(DecodeError::Malformed(PayloadError::Invalid("allocations")));
    assert_eq!(decode(&[(1, 0xC3, 2), (1, 0xC2, 1)]), invalid);
    assert_eq!(decode(&[(1, 0xC2, 2), (1, 0xC3, 4)]), invalid);
    assert_eq!(decode(&[(1, 0xC2, 2)]), invalid);
    assert_eq!(decode(&[(1, 0xC2, 1), (1, 0xC2, 1)]), invalid);
    assert_eq!(decode(&[(1, 0xC2, 0)]), invalid);
    assert_eq!(decode(&[]), invalid);
}

#[test]
fn too_deep_modifiers_are_refused() {
    let mut deep = modifier(0x600, usd(0));
    for _ in 0..20 {
        let mut outer = modifier(0x600, usd(0));
        outer.modifiers.push(deep);
        deep = outer;
    }
    let order = Script::created().order;
    let mut line = added(1, usd(100));
    line.modifiers = vec![deep];
    assert_eq!(
        order.decide(location(), id(0xD), OrderCommand::AddLine(line)),
        Err(CommandError::Schema(crate::schema::SchemaError::TooDeep))
    );
}

#[test]
fn the_registry_lists_every_schema_once() {
    let mut names: Vec<(&str, u32)> =
        OrderEvent::SCHEMAS.iter().map(|schema| (schema.name, schema.version)).collect();
    for schema in OrderEvent::SCHEMAS {
        let reference = schema.to_ref().unwrap();
        assert!(reference.name.as_str().starts_with("order."));
        assert!(schema.matches(&reference));
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), OrderEvent::SCHEMAS.len());
    let unknown = SchemaRef {
        name: SchemaName::new("order.tip_added").unwrap(),
        version: 1.try_into().unwrap(),
    };
    assert_eq!(OrderEvent::from_value(&unknown, &Value::Null), Err(DecodeError::UnknownSchema));
    let newer = SchemaRef {
        name: SchemaName::new("order.line_removed").unwrap(),
        version: 2.try_into().unwrap(),
    };
    assert_eq!(OrderEvent::from_value(&newer, &Value::Null), Err(DecodeError::UnknownSchema));
}

/// One example of each schema, for the pinned known-answer test.
fn golden_events() -> Vec<OrderEvent> {
    let mut line = added(1, usd(450));
    line.seat = NonZeroU16::new(1);
    line.course = NonZeroU8::new(2);
    line.notes = Some(Note::new("no salt").unwrap());
    vec![
        OrderEvent::Created(created()),
        OrderEvent::AttributesChanged(AttributesChanged {
            mode: Some(Mode::Takeout),
            table: Some(Change::Clear),
            guest_count: Some(Change::Set(NonZeroU16::new(4).unwrap())),
            ..AttributesChanged::default()
        }),
        OrderEvent::LineAdded(line),
        OrderEvent::LineChanged(LineChanged {
            quantity: Some(each(2)),
            notes: Some(Change::Clear),
            ..LineChanged::to(id(1))
        }),
        OrderEvent::LineRemoved { line: id(1) },
        OrderEvent::LinesFired { lines: IdSet::new([id(2), id(1)]).unwrap() },
        OrderEvent::LineVoided {
            line: id(1),
            reason: Reason {
                code: ReasonCode::new("kitchen_error").unwrap(),
                note: Some(Note::new("burnt").unwrap()),
            },
        },
        OrderEvent::LineComped { line: id(2), reason: reason("birthday") },
        OrderEvent::Voided { reason: reason("walkout") },
        OrderEvent::Abandoned,
        OrderEvent::CheckOpened { check: id(0xC1) },
        OrderEvent::LinesAllocated(
            LinesAllocated::new([
                Allocation { line: id(2), check: id(0xC1), shares: NonZeroU16::MIN },
                Allocation { line: id(1), check: id(0xC1), shares: NonZeroU16::MIN },
                Allocation { line: id(1), check: id(0xA), shares: NonZeroU16::new(2).unwrap() },
            ])
            .unwrap(),
        ),
        // A third of the wine, and a comped dessert, with 8.875% tax on the wine.
        OrderEvent::CheckClosed(CheckClosed {
            check: id(0xC1),
            rules_version: RulesVersion::from_bytes([9; 32]),
            lines: vec![
                LineCharge { line: id(1), gross: usd(1000), net: usd(1000), tax: usd(89) },
                LineCharge { line: id(2), gross: usd(650), net: usd(0), tax: usd(0) },
            ],
            taxes: vec![TaxCharge { tax: id(0x900), taxable: usd(1000), amount: usd(89) }],
            total: usd(1089),
            payments: Some(IdSet::new([id(0xB1)]).unwrap()),
        }),
        OrderEvent::Closed,
        OrderEvent::Reopened {
            reason: Reason {
                code: ReasonCode::new("wrong_tender").unwrap(),
                note: Some(Note::new("paid by card").unwrap()),
            },
        },
        OrderEvent::OwnershipRequested { lease: Lease::new(2).unwrap() },
        OrderEvent::OwnershipGranted(OwnershipGranted {
            request: id(0xE1),
            device: id(0xD2),
            lease: Lease::new(3).unwrap(),
            epoch: Epoch::new(1).unwrap(),
        }),
        OrderEvent::OwnershipRefused { request: id(0xE1), refusal: Refusal::PaymentInProgress },
        OrderEvent::OwnershipOverridden {
            lease: Lease::new(1).unwrap(),
            reason: Reason {
                code: ReasonCode::new("hub_unreachable").unwrap(),
                note: Some(Note::new("bar is offline").unwrap()),
            },
        },
        OrderEvent::DiscountApplied(DiscountApplied {
            discount: id(0xD1),
            line: Some(id(1)),
            value: DiscountValue::Amount(usd(250)),
            reason: Reason {
                code: ReasonCode::new("loyalty").unwrap(),
                note: Some(Note::new("regular").unwrap()),
            },
        }),
        OrderEvent::DiscountRemoved { discount: id(0xD1) },
        OrderEvent::LineRepriced(LineRepriced {
            line: id(1),
            unit_price: usd(395),
            reason: Reason {
                code: ReasonCode::new("price_match").unwrap(),
                note: Some(Note::new("cafe across the street").unwrap()),
            },
        }),
        golden_return(),
    ]
}

/// The return of the pinned examples: two of a count and half a kilogram (ADR-0032).
fn golden_return() -> OrderEvent {
    OrderEvent::LinesReturned(LinesReturned {
        refund: id(0xC1),
        lines: vec![
            LineReturned { line: id(0x301), quantity: each(2), net: usd(650), tax: usd(58) },
            LineReturned {
                line: id(0x302),
                quantity: Quantity::from_micros(500_000, Unit::Kilogram),
                net: usd(125),
                tax: usd(0),
            },
        ],
        reason: Reason {
            code: ReasonCode::new("damaged").unwrap(),
            note: Some(Note::new("cracked lid").unwrap()),
        },
    })
}

/// Payload rules that span fields, which a device's own commands already respect, so only a
/// faulty or hostile kernel would write such payloads.
#[test]
fn payload_rules_across_fields_are_enforced() {
    let decode = |event: &OrderEvent| {
        let schema = event.schema().to_ref().unwrap();
        OrderEvent::from_value(&schema, &event.to_value())
    };
    let invalid = |field| Err(DecodeError::Malformed(PayloadError::Invalid(field)));

    let mut euro_modifier = added(1, usd(100));
    euro_modifier.modifiers = vec![modifier(0x600, eur(0))];
    assert_eq!(decode(&OrderEvent::LineAdded(euro_modifier)), invalid("modifiers"));
    let mut nested = modifier(0x600, usd(0));
    nested.modifiers.push(modifier(0x601, eur(0)));
    let mut euro_nested = added(1, usd(100));
    euro_nested.modifiers = vec![nested];
    assert_eq!(decode(&OrderEvent::LineAdded(euro_nested)), invalid("modifiers"));
    let mut negative = added(1, usd(100));
    negative.modifiers = vec![modifier(0x600, usd(-1))];
    assert_eq!(decode(&OrderEvent::LineAdded(negative)), invalid("modifiers"));
    let mixed = LineChanged {
        modifiers: Some(vec![modifier(0x600, usd(0)), modifier(0x601, eur(0))]),
        ..LineChanged::to(id(1))
    };
    assert_eq!(decode(&OrderEvent::LineChanged(mixed)), invalid("modifiers"));
    let empty = Err(DecodeError::Malformed(PayloadError::EmptyChange));
    assert_eq!(decode(&OrderEvent::AttributesChanged(AttributesChanged::default())), empty);
    assert_eq!(decode(&OrderEvent::LineChanged(LineChanged::to(id(1)))), empty);

    // A closed check's snapshot adds up, in one currency, in one order.
    let taxed = || {
        let mut snapshot = closed(0xA, &[(1, 450), (2, 1000)]);
        snapshot.lines[1].tax = usd(89);
        snapshot.taxes = vec![TaxCharge { tax: id(0x900), taxable: usd(1000), amount: usd(89) }];
        snapshot.total = usd(1539);
        snapshot
    };
    let check = |change: fn(&mut CheckClosed)| {
        let mut snapshot = taxed();
        change(&mut snapshot);
        decode(&OrderEvent::CheckClosed(snapshot))
    };
    assert!(check(|_| {}).is_ok());
    assert_eq!(check(|s| s.lines.swap(0, 1)), invalid("lines"));
    assert_eq!(check(|s| s.lines.clear()), invalid("lines"));
    assert_eq!(check(|s| s.lines[0].net = usd(451)), invalid("lines"));
    assert_eq!(check(|s| s.lines[0].net = usd(-1)), invalid("lines"));
    assert_eq!(check(|s| s.lines[0].gross = eur(450)), invalid("lines"));
    assert_eq!(check(|s| s.taxes[0].taxable = usd(0)), invalid("taxes"));
    assert_eq!(check(|s| s.taxes[0].amount = usd(88)), invalid("taxes"));
    assert_eq!(check(|s| s.taxes.push(s.taxes[0])), invalid("taxes"));
    assert_eq!(check(|s| s.lines[1].tax = usd(-89)), invalid("lines"));
    assert_eq!(check(|s| s.total = usd(1540)), invalid("total"));
    assert_eq!(
        check(|s| s.payments = None),
        Err(DecodeError::Malformed(PayloadError::Missing("payments")))
    );
    // Nothing to pay needs no payment, and may still have one.
    let mut free = closed(0xA, &[(1, 0)]);
    assert!(decode(&OrderEvent::CheckClosed(free.clone())).is_ok());
    free.payments = Some(IdSet::new([id(0xB1)]).unwrap());
    assert!(decode(&OrderEvent::CheckClosed(free)).is_ok());
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The order payloads, pinned forever: one example of each schema. Python's `cbor2` decoded
/// each one, confirmed it is canonical, and matched it field by field against the documented
/// key tables; it encoded the check payloads, the closing and reopening ones, the ownership ones
/// (ADR-0021), the discount and pricing ones (ADR-0030) and the return (ADR-0032), itself, from
/// the key tables. If this test fails, a payload format changed,
/// and stored events would no longer decode.
#[test]
fn the_payload_formats_are_pinned() {
    let pinned = [
        (
            "order.created",
            "a601000200036355534405500192f0c1000070008000000000000200060208500192f0c1000070008000000000000300",
        ),
        ("order.attributes_changed", "a3010103f60404"),
        (
            "order.line_added",
            "ab01500192f0c100007000800000000000000102500192f0c10000700080000000000004000358200707070707070707070707070707070707070707070707070707070707070707046a466c617420776869746505500192f0c100007000800000000000050006821901c26355534407821a000f424064656163680881a701500192f0c100007000800000000000060002684f6174206d696c6b03000401050006820063555344078009010a020b676e6f2073616c74",
        ),
        (
            "order.line_changed",
            "a301500192f0c100007000800000000000000102821a001e8480646561636806f6",
        ),
        ("order.line_removed", "a101500192f0c1000070008000000000000001"),
        (
            "order.lines_fired",
            "a10182500192f0c1000070008000000000000001500192f0c1000070008000000000000002",
        ),
        (
            "order.line_voided",
            "a301500192f0c1000070008000000000000001026d6b69746368656e5f6572726f7203656275726e74",
        ),
        ("order.line_comped", "a201500192f0c100007000800000000000000202686269727468646179"),
        ("order.voided", "a1016777616c6b6f7574"),
        ("order.abandoned", "a0"),
        ("order.check_opened", "a101500192f0c10000700080000000000000c1"),
        (
            "order.lines_allocated",
            "a10183a301500192f0c100007000800000000000000102500192f0c100007000800000000000000a0302a301500192f0c100007000800000000000000102500192f0c10000700080000000000000c10301a301500192f0c100007000800000000000000202500192f0c10000700080000000000000c10301",
        ),
        (
            "order.check_closed",
            "a601500192f0c10000700080000000000000c102582009090909090909090909090909090909090909090909090909090909090909090382a401500192f0c100007000800000000000000102821903e86355534403821903e8635553440482185963555344a401500192f0c1000070008000000000000002028219028a6355534403820063555344048200635553440481a301500192f0c100007000800000000000090002821903e86355534403821859635553440582190441635553440681500192f0c10000700080000000000000b1",
        ),
        ("order.closed", "a0"),
        ("order.reopened", "a2016c77726f6e675f74656e646572026c706169642062792063617264"),
        ("order.ownership_requested", "a10102"),
        (
            "order.ownership_granted",
            "a401500192f0c10000700080000000000000e102500192f0c10000700080000000000000d203030401",
        ),
        ("order.ownership_refused", "a201500192f0c10000700080000000000000e10202"),
        (
            "order.ownership_overridden",
            "a30101026f6875625f756e726561636861626c65036e626172206973206f66666c696e65",
        ),
        (
            "order.discount_applied",
            "a501500192f0c10000700080000000000000d102500192f0c1000070008000000000000001048218fa6355534405676c6f79616c74790667726567756c6172",
        ),
        ("order.discount_removed", "a101500192f0c10000700080000000000000d1"),
        (
            "order.line_repriced",
            "a401500192f0c1000070008000000000000001028219018b63555344036b70726963655f6d61746368047663616665206163726f73732074686520737472656574",
        ),
        (
            "order.lines_returned",
            "a401500192f0c10000700080000000000000c10282a401500192f0c100007000800000000000030102821a001e84806465616368038219028a635553440482183a63555344a401500192f0c100007000800000000000030202821a0007a120626b670382187d6355534404820063555344036764616d61676564046b637261636b6564206c6964",
        ),
    ];
    let events = golden_events();
    assert_eq!(events.len(), pinned.len());
    assert_eq!(events.len(), OrderEvent::SCHEMAS.len());
    for (event, (name, hex)) in events.into_iter().zip(pinned) {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), name);
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice(), "{name}");
        assert_eq!(OrderEvent::decode(&schema, &payload), Ok(event), "{name}");
    }
}

fn percent_off(discount: u64, line: Option<u64>, bp: u32) -> DiscountApplied {
    DiscountApplied {
        discount: id(discount),
        line: line.map(id),
        value: DiscountValue::Percent(bp),
        reason: reason("loyalty"),
    }
}

fn amount_off(discount: u64, line: u64, amount: Money) -> DiscountApplied {
    DiscountApplied {
        discount: id(discount),
        line: Some(id(line)),
        value: DiscountValue::Amount(amount),
        reason: reason("damaged"),
    }
}

/// A discount at rest, as Python's `cbor2` encoded it: 10% off the whole order, with no note.
#[test]
fn a_percentage_off_the_whole_order_is_pinned() {
    let applied = OrderEvent::DiscountApplied(DiscountApplied {
        reason: reason("staff_meal"),
        ..percent_off(0xD2, None, 1_000)
    });
    let (schema, payload) = applied.encode().unwrap();
    let pinned = "a301500192f0c10000700080000000000000d2031903e8056a73746166665f6d65616c";
    assert_eq!(payload.as_bytes(), bytes(pinned).as_slice());
    assert_eq!(OrderEvent::decode(&schema, &payload), Ok(applied));
}

/// A discount takes a percentage or an amount, never both or neither; a percentage from 0.01%
/// to 100%, an amount more than zero and off a line.
#[test]
fn discount_payloads_keep_their_rules() {
    let schema = OrderEvent::DiscountApplied(percent_off(1, None, 1)).schema().to_ref().unwrap();
    let with = |applied: DiscountApplied, extra: &[(u64, Value)]| {
        let event = OrderEvent::DiscountApplied(applied);
        let mut entries = event.to_value().as_map().unwrap().clone().into_entries();
        for (key, value) in extra {
            entries.retain(|(existing, _)| existing.as_u64() != Some(*key));
            entries.push((Value::Unsigned(*key), value.clone()));
        }
        OrderEvent::from_value(
            &schema,
            &Value::Map(keel_events::cbor::Map::from_entries(entries).unwrap()),
        )
    };
    let invalid = |field| Err(DecodeError::Malformed(PayloadError::Invalid(field)));
    assert!(with(percent_off(1, None, 1), &[]).is_ok());
    assert!(with(percent_off(1, None, 10_000), &[]).is_ok());
    assert_eq!(with(percent_off(1, None, 0), &[]), invalid("percent"));
    assert_eq!(with(percent_off(1, None, 10_001), &[]), invalid("percent"));
    assert_eq!(with(percent_off(1, None, 1), &[(3, Value::Unsigned(1 << 32))]), invalid("percent"));
    assert_eq!(with(amount_off(1, 2, usd(0)), &[]), invalid("amount"));
    assert_eq!(with(amount_off(1, 2, usd(-5)), &[]), invalid("amount"));
    let both = [(3, Value::Unsigned(500))];
    assert_eq!(with(amount_off(1, 2, usd(5)), &both), invalid("discount"));
    let mut neither = OrderEvent::DiscountApplied(amount_off(1, 2, usd(5)))
        .to_value()
        .as_map()
        .unwrap()
        .clone()
        .into_entries();
    neither.retain(|(key, _)| key.as_u64() != Some(4));
    assert_eq!(
        OrderEvent::from_value(
            &schema,
            &Value::Map(keel_events::cbor::Map::from_entries(neither).unwrap())
        ),
        Err(DecodeError::Malformed(PayloadError::Missing("percent or amount")))
    );
    let mut off_order = amount_off(1, 2, usd(5));
    off_order.line = None;
    assert_eq!(with(off_order, &[]), invalid("amount"));
}

#[test]
fn discounts_are_priced_with_the_order() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(1_000))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(500))));
    script.apply(&OrderEvent::DiscountApplied(amount_off(0xD1, 1, usd(250))));
    script.apply(&OrderEvent::DiscountApplied(percent_off(0xD2, Some(1), 1_000)));
    script.apply(&OrderEvent::DiscountApplied(percent_off(0xD3, None, 2_000)));
    assert_eq!(script.kinds(), []);
    let rules = keel_pricing::Rules::untaxed();
    let totals = keel_pricing::price(&script.order.basket().unwrap(), &rules).unwrap();
    // Line 1: 10.00, less 2.50, less 10% of 7.50: 6.75. With line 2's 5.00, 11.75, less 20%:
    // 9.40.
    assert_eq!(totals.lines[0].discounts, [usd(250), usd(75)]);
    assert_eq!(totals.discounts, [usd(235)]);
    assert_eq!(totals.total, usd(940));

    // Taken off, a discount no longer prices; its identifier stays used.
    script.apply(&OrderEvent::DiscountRemoved { discount: id(0xD3) });
    assert!(script.order.discounts().is_empty() && script.order.discount_used(id(0xD3)));
    let totals = keel_pricing::price(&script.order.basket().unwrap(), &rules).unwrap();
    assert_eq!(totals.total, usd(1_175));

    // Split among checks, an amount off a shared line is split as the line is: 1.01 off line 2,
    // shared evenly, is 0.51 off the first check's half and 0.50 off the second's.
    script.apply(&OrderEvent::DiscountApplied(amount_off(0xD4, 2, usd(101))));
    script.apply(&OrderEvent::DiscountApplied(percent_off(0xD5, None, 1_000)));
    script.apply(&OrderEvent::CheckOpened { check: id(0xC2) });
    let split = LinesAllocated::new([allocation(2, 0xA, 1), allocation(2, 0xC2, 1)]).unwrap();
    script.apply(&OrderEvent::LinesAllocated(split));
    assert_eq!(script.kinds(), []);
    let first = script.order.check_basket(id(0xA)).unwrap();
    let second = script.order.check_basket(id(0xC2)).unwrap();
    assert_eq!(first.lines[1].discounts, [keel_pricing::Discount::Amount(usd(51))]);
    assert_eq!(second.lines[0].discounts, [keel_pricing::Discount::Amount(usd(50))]);
    // The whole order's percentage applies to each check.
    let tenth = keel_pricing::Discount::Percent(keel_types::Rate::from_basis_points(1_000));
    assert_eq!(first.discounts, [tenth]);
    assert_eq!(second.discounts, [tenth]);
}

#[test]
fn discount_commands_follow_the_rules() {
    let apply = OrderCommand::ApplyDiscount;
    let mut device = Device::with_line(false);
    device.run(OrderCommand::AddLine(added(2, usd(100)))).unwrap();
    let invalid = |field| Err(CommandError::Invalid(PayloadError::Invalid(field)));
    assert_eq!(device.run(apply(percent_off(1, Some(1), 0))), invalid("percent"));
    let mut off_order = amount_off(1, 1, usd(5));
    off_order.line = None;
    assert_eq!(device.run(apply(off_order)), invalid("amount"));
    assert_eq!(device.run(apply(amount_off(1, 1, eur(5)))), Err(CommandError::WrongCurrency));
    assert_eq!(device.run(apply(amount_off(1, 7, usd(5)))), Err(CommandError::UnknownLine(id(7))));
    device.run(apply(amount_off(1, 1, usd(5)))).unwrap();
    device.run(apply(percent_off(2, None, 500))).unwrap();
    device.run(apply(percent_off(3, Some(2), 500))).unwrap();
    assert_eq!(
        device.run(apply(percent_off(1, None, 5))),
        Err(CommandError::DiscountExists(id(1)))
    );
    assert_eq!(device.order.line(id(1)).unwrap().discounts(), [amount_off(1, 1, usd(5))]);

    // Taken off once; its identifier is never used again.
    device.run(OrderCommand::RemoveDiscount(id(1))).unwrap();
    assert_eq!(
        device.run(OrderCommand::RemoveDiscount(id(1))),
        Err(CommandError::UnknownDiscount(id(1)))
    );
    assert_eq!(
        device.run(apply(percent_off(1, Some(1), 5))),
        Err(CommandError::DiscountExists(id(1)))
    );
    assert_eq!(
        device.run(OrderCommand::RemoveDiscount(id(9))),
        Err(CommandError::UnknownDiscount(id(9)))
    );

    // A line taken off takes no discount, and its discounts can still come off.
    device.run(OrderCommand::RemoveLine(id(2))).unwrap();
    assert_eq!(
        device.run(apply(percent_off(4, Some(2), 5))),
        Err(CommandError::LineNotLive(id(2)))
    );
    device.run(OrderCommand::RemoveDiscount(id(3))).unwrap();

    // A closed check freezes its lines' discounts, and the whole order's.
    device.run(apply(percent_off(5, Some(1), 5))).unwrap();
    device.record(&OrderEvent::CheckClosed(closed(0xA, &[(1, 100)])));
    assert_eq!(
        device.run(apply(percent_off(6, Some(1), 5))),
        Err(CommandError::LineOnClosedCheck(id(1)))
    );
    assert_eq!(
        device.run(OrderCommand::RemoveDiscount(id(5))),
        Err(CommandError::LineOnClosedCheck(id(1)))
    );
    assert_eq!(device.run(apply(percent_off(6, None, 5))), Err(CommandError::CheckClosed(id(0xA))));
    assert_eq!(
        device.run(OrderCommand::RemoveDiscount(id(2))),
        Err(CommandError::CheckClosed(id(0xA)))
    );
    device.run(OrderCommand::Close).unwrap();
    assert_eq!(device.run(apply(percent_off(6, None, 5))), Err(CommandError::OrderClosed));
}

#[test]
fn concurrent_discounts_resolve_by_the_rules() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(450))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(300))));
    // One identifier names one discount, even once it is taken off.
    let first = script.apply(&OrderEvent::DiscountApplied(percent_off(1, Some(1), 500)));
    let again = script.apply(&OrderEvent::DiscountApplied(percent_off(1, None, 500)));
    script.apply(&OrderEvent::DiscountRemoved { discount: id(1) });
    let reused = script.apply(&OrderEvent::DiscountApplied(percent_off(1, Some(2), 500)));
    // Two devices taking off the same discount agree; one never applied is reported.
    script.apply(&OrderEvent::DiscountRemoved { discount: id(1) });
    let never = script.apply(&OrderEvent::DiscountRemoved { discount: id(9) });
    let unknown = script.apply(&OrderEvent::DiscountApplied(percent_off(2, Some(7), 500)));
    let foreign = script.apply(&OrderEvent::DiscountApplied(amount_off(3, 1, eur(5))));
    script.apply(&OrderEvent::LineRemoved { line: id(2) });
    let removed = script.apply(&OrderEvent::DiscountApplied(percent_off(4, Some(2), 500)));
    assert!(first != again);
    assert!(!script.order.discount_used(id(2)) && !script.order.discount_used(id(3)));
    assert!(!script.order.discount_used(id(4)));
    assert!(script.order.line(id(1)).unwrap().discounts().is_empty());
    assert!(script.order.discounts().is_empty());
    let conflicts: Vec<(Id<Event>, ConflictKind)> =
        script.order.conflicts().iter().map(|c| (c.event, c.kind)).collect();
    assert_eq!(
        conflicts,
        [
            (again, ConflictKind::DuplicateDiscount(id(1))),
            (reused, ConflictKind::DuplicateDiscount(id(1))),
            (never, ConflictKind::UnknownDiscount(id(9))),
            (unknown, ConflictKind::UnknownLine(id(7))),
            (foreign, ConflictKind::CurrencyMismatch(id(1))),
            (removed, ConflictKind::DiscountedAfterRemoval(id(2))),
        ]
    );

    // Once a check is closed, discounts on its lines, and on the whole order, apply and are
    // reported: the snapshot stands.
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 450)])));
    let on_line = script.apply(&OrderEvent::DiscountApplied(percent_off(5, Some(1), 500)));
    let on_order = script.apply(&OrderEvent::DiscountApplied(percent_off(6, None, 500)));
    let off_line = script.apply(&OrderEvent::DiscountRemoved { discount: id(5) });
    let off_order = script.apply(&OrderEvent::DiscountRemoved { discount: id(6) });
    script.apply(&OrderEvent::Closed);
    let closed_order = script.apply(&OrderEvent::DiscountApplied(percent_off(7, None, 500)));
    assert_eq!(script.order.discounts(), [percent_off(7, None, 500)]);
    let late: Vec<(Id<Event>, ConflictKind)> =
        script.order.conflicts().iter().skip(6).map(|c| (c.event, c.kind)).collect();
    assert_eq!(
        late,
        [
            (on_line, ConflictKind::ChangedOnClosedCheck(id(1))),
            (on_order, ConflictKind::DiscountOnClosed(id(6))),
            (off_line, ConflictKind::ChangedOnClosedCheck(id(1))),
            (off_order, ConflictKind::DiscountOnClosed(id(6))),
            (closed_order, ConflictKind::DiscountOnClosed(id(7))),
        ]
    );
}

#[test]
fn a_discounts_effect_is_what_it_would_take() {
    let rules = keel_pricing::Rules::untaxed();
    let mut script = Script::new();
    assert_eq!(script.order.discount_effect(&percent_off(1, None, 1_000), &rules), Ok(None));
    script.apply(&OrderEvent::Created(created()));
    script.apply(&OrderEvent::LineAdded(added(1, usd(1_000))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(300))));
    script.apply(&OrderEvent::DiscountApplied(percent_off(0xD1, Some(1), 1_000)));
    script.apply(&OrderEvent::DiscountApplied(percent_off(0xD2, None, 5_000)));
    let effect = |applied: DiscountApplied| script.order.discount_effect(&applied, &rules).unwrap();
    let both = |taken, total, percent| {
        Some(DiscountEffect { taken: usd(taken), total: usd(total), percent })
    };
    // Line 1's 10% takes $1.00 of $10.00; the order's 50% takes $6.00 of the $12.00 left.
    // Another 20% on line 1 takes $1.80 of its $9.00: together with its 10%, 28% of the line,
    // compounded (0.9 × 0.8 = 0.72 left). The order's 50% then takes $5.10: $7.90 in all.
    assert_eq!(effect(percent_off(3, Some(1), 2_000)), both(180, 790, 2_800));
    // Another 10% on the order takes $0.60 of the $6.00 its 50% leaves: 55% together.
    assert_eq!(effect(percent_off(3, None, 1_000)), both(60, 760, 5_500));
    // $1.00 off line 1 takes $1.00 of the $9.00 its 10% leaves: 20% of the line together
    // (0.9 × 8/9 = 0.8 left), exactly.
    assert_eq!(effect(amount_off(3, 1, usd(100))), both(100, 750, 2_000));
    // More than is left takes what is left: all of the line.
    assert_eq!(effect(amount_off(3, 1, usd(5_000))), both(900, 1_150, 10_000));
    assert_eq!(effect(amount_off(3, 1, usd(900))), both(900, 1_150, 10_000));
    // $1.00 off line 2's $3.00 is a third of it, 33.34% rounded up; the order's 50% then takes
    // $5.50 of the $11.00 left: $7.50 in all. Found by a planted bug: no share here had to round.
    assert_eq!(effect(amount_off(3, 2, usd(100))), both(100, 750, 3_334));
    // On a comped line nothing is left, and nothing is taken.
    script.apply(&OrderEvent::LineComped { line: id(2), reason: reason("birthday") });
    let comped = script.order.discount_effect(&amount_off(3, 2, usd(100)), &rules).unwrap();
    assert_eq!(comped, both(0, 550, 0));
    // Raised to three, line 1's 10% takes $3.00 of $30.00, and the order's 50% $13.50 of the
    // $27.00 left: $16.50 in all, the line's share still 10%.
    let three = Quantity::from_whole(3, Unit::Each).unwrap();
    let raised = script.order.quantity_effect(id(1), three, &rules).unwrap();
    assert_eq!(raised, both(0, 1_650, 1_000));
    // A line that isn't live has no effect to price.
    script.apply(&OrderEvent::LineRemoved { line: id(2) });
    assert_eq!(script.order.discount_effect(&amount_off(3, 2, usd(100)), &rules), Ok(None));
    assert_eq!(script.order.discount_effect(&amount_off(3, 7, usd(100)), &rules), Ok(None));
}

fn repriced(line: u64, price: Money) -> LineRepriced {
    LineRepriced { line: id(line), unit_price: price, reason: reason("price_match") }
}

#[test]
fn a_line_priced_again_prices_at_its_new_price() {
    let mut script = Script::created();
    let mut latte = added(1, usd(450));
    latte.quantity = each(2);
    script.apply(&OrderEvent::LineAdded(latte));
    script.apply(&OrderEvent::LineRepriced(repriced(1, usd(395))));
    assert_eq!(script.kinds(), []);
    let line = script.order.line(id(1)).unwrap();
    assert_eq!(line.unit_price(), usd(395));
    assert_eq!(line.item().unit_price, usd(450), "the price it was rung at stays");
    assert_eq!(line.repriced(), Some(&repriced(1, usd(395))));
    // Two at $3.95.
    let totals =
        keel_pricing::price(&script.order.basket().unwrap(), &keel_pricing::Rules::untaxed())
            .unwrap();
    assert_eq!(totals.lines[0].gross, usd(790));
    // The last price set is the line's; free is a price.
    script.apply(&OrderEvent::LineRepriced(repriced(1, usd(0))));
    assert_eq!(script.order.line(id(1)).unwrap().unit_price(), usd(0));

    // A price in another currency, or for a line removed or unknown, doesn't apply; one on a
    // closed check applies and is reported.
    let foreign = script.apply(&OrderEvent::LineRepriced(repriced(1, eur(100))));
    let unknown = script.apply(&OrderEvent::LineRepriced(repriced(7, usd(100))));
    script.apply(&OrderEvent::LineAdded(added(2, usd(300))));
    script.apply(&OrderEvent::LineRemoved { line: id(2) });
    let removed = script.apply(&OrderEvent::LineRepriced(repriced(2, usd(100))));
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 100)])));
    let frozen = script.apply(&OrderEvent::LineRepriced(repriced(1, usd(100))));
    assert_eq!(script.order.line(id(1)).unwrap().unit_price(), usd(100));
    assert_eq!(script.order.line(id(2)).unwrap().unit_price(), usd(300));
    let conflicts: Vec<(Id<Event>, ConflictKind)> =
        script.order.conflicts().iter().map(|c| (c.event, c.kind)).collect();
    assert_eq!(
        conflicts,
        [
            (foreign, ConflictKind::CurrencyMismatch(id(1))),
            (unknown, ConflictKind::UnknownLine(id(7))),
            (removed, ConflictKind::RepricedAfterRemoval(id(2))),
            (frozen, ConflictKind::ChangedOnClosedCheck(id(1))),
        ]
    );
}

#[test]
fn a_line_is_priced_again_only_as_its_rules_allow() {
    let reprice = |line, price| OrderCommand::RepriceLine(repriced(line, price));
    let mut device = Device::with_line(true);
    assert_eq!(
        device.run(reprice(1, usd(-1))),
        Err(CommandError::Invalid(PayloadError::Invalid("unit price")))
    );
    assert_eq!(device.run(reprice(7, usd(50))), Err(CommandError::UnknownLine(id(7))));
    assert_eq!(device.run(reprice(1, eur(50))), Err(CommandError::WrongCurrency));
    assert_eq!(device.run(reprice(1, usd(100))), Err(CommandError::NoChange));
    // A fired line is priced again; then its new price is the one that changes nothing.
    device.run(reprice(1, usd(50))).unwrap();
    assert_eq!(device.run(reprice(1, usd(50))), Err(CommandError::NoChange));
    device.run(reprice(1, usd(100))).unwrap();
    device.run(OrderCommand::VoidLine { line: id(1), reason: reason("kitchen_error") }).unwrap();
    assert_eq!(device.run(reprice(1, usd(0))), Err(CommandError::LineNotLive(id(1))));
    let mut device = Device::with_line(false);
    device.record(&OrderEvent::CheckClosed(closed(0xA, &[(1, 100)])));
    assert_eq!(device.run(reprice(1, usd(0))), Err(CommandError::LineOnClosedCheck(id(1))));
}

/// A device holding a closed order (ADR-0032): line 1, three at $4.00, discounted to $10.00 with
/// $0.89 of tax; line 2, half a kilogram, shared by the main check and check 0xC1, $5.00 on each
/// with no tax; line 3, comped. Every check closed, and the order.
fn closed_order() -> Device {
    let mut device = Device::new();
    device.run(OrderCommand::Create(created())).unwrap();
    let mut three = added(1, usd(400));
    three.quantity = each(3);
    device.run(OrderCommand::AddLine(three)).unwrap();
    let mut half = added(2, usd(2_000));
    half.quantity = Quantity::from_micros(500_000, Unit::Kilogram);
    device.run(OrderCommand::AddLine(half)).unwrap();
    device.run(OrderCommand::AddLine(added(3, usd(300)))).unwrap();
    device.run(OrderCommand::CompLine { line: id(3), reason: reason("birthday") }).unwrap();
    device.run(OrderCommand::OpenCheck(id(0xC1))).unwrap();
    let split = LinesAllocated::new([allocation(2, 0xA, 1), allocation(2, 0xC1, 1)]).unwrap();
    device.run(OrderCommand::AllocateLines(split)).unwrap();
    let charge = |line, gross, net, tax| LineCharge {
        line: id(line),
        gross: usd(gross),
        net: usd(net),
        tax: usd(tax),
    };
    let taxed = TaxCharge { tax: id(0x900), taxable: usd(1_000), amount: usd(89) };
    device.record(&OrderEvent::CheckClosed(CheckClosed {
        lines: vec![charge(1, 1_200, 1_000, 89), charge(2, 500, 500, 0), charge(3, 300, 0, 0)],
        taxes: vec![taxed],
        total: usd(1_589),
        ..closed(0xA, &[])
    }));
    device.record(&OrderEvent::CheckClosed(closed(0xC1, &[(2, 500)])));
    device.record(&OrderEvent::Closed);
    device
}

fn returning(refund: u64, lines: &[(u64, Quantity)]) -> OrderCommand {
    OrderCommand::ReturnLines(Return {
        refund: id(refund),
        lines: lines.iter().map(|&(line, quantity)| (id(line), quantity)).collect(),
        reason: reason("damaged"),
    })
}

/// What the device's last return refunded for each line: its net and tax.
fn last_refunds(device: &Device) -> Vec<(Id<Line>, Quantity, i64, i64)> {
    let Some(OrderEvent::LinesReturned(returned)) = device.last.clone() else {
        return Vec::new();
    };
    returned
        .lines
        .iter()
        .map(|line| (line.line, line.quantity, line.net.minor(), line.tax.minor()))
        .collect()
}

#[test]
fn lines_are_returned_and_refunded_their_share_of_what_was_paid() {
    let mut device = closed_order();
    assert_eq!(device.order.charged(id(1)), Some((usd(1_000), usd(89))));
    assert_eq!(device.order.charged(id(2)), Some((usd(1_000), usd(0))), "both checks' parts");
    // One of three: a third of $10.00 and of $0.89, rounded half up: $3.33 and $0.30.
    device.run(returning(0xC1, &[(1, each(1))])).unwrap();
    assert_eq!(last_refunds(&device), [(id(1), each(1), 333, 30)]);
    // The second: two thirds less one, $6.67 − $3.33 and $0.59 − $0.30.
    device.run(returning(0xC2, &[(1, each(1))])).unwrap();
    assert_eq!(last_refunds(&device), [(id(1), each(1), 334, 29)]);
    // The last refunds exactly what is left, and the shared line its two checks' parts. The
    // comped line refunds nothing.
    let half = Quantity::from_micros(500_000, Unit::Kilogram);
    device.run(returning(0xC3, &[(3, each(1)), (2, half), (1, each(1))])).unwrap();
    assert_eq!(
        last_refunds(&device),
        [(id(1), each(1), 333, 30), (id(2), half, 1_000, 0), (id(3), each(1), 0, 0)]
    );
    let returned = device.order.line(id(1)).unwrap().returned().unwrap();
    assert_eq!((returned.quantity, returned.net, returned.tax), (each(3), usd(1_000), usd(89)));
    assert!(device.order.conflicts().is_empty());
    // Nothing is left of them.
    assert_eq!(
        device.run(returning(0xC4, &[(1, each(1))])),
        Err(CommandError::ReturnBeyond(id(1)))
    );
}

#[test]
fn return_commands_follow_the_rules() {
    let mut device = closed_order();
    let half = Quantity::from_micros(500_000, Unit::Kilogram);
    assert_eq!(
        device.run(returning(0xC1, &[(1, each(4))])),
        Err(CommandError::ReturnBeyond(id(1)))
    );
    assert_eq!(device.run(returning(0xC1, &[(9, each(1))])), Err(CommandError::UnknownLine(id(9))));
    assert_eq!(device.run(returning(0xC1, &[(1, half)])), Err(CommandError::WrongUnit(id(1))));
    assert_eq!(
        device.run(returning(0xC1, &[(1, each(0))])),
        Err(CommandError::Invalid(PayloadError::Invalid("quantity")))
    );
    assert_eq!(
        device.run(returning(0xC1, &[(1, each(1)), (1, each(1))])),
        Err(CommandError::Invalid(PayloadError::Invalid("lines")))
    );
    assert_eq!(
        device.run(returning(0xC1, &[])),
        Err(CommandError::Invalid(PayloadError::Invalid("lines")))
    );
    // Only the owner returns it.
    assert_eq!(
        device.order.decide(location(), id(0xD2), returning(0xC1, &[(1, each(1))])),
        Err(CommandError::NotOwner(id(0xD)))
    );
    device.run(returning(0xC1, &[(1, each(1))])).unwrap();
    assert_eq!(
        device.run(returning(0xC1, &[(1, each(1))])),
        Err(CommandError::ReturnExists(id(0xC1)))
    );
    // An order with a return stays closed.
    assert_eq!(device.run(OrderCommand::Reopen(reason("wrong"))), Err(CommandError::Returned));
    // An open order's lines aren't returned.
    let mut open = Device::with_line(true);
    assert_eq!(open.run(returning(0xC1, &[(1, each(1))])), Err(CommandError::NotClosed));
}

#[test]
fn returns_that_dont_fit_are_reported() {
    let mut script = Script::created();
    script.apply(&OrderEvent::LineAdded(added(1, usd(400))));
    let returned = |refund: u64, line: u64, quantity, net| {
        OrderEvent::LinesReturned(LinesReturned {
            refund: id(refund),
            lines: vec![LineReturned {
                line: id(line),
                quantity,
                net,
                tax: Money::zero(net.currency()),
            }],
            reason: reason("damaged"),
        })
    };
    // Before the order closed, and beyond what any check charged: it applies, and is reported.
    script.apply(&returned(0xC1, 1, each(1), usd(400)));
    assert_eq!(
        script.kinds(),
        [ConflictKind::ReturnedBeforeClose, ConflictKind::ReturnedBeyond(id(1))]
    );
    script.apply(&OrderEvent::CheckClosed(closed(0xA, &[(1, 400)])));
    script.apply(&OrderEvent::Closed);
    let before = script.kinds().len();
    script.apply(&returned(0xC1, 1, each(1), usd(1)));
    script.apply(&returned(0xC2, 9, each(1), usd(1)));
    script.apply(&returned(0xC3, 1, each(1), eur(1)));
    script.apply(&returned(0xC4, 1, Quantity::from_micros(1, Unit::Kilogram), usd(1)));
    // A second of the only one: beyond its quantity.
    script.apply(&returned(0xC5, 1, each(1), usd(0)));
    script.apply(&OrderEvent::Reopened { reason: reason("wrong") });
    assert_eq!(
        script.kinds()[before..],
        [
            ConflictKind::DuplicateReturn(id(0xC1)),
            ConflictKind::UnknownLine(id(9)),
            ConflictKind::CurrencyMismatch(id(1)),
            ConflictKind::UnitMismatch(id(1)),
            ConflictKind::ReturnedBeyond(id(1)),
            ConflictKind::ReopenedAfterReturn,
        ]
    );
    let returned = script.order.line(id(1)).unwrap().returned().unwrap();
    assert_eq!((returned.quantity, returned.net), (each(2), usd(400)));
}

#[test]
fn return_payloads_keep_their_rules() {
    let decode = |returned: LinesReturned| {
        let event = OrderEvent::LinesReturned(returned);
        let schema = event.schema().to_ref().unwrap();
        OrderEvent::from_value(&schema, &event.to_value())
    };
    let invalid = Err(DecodeError::Malformed(PayloadError::Invalid("lines")));
    let line = |n, quantity, net, tax| LineReturned { line: id(n), quantity, net, tax };
    let of = |lines| LinesReturned { refund: id(0xC1), lines, reason: reason("damaged") };
    assert!(decode(of(vec![line(1, each(1), usd(0), usd(0))])).is_ok(), "a comp refunds nothing");
    assert_eq!(decode(of(vec![])), invalid);
    assert_eq!(
        decode(of(vec![line(2, each(1), usd(1), usd(0)), line(1, each(1), usd(1), usd(0))])),
        invalid
    );
    assert_eq!(
        decode(of(vec![line(1, each(1), usd(1), usd(0)), line(1, each(1), usd(1), usd(0))])),
        invalid
    );
    assert_eq!(decode(of(vec![line(1, each(0), usd(1), usd(0))])), invalid);
    assert_eq!(decode(of(vec![line(1, each(1), usd(-1), usd(0))])), invalid);
    assert_eq!(decode(of(vec![line(1, each(1), usd(1), usd(-1))])), invalid);
    assert_eq!(decode(of(vec![line(1, each(1), usd(1), eur(0))])), invalid);
    assert_eq!(
        decode(of(vec![line(1, each(1), usd(1), usd(0)), line(2, each(1), eur(1), eur(0))])),
        invalid
    );
}

#[test]
fn shares_round_half_up_and_add_up() {
    let third = |part| crate::order::share(usd(1_000), each(part), each(3)).map(Money::minor);
    assert_eq!(
        [third(0), third(1), third(2), third(3)],
        [Some(0), Some(333), Some(667), Some(1_000)]
    );
    // Half a cent rounds up: $0.05 halved.
    assert_eq!(crate::order::share(usd(5), each(1), each(2)), Some(usd(3)));
    assert_eq!(crate::order::share(usd(5), each(3), each(2)), None, "more than the whole");
    assert_eq!(crate::order::share(usd(-5), each(1), each(2)), None);
    assert_eq!(
        crate::order::share(usd(5), each(1), Quantity::from_micros(1, Unit::Kilogram)),
        None
    );
    assert_eq!(crate::order::share(usd(5), each(0), each(0)), None);
    assert_eq!(crate::order::share(usd(i64::MAX), each(1), each(1)), Some(usd(i64::MAX)));
    assert_eq!(refunded(usd(1_000), usd(333), each(1), each(1), each(3)), Some(usd(334)));
    assert_eq!(refunded(usd(1_000), usd(1_000), each(3), each(1), each(3)), None);
    // The last refunds what is left, whatever was refunded before.
    assert_eq!(refunded(usd(1_000), usd(666), each(2), each(1), each(3)), Some(usd(334)));
    // Never below nothing, nor beyond what is left; nothing once more than the charge went.
    assert_eq!(refunded(usd(1_000), usd(700), each(1), each(1), each(3)), Some(usd(0)));
    assert_eq!(refunded(usd(1_000), usd(1_001), each(1), each(1), each(3)), None);
}

/// Two returns worked out on two devices, each the share of one of three, and then the last one
/// here: it refunds what is left of the charge, so the three add up to it. Found in review: it
/// refunded its share, a cent short.
#[test]
fn the_last_return_refunds_what_is_left_whatever_came_before() {
    let mut device = closed_order();
    for (n, other) in [(0xC1_u64, 0xD), (0xC2, 0xD2)] {
        let event = OrderEvent::LinesReturned(LinesReturned {
            refund: id(n),
            lines: vec![LineReturned {
                line: id(1),
                quantity: each(1),
                net: usd(333),
                tax: usd(30),
            }],
            reason: reason("damaged"),
        });
        let mut meta = device.script.meta_at(location());
        meta.origin_device = id(other);
        device.order.apply(&meta, &event);
    }
    device.run(returning(0xC3, &[(1, each(1))])).unwrap();
    assert_eq!(last_refunds(&device), [(id(1), each(1), 334, 29)]);
    let returned = device.order.line(id(1)).unwrap().returned().unwrap();
    assert_eq!((returned.net, returned.tax), (usd(1_000), usd(89)));
}
