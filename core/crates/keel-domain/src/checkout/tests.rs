//! Known-answer tests for checkout: a table paying check by check, with the arithmetic worked
//! out; the rules for starting payments and closing checks; and the issues it reports.

use core::num::NonZeroU16;

use keel_events::envelope::{Actor, Location};
use keel_pricing::{Rules, Tax};
use keel_types::{Currency, Hlc, Id, Money, Quantity, Rate, Unit};

use super::*;
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::{CatalogVersion, Name, ReasonCode};
use crate::order::{
    Allocation, Channel, ItemSnapshot, LineAdded, LinesAllocated, Mode, OrderCommand, OrderCreated,
    Reason,
};
use crate::payment::{CashTendered, PaymentAuthorized, PaymentCommand, PaymentError};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(minor: i64) -> Money {
    Money::from_minor(minor, Currency::from_code("USD").unwrap())
}

fn location() -> Id<Location> {
    id(0x100)
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

fn added(line: u64, price: i64) -> LineAdded {
    LineAdded {
        line: id(line),
        item: ItemSnapshot {
            variant: id(0x400),
            catalog_version: CatalogVersion::from_bytes([7; 32]),
            name: Name::new("Dinner").unwrap(),
            tax_category: id(0x500),
            unit_price: usd(price),
        },
        quantity: Quantity::from_whole(1, Unit::Each).unwrap(),
        modifiers: Vec::new(),
        seat: None,
        course: None,
        notes: None,
    }
}

/// An 8.875% sales tax on everything, rounded once per check.
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
    RulesVersion::from_bytes([9; 32])
}

/// An order and its payments, on one device.
struct Table {
    order: Order,
    payments: Vec<Payment>,
    events: u64,
    rules: Rules,
}

impl Table {
    /// A dine-in order in US dollars.
    fn new() -> Table {
        let mut table =
            Table { order: Order::new(id(0xA)), payments: Vec::new(), events: 0, rules: rules() };
        let created = OrderCreated {
            channel: Channel::Pos,
            mode: Mode::DineIn,
            currency: Currency::from_code("USD").unwrap(),
            revenue_center: None,
            table: None,
            guest_count: None,
            customer: None,
            owner: None,
        };
        table.run(OrderCommand::Create(created)).unwrap();
        table
    }

    fn meta(&mut self) -> EventMeta {
        self.events = self.events.checked_add(1).unwrap();
        EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location: location(),
            origin_device: id(0xD),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_790_600_000_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-09-28".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        }
    }

    fn record(&mut self, event: &OrderEvent) {
        let meta = self.meta();
        self.order.apply(&meta, event);
    }

    fn run(&mut self, command: OrderCommand) -> Result<(), CommandError> {
        let event = self.order.decide(location(), id(0xD), command)?;
        self.record(&event);
        Ok(())
    }

