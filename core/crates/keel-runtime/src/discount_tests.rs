//! Known answers for discounts at the register (ADR-0030): put on and taken off, priced on the
//! ticket, and gated by the member's role.

use core::num::NonZeroU32;

use keel_domain::codec::ReasonCode;
use keel_domain::order::{DiscountValue, Mode, Reason};
use keel_domain::profile::demo;
use keel_domain::staff::{Action, Request};

use crate::tests::{Scratch, TestRuntime, id, register, usd};
use crate::{RuntimeError, Ticket};

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

/// Sam, a cashier, signed in, with four large lattes, $21.00, on an order.
fn four_lattes(runtime: &mut TestRuntime) -> Ticket {
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x403), &[], NonZeroU32::new(4).unwrap()).unwrap()
}

/// The requests of the approvals in the device's log, in order.
fn approved(runtime: &TestRuntime) -> Vec<Request> {
    let log = runtime.store().log(id(0xd1), 0, 1_000).unwrap();
    log.iter()
        .filter(|event| event.body().stream.kind.as_str() == keel_domain::staff::STREAM)
        .map(|event| {
            use keel_domain::schema::DomainEvent;
            let body = event.body();
            let keel_domain::staff::ApprovalEvent::Granted(granted) =
                keel_domain::staff::ApprovalEvent::decode(&body.schema, &body.payload).unwrap();
            granted.request()
        })
        .collect()
}

#[test]
fn discounts_price_on_the_ticket_within_the_cashiers_limits() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let ticket = four_lattes(&mut runtime);
    let (order, line) = (ticket.order, ticket.lines[0].line);

    // 10% off the line takes $2.10: within a cashier's 10% and $5.00.
    let ticket =
        runtime.apply_discount(order, Some(line), DiscountValue::Percent(1_000), reason("loyalty"));
    let ticket = ticket.unwrap();
    let on_line = &ticket.lines[0].discounts;
    assert_eq!(on_line.len(), 1);
    assert_eq!(on_line[0].amount.money, usd(210));
    assert_eq!(on_line[0].amount.text, "$2.10");
    assert_eq!(on_line[0].percent, Some(1_000));
    assert_eq!(on_line[0].reason.as_str(), "loyalty");
    assert_eq!(ticket.lines[0].amount.money, usd(2_100), "a line's amount is before discounts");
    assert_eq!(ticket.subtotal.money, usd(1_890));

    // 20% off the whole order is beyond a cashier: a manager approves it, for what it takes.
    let whole = DiscountValue::Percent(2_000);
    assert!(matches!(
        runtime.apply_discount(order, None, whole, reason("manager_comp")),
        Err(RuntimeError::ApprovalRequired(Action::DiscountApply))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    let ticket = runtime.apply_discount(order, None, whole, reason("manager_comp")).unwrap();
    assert_eq!(ticket.discounts.len(), 1);
    assert_eq!(ticket.discounts[0].amount.money, usd(378));
    // $18.90 less $3.78 is $15.12, with $1.34 of tax at 8.875%.
    assert_eq!(ticket.subtotal.money, usd(1_512));
    assert_eq!(ticket.total.money, usd(1_646));
    let asked =
        Request { action: Action::DiscountApply, amount: Some(usd(378)), percent: Some(2_000) };
    assert_eq!(approved(&runtime), [asked]);

    // An amount off counts as the share it takes of what is left of its line, rounded up: $1.00
    // of $18.90 is 5.3%. The order's 20% then takes less.
    let dollar = DiscountValue::Amount(usd(100));
    let ticket = runtime.apply_discount(order, Some(line), dollar, reason("damaged")).unwrap();
    assert_eq!(ticket.lines[0].discounts[1].amount.money, usd(100));
    assert_eq!(ticket.lines[0].discounts[1].percent, None);
    assert_eq!(ticket.discounts[0].amount.money, usd(358));
    assert_eq!(ticket.subtotal.money, usd(1_432));
    assert_eq!(approved(&runtime).len(), 1, "within the cashier's role");

    // $6.00 off is beyond a cashier's $5.00.
    let six = DiscountValue::Amount(usd(600));
    assert!(matches!(
        runtime.apply_discount(order, Some(line), six, reason("damaged")),
        Err(RuntimeError::ApprovalRequired(Action::DiscountApply))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.apply_discount(order, Some(line), six, reason("damaged")).unwrap();
    let second = approved(&runtime)[1];
    assert_eq!(second.amount, Some(usd(600)));
    // $6.00 of the $17.90 left: 33.5%, rounded up to 3,352 basis points.
    assert_eq!(second.percent, Some(3_352));

    // Taking one off needs no approval.
    let ticket = runtime.ticket(order).unwrap();
    let off = ticket.lines[0].discounts[2].discount;
    let ticket = runtime.remove_discount(order, off).unwrap();
    assert_eq!(ticket.lines[0].discounts.len(), 2);
    assert_eq!(ticket.subtotal.money, usd(1_432));
    assert!(matches!(runtime.remove_discount(order, off), Err(RuntimeError::Order(_))));

    // Paid, the ticket shows what the check's snapshot charged, and the discounts still.
    let paid = runtime.pay_cash(order, usd(2_000)).unwrap();
    assert_eq!(paid.ticket.subtotal.money, usd(1_432));
    assert_eq!(paid.ticket.lines[0].discounts.len(), 2);
    assert_eq!(paid.ticket.discounts[0].amount.money, usd(358));
    // A closed order takes no more.
    assert!(matches!(
        runtime.apply_discount(order, Some(line), dollar, reason("damaged")),
        Err(RuntimeError::Order(_))
    ));
}

#[test]
fn a_discount_the_order_refuses_asks_for_no_approval() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let ticket = four_lattes(&mut runtime);
    let order = ticket.order;
    // An amount off the whole order, or a line the order doesn't have: refused before any
    // approval is asked for.
    let six = DiscountValue::Amount(usd(600));
    assert!(matches!(
        runtime.apply_discount(order, None, six, reason("damaged")),
        Err(RuntimeError::Order(_))
    ));
    assert!(matches!(
        runtime.apply_discount(order, Some(id(0xbad)), six, reason("damaged")),
        Err(RuntimeError::Order(_))
    ));
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NothingToApprove)));
    assert!(matches!(
        runtime.apply_discount(id(0xbad), None, DiscountValue::Percent(1), reason("damaged")),
        Err(RuntimeError::UnknownOrder(_))
    ));
}
