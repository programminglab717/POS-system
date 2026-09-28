//! Known-answer tests for orders: the lifecycle, each conflict rule, skipped events, and each
//! command's checks.

use core::num::{NonZeroU8, NonZeroU16};
use core::time::Duration;

use keel_events::cbor::Value;
use keel_events::envelope::{Actor, Event, Location, Payload, SchemaName, SchemaRef};
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_events::log::{EventDraft, LogConfig, LogHead, LogWriter};
use keel_types::{Currency, Hlc, Id, Money, Quantity, SeededEntropy, Timestamp, Unit};

use super::*;
use crate::aggregate::{Aggregate, EventMeta, fold};
use crate::codec::{CatalogVersion, Change, IdSet, Name, Note, PayloadError, ReasonCode};
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
}

impl Device {
    fn new() -> Device {
        Device { order: Order::new(id(0xA)), script: Script::new() }
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
        let event = self.order.decide(location(), command)?;
        let before = self.order.conflicts().len();
        let meta = self.script.meta_at(location());
        self.order.apply(&meta, &event);
        assert_eq!(self.order.conflicts().len(), before, "a valid command caused a conflict");
        Ok(())
    }
}

#[test]
fn commands_need_an_active_order_at_the_devices_location() {
    let mut device = Device::new();
    assert_eq!(device.run(OrderCommand::Abandon), Err(CommandError::NotCreated));
    device.run(OrderCommand::Create(created())).unwrap();
    assert_eq!(device.run(OrderCommand::Create(created())), Err(CommandError::AlreadyCreated));
    assert_eq!(
        device.order.decide(id(0x101), OrderCommand::Abandon),
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
        order.decide(location(), OrderCommand::AddLine(line)),
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
    ]
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
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The order payloads, pinned forever: one example of each schema. Python's `cbor2` decoded
/// each one, confirmed it is canonical, and matched it field by field against the documented
/// key tables. If this test fails, a payload format changed, and stored events would no longer
/// decode.
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