    fn checkout(&self) -> Checkout<'_> {
        Checkout::new(&self.order, &self.payments, &self.rules, version())
    }

    fn payment(&mut self, n: u64) -> &mut Payment {
        let at = self.payments.iter().position(|payment| payment.id() == id(n)).unwrap();
        &mut self.payments[at]
    }

    /// Starts payment `n` on `check`.
    fn start(
        &mut self,
        n: u64,
        check: u64,
        tender: Tender,
        amount: i64,
    ) -> Result<(), CheckoutError> {
        let event = self.checkout().start_payment(
            location(),
            id(0xD),
            id(n),
            id(check),
            tender,
            usd(amount),
        )?;
        let meta = self.meta();
        let mut payment = Payment::new(id(n));
        payment.apply(&meta, &event);
        self.payments.push(payment);
        Ok(())
    }

    /// Records what happened to payment `n`.
    fn pay(&mut self, n: u64, command: PaymentCommand) -> Result<(), PaymentError> {
        let event = self.payment(n).decide(location(), command)?;
        let meta = self.meta();
        self.payment(n).apply(&meta, &event);
        Ok(())
    }

    /// Closes `check`, and returns its snapshot.
    fn close(&mut self, check: u64) -> Result<CheckClosed, CheckoutError> {
        let event = self.checkout().close_check(location(), id(0xD), id(check))?;
        self.record(&event);
        let OrderEvent::CheckClosed(closed) = event else { panic!("a check's close") };
        assert_eq!(self.order.conflicts(), [], "closing a check caused a conflict");
        Ok(closed)
    }

    fn balance(&self, check: u64) -> Balance {
        self.checkout().balance(id(check)).unwrap()
    }

    /// Lines 1 to 3 for a burger, fries and wine, split as in slice 3's known answer: the
    /// burger on check 0xC2, the fries on 0xC3, and the wine shared by all three checks.
    fn split() -> Table {
        let mut table = Table::new();
        for (line, price) in [(1, 1450), (2, 495), (3, 3000)] {
            table.run(OrderCommand::AddLine(added(line, price))).unwrap();
        }
        table.run(OrderCommand::OpenCheck(id(0xC2))).unwrap();
        table.run(OrderCommand::OpenCheck(id(0xC3))).unwrap();
        let allocation =
            |line, check| Allocation { line: id(line), check: id(check), shares: NonZeroU16::MIN };
        let split = LinesAllocated::new([
            allocation(1, 0xC2),
            allocation(2, 0xC3),
            allocation(3, 0xA),
            allocation(3, 0xC2),
            allocation(3, 0xC3),
        ])
        .unwrap();
        table.run(OrderCommand::AllocateLines(split)).unwrap();
        table
    }
}

fn authorize(amount: i64) -> PaymentCommand {
    PaymentCommand::Authorize(PaymentAuthorized { amount: usd(amount), reference: None })
}

fn capture(amount: i64, tip: Option<i64>) -> PaymentCommand {
    PaymentCommand::Capture(PaymentCaptured {
        amount: usd(amount),
        tip: tip.map(usd),
        reference: None,
        cash: None,
    })
}

fn in_cash(amount: i64, tendered: i64) -> PaymentCommand {
    PaymentCommand::Capture(PaymentCaptured {
        amount: usd(amount),
        tip: None,
        reference: None,
        cash: Some(CashTendered { tendered: usd(tendered), rounding: None }),
    })
}

fn charge(line: u64, net: i64, tax: i64) -> LineCharge {
    LineCharge { line: id(line), gross: usd(net), net: usd(net), tax: usd(tax) }
}

#[test]
fn a_table_pays_check_by_check() {
    // Each check is its own sale, taxed 8.875% once: a third of the wine is 10.00.
    // Check 1 is 10.00, taxed 0.8875, so 0.89: 10.89.
    // Check 2 is 14.50 + 10.00 = 24.50, taxed 2.174375, so 2.17: 26.67.
    // Check 3 is 4.95 + 10.00 = 14.95, taxed 1.3268125, so 1.33: 16.28.
    let mut table = Table::split();
    assert_eq!(
        [0xA, 0xC2, 0xC3].map(|check| table.balance(check).due),
        [usd(1089), usd(2667), usd(1628)]
    );

    // Check 1 pays by card, with a tip on top.
    table.start(0xB1, 0xA, Tender::Card, 1089).unwrap();
    assert_eq!(table.balance(0xA).unresolved, [id(0xB1)]);
    table.pay(0xB1, authorize(1089)).unwrap();
    table.pay(0xB1, capture(1089, Some(200))).unwrap();
    let paid = table.balance(0xA);
    assert_eq!(
        (paid.total, paid.captured, paid.tips, paid.due, paid.unresolved),
        (usd(1089), usd(1089), usd(200), usd(0), vec![])
    );
    let closed = table.close(0xA).unwrap();
    assert_eq!(
        closed,
        CheckClosed {
            check: id(0xA),
            rules_version: version(),
            lines: vec![charge(3, 1000, 89)],
            taxes: vec![TaxCharge { tax: id(0x900), taxable: usd(1000), amount: usd(89) }],
            total: usd(1089),
            payments: Some(IdSet::new([id(0xB1)]).unwrap()),
        }
    );

    // Check 2 splits its tender: 20.00 in cash, then the rest by card. The check's 2.17 of tax
    // is shared by its lines in proportion to their net, 1450 : 1000, by largest remainder:
    // 1.284... and 0.885... round down to 1.28 and 0.88, and the spare cent goes to the wine.
    table.start(0xB2, 0xC2, Tender::Cash, 2000).unwrap();
    table.pay(0xB2, in_cash(2000, 2000)).unwrap();
    assert_eq!(table.balance(0xC2).due, usd(667));
    assert_eq!(table.close(0xC2), Err(CheckoutError::NotCovered { due: usd(667) }));
    table.start(0xB3, 0xC2, Tender::Card, 667).unwrap();
    // No second attempt, and no close, while the card's outcome is unknown.
    assert_eq!(
        table.start(0xB4, 0xC2, Tender::Cash, 667),
        Err(CheckoutError::Unresolved(id(0xB3)))
    );
    assert_eq!(table.close(0xC2), Err(CheckoutError::Unresolved(id(0xB3))));
    table.pay(0xB3, capture(667, None)).unwrap();
    let closed = table.close(0xC2).unwrap();
    assert_eq!(closed.lines, [charge(1, 1450, 128), charge(3, 1000, 89)]);
    assert_eq!(
        (closed.total, closed.payments),
        (usd(2667), Some(IdSet::new([id(0xB2), id(0xB3)]).unwrap()))
    );

    // Check 3 pays 16.28 in cash from a 20.00 note: 3.72 change. Its tax, 1.33, is shared
    // 495 : 1000, 0.440... and 0.889..., so 0.44 and 0.89.
    table.start(0xB5, 0xC3, Tender::Cash, 1628).unwrap();
    table.pay(0xB5, in_cash(1628, 2000)).unwrap();
    let change = table.payments.last().unwrap().captured().unwrap().change();
    assert_eq!(change, Some(usd(372)));
    let closed = table.close(0xC3).unwrap();
    assert_eq!(closed.lines, [charge(2, 495, 44), charge(3, 1000, 89)]);
    assert_eq!(closed.taxes, [TaxCharge { tax: id(0x900), taxable: usd(1495), amount: usd(133) }]);

    // A payment given twice counts once.
    let twice = table.payments.iter().chain(&table.payments);
    let twice = Checkout::new(&table.order, twice, &table.rules, version());
    assert_eq!(twice.balance(id(0xC2)), table.checkout().balance(id(0xC2)));

    // The wine's three parts add up to the bottle; the order closes, with nothing to report.
    table.run(OrderCommand::Close).unwrap();
    assert_eq!(*table.order.status(), OrderStatus::Closed);
    assert_eq!(table.checkout().issues(), Ok(vec![]));
    assert_eq!(table.order.conflicts(), []);
}

#[test]
fn payments_start_only_on_open_checks_with_nothing_unresolved() {
    let mut table = Table::new();
    table.run(OrderCommand::AddLine(added(1, 1000))).unwrap();
    // 10.00 with 8.875% tax is 10.8875, so 10.89.
    let start = |table: &Table, n: u64, check: u64, amount: Money| {
        table.checkout().start_payment(location(), id(0xD), id(n), id(check), Tender::Card, amount)
    };
    assert_eq!(start(&table, 1, 0xA, usd(1090)), Err(CheckoutError::TooMuch { due: usd(1089) }));
    assert_eq!(start(&table, 1, 0xA, usd(0)), Err(CheckoutError::InvalidAmount));
    let euros = Money::from_minor(100, Currency::from_code("EUR").unwrap());
    assert_eq!(start(&table, 1, 0xA, euros), Err(CheckoutError::WrongCurrency));
    assert_eq!(
        start(&table, 1, 0xC9, usd(100)),
        Err(CheckoutError::Order(CommandError::UnknownCheck(id(0xC9))))
    );
    assert_eq!(
        table.checkout().start_payment(id(0x101), id(0xD), id(1), id(0xA), Tender::Card, usd(100)),
        Err(CheckoutError::Order(CommandError::WrongLocation))
    );

    // Part now, the rest later; the same identifier twice is refused.
    table.start(0xB1, 0xA, Tender::Card, 500).unwrap();
    assert_eq!(start(&table, 0xB1, 0xA, usd(100)), Err(CheckoutError::PaymentExists(id(0xB1))));
    table
        .pay(
            0xB1,
            PaymentCommand::Fail(crate::payment::PaymentEnded {
                reason: reason("declined"),
                reference: None,
            }),
        )
        .unwrap();
    table.start(0xB2, 0xA, Tender::Card, 1089).unwrap();
    table.pay(0xB2, capture(1089, None)).unwrap();
    assert_eq!(start(&table, 3, 0xA, usd(1)), Err(CheckoutError::TooMuch { due: usd(0) }));
    table.close(0xA).unwrap();
    assert_eq!(
        start(&table, 3, 0xA, usd(1)),
        Err(CheckoutError::Order(CommandError::CheckClosed(id(0xA))))
    );
    table.run(OrderCommand::Close).unwrap();
    assert_eq!(start(&table, 3, 0xA, usd(1)), Err(CheckoutError::Order(CommandError::OrderClosed)));
}

#[test]
fn only_the_owning_device_starts_payments_and_closes_checks() {
    // Device 0xD made the order: another device must ask for it first (ADR-0021).
    let mut table = Table::new();
    table.run(OrderCommand::AddLine(added(1, 1000))).unwrap();
    let not_owner = CheckoutError::Order(CommandError::NotOwner(id(0xD)));
    let started = table.checkout().start_payment(
        location(),
        id(0xD2),
        id(0xB1),
        id(0xA),
        Tender::Cash,
        usd(1089),
    );
    assert_eq!(started, Err(not_owner.clone()));
    table.start(0xB1, 0xA, Tender::Cash, 1089).unwrap();
    // Its outcome is recorded whoever owns the order by then: payments don't check ownership.
    table.pay(0xB1, in_cash(1089, 1089)).unwrap();
    assert_eq!(table.checkout().close_check(location(), id(0xD2), id(0xA)), Err(not_owner));
    // The order's location comes first.
    assert_eq!(
        table.checkout().close_check(id(0x101), id(0xD2), id(0xA)),
        Err(CheckoutError::Order(CommandError::WrongLocation))
    );
    table.close(0xA).unwrap();
}

#[test]
fn checks_close_once_their_payments_cover_them() {
    let mut table = Table::new();
    table.run(OrderCommand::AddLine(added(1, 1000))).unwrap();
    table.run(OrderCommand::AddLine(added(2, 500))).unwrap();
    table.run(OrderCommand::OpenCheck(id(0xC2))).unwrap();
    let nothing = Err(CheckoutError::Order(CommandError::NothingToClose));
    assert_eq!(table.close(0xC2), nothing);
    assert_eq!(table.close(0xA), Err(CheckoutError::NotCovered { due: usd(1633) }));

    // A comped line costs nothing: its check closes with no payment.
    let moved = LinesAllocated::moving(&IdSet::new([id(2)]).unwrap(), id(0xC2));
    table.run(OrderCommand::AllocateLines(moved)).unwrap();
    table.run(OrderCommand::CompLine { line: id(2), reason: reason("birthday") }).unwrap();
    let closed = table.close(0xC2).unwrap();
    assert_eq!((closed.total, closed.payments, closed.taxes), (usd(0), None, vec![]));
    assert_eq!(
        closed.lines,
        [LineCharge { line: id(2), gross: usd(500), net: usd(0), tax: usd(0) }]
    );
    assert_eq!(table.close(0xC2), Err(CheckoutError::Order(CommandError::CheckClosed(id(0xC2)))));

    // A check paid more than it costs still closes; the overpayment is reported.
    table.start(0xB1, 0xA, Tender::Cash, 1089).unwrap();
    table.pay(0xB1, in_cash(1089, 1089)).unwrap();
    table.run(OrderCommand::CompLine { line: id(1), reason: reason("birthday") }).unwrap();
    assert_eq!(table.balance(0xA).due, usd(-1089));
    table.close(0xA).unwrap();
    assert_eq!(
        table.checkout().issues(),
        Ok(vec![Issue::Overpaid { check: id(0xA), by: usd(1089) }])
    );
}

#[test]
fn money_that_does_not_fit_the_order_is_reported() {
    // Two devices each take the whole check at once, and the first closes it before it hears of
    // the second payment: the check is overpaid, by a payment that came after it closed.
    let mut table = Table::new();
    table.run(OrderCommand::AddLine(added(1, 1000))).unwrap();
    let mut taken = Vec::new();
    for n in [0xB1, 0xB2] {
        let event = table
            .checkout()
            .start_payment(location(), id(0xD), id(n), id(0xA), Tender::Card, usd(1089))
            .unwrap();
        taken.push((n, event));
    }
    for (n, event) in taken {
        let meta = table.meta();
        let mut payment = Payment::new(id(n));
        payment.apply(&meta, &event);
        let captured = payment.decide(location(), capture(1089, None)).unwrap();
        let meta = table.meta();
        payment.apply(&meta, &captured);
        table.payments.push(payment);
    }
    let first = Checkout::new(&table.order, &table.payments[..1], &table.rules, version());
    let close = first.close_check(location(), id(0xD), id(0xA)).unwrap();
    table.record(&close);
    assert_eq!(
        table.checkout().issues(),
        Ok(vec![Issue::AfterClose(id(0xB2)), Issue::Overpaid { check: id(0xA), by: usd(1089) }])
    );

    // A replica that hasn't received the payments yet sees the check underpaid.
    let behind = Checkout::new(&table.order, core::iter::empty(), &table.rules, version());
    assert_eq!(behind.issues(), Ok(vec![Issue::Underpaid { check: id(0xA), by: usd(1089) }]));

    // A payment for a check the order doesn't have, one in another currency, and one still
    // unresolved when the order is voided.
    let mut table = Table::new();
    table.run(OrderCommand::AddLine(added(1, 1000))).unwrap();
    table.start(0xB1, 0xA, Tender::Card, 500).unwrap();
    for (n, check, amount) in [
        (0xB2, 0xC9, usd(100)),
        (0xB3, 0xA, Money::from_minor(100, Currency::from_code("EUR").unwrap())),
    ] {
        let initiated = PaymentEvent::Initiated(PaymentInitiated {
            order: id(0xA),
            check: id(check),
            tender: Tender::Card,
            amount,
        });
        let meta = table.meta();
        let mut payment = Payment::new(id(n));
        payment.apply(&meta, &initiated);
        table.payments.push(payment);
    }
    // Payments of other orders are left out.
    let mut other = Payment::new(id(0xB4));
    let meta = table.meta();
    other.apply(
        &meta,
        &PaymentEvent::Initiated(PaymentInitiated {
            order: id(0xF),
            check: id(0xF),
            tender: Tender::Cash,
            amount: usd(1),
        }),
    );
    table.payments.push(other);
    table.run(OrderCommand::Void(reason("walkout"))).unwrap();
    assert_eq!(
        table.checkout().issues(),
        Ok(vec![
            Issue::OnVoidedOrder(id(0xB1)),
            Issue::UnknownCheck(id(0xB2)),
            Issue::OnVoidedOrder(id(0xB2)),
            Issue::WrongCurrency(id(0xB3)),
            Issue::OnVoidedOrder(id(0xB3)),
        ])
    );
    // The payment in euros doesn't count toward its check, but it is still unresolved there.
    let balance = table.balance(0xA);
    assert_eq!((balance.captured, balance.unresolved), (usd(0), vec![id(0xB1), id(0xB3)]));
}
