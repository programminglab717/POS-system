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

    // 20% off the whole order is beyond a cashier: a manager approves it, for what all the
    // order's discounts take together, $2.10 and $3.78, and the order's share, 20%.
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
        Request { action: Action::DiscountApply, amount: Some(usd(588)), percent: Some(2_000) };
    assert_eq!(approved(&runtime), [asked]);

    // $1.00 more off the line: the line's discounts together take 14.77% of it (0.9 × 17.90 /
    // 18.90 is left), and the order's $6.68 in all, past a cashier's $5.00. Found in review:
    // counted alone, as 5.3%, it needed no approval, so discounts could be stacked.
    let dollar = DiscountValue::Amount(usd(100));
    assert!(matches!(
        runtime.apply_discount(order, Some(line), dollar, reason("damaged")),
        Err(RuntimeError::ApprovalRequired(Action::DiscountApply))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    let ticket = runtime.apply_discount(order, Some(line), dollar, reason("damaged")).unwrap();
    assert_eq!(ticket.lines[0].discounts[1].amount.money, usd(100));
    assert_eq!(ticket.lines[0].discounts[1].percent, None);
    assert_eq!(ticket.discounts[0].amount.money, usd(358));
    assert_eq!(ticket.subtotal.money, usd(1_432));
    assert_eq!(approved(&runtime)[1].amount, Some(usd(668)));
    assert_eq!(approved(&runtime)[1].percent, Some(1_477));

    // $6.00 more: $11.48 in all, and the line's discounts together 43.34% of it.
    let six = DiscountValue::Amount(usd(600));
    assert!(matches!(
        runtime.apply_discount(order, Some(line), six, reason("damaged")),
        Err(RuntimeError::ApprovalRequired(Action::DiscountApply))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.apply_discount(order, Some(line), six, reason("damaged")).unwrap();
    let third = approved(&runtime)[2];
    assert_eq!(third.amount, Some(usd(1_148)));
    assert_eq!(third.percent, Some(4_334));

    // Taking one off needs no approval.
    let ticket = runtime.ticket(order).unwrap();
    let off = ticket.lines[0].discounts[2].discount;
    let ticket = runtime.remove_discount(order, off).unwrap();
    assert_eq!(ticket.lines[0].discounts.len(), 2);
    assert_eq!(ticket.subtotal.money, usd(1_432));
    assert_eq!(approved(&runtime).len(), 3, "taking one off asks for nothing");
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

/// Sam, a cashier, signed in, with two espressos, $6.50, on an order.
fn two_espressos(runtime: &mut TestRuntime) -> Ticket {
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::new(2).unwrap()).unwrap()
}

#[test]
fn a_price_set_by_hand_is_approved_for_the_difference_over_the_whole_line() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let ticket = two_espressos(&mut runtime);
    let (order, line) = (ticket.order, ticket.lines[0].line);
    // The same price changes nothing, and asks for no approval.
    assert!(matches!(
        runtime.reprice_line(order, line, usd(325), reason("price_match")),
        Err(RuntimeError::Order(_))
    ));
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NothingToApprove)));
    // A cashier's role doesn't price by hand: $2.75 instead of $3.25, for two, is $1.00 off.
    assert!(matches!(
        runtime.reprice_line(order, line, usd(275), reason("price_match")),
        Err(RuntimeError::ApprovalRequired(Action::PriceOverride))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    let ticket = runtime.reprice_line(order, line, usd(275), reason("price_match")).unwrap();
    let asked = Request { action: Action::PriceOverride, amount: Some(usd(100)), percent: None };
    assert_eq!(approved(&runtime), [asked]);
    let repriced = ticket.lines[0].repriced.as_ref().unwrap();
    assert_eq!(repriced.unit_price.money, usd(275));
    assert_eq!(repriced.rung.money, usd(325));
    assert_eq!(repriced.reason.as_str(), "price_match");
    assert_eq!(ticket.lines[0].amount.money, usd(550));
    assert_eq!(ticket.subtotal.money, usd(550));
    // Up as well as down: $3.00 is $0.25 more each, $0.50, and a manager prices alone.
    runtime.sign_in(id(0x501), "1234").unwrap();
    let ticket = runtime.reprice_line(order, line, usd(300), reason("upcharge")).unwrap();
    assert_eq!(ticket.lines[0].amount.money, usd(600));
    assert_eq!(approved(&runtime).len(), 1, "no approval for what a role permits");
    // Free is a price; less than free isn't.
    runtime.reprice_line(order, line, usd(0), reason("manager")).unwrap();
    assert!(matches!(
        runtime.reprice_line(order, line, usd(-1), reason("manager")),
        Err(RuntimeError::Order(_))
    ));
}

#[test]
fn a_lines_notes_are_set_and_cleared() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let ticket = two_espressos(&mut runtime);
    let (order, line) = (ticket.order, ticket.lines[0].line);
    let notes = keel_domain::codec::Note::new("extra hot").unwrap();
    let ticket = runtime.set_line_notes(order, line, Some(notes.clone())).unwrap();
    assert_eq!(ticket.lines[0].notes, Some(notes.clone()));
    // The same notes change nothing.
    assert!(matches!(
        runtime.set_line_notes(order, line, Some(notes)),
        Err(RuntimeError::Order(_))
    ));
    let ticket = runtime.set_line_notes(order, line, None).unwrap();
    assert_eq!(ticket.lines[0].notes, None);
    assert!(matches!(runtime.set_line_notes(order, line, None), Err(RuntimeError::Order(_))));
}

/// Found in review: discounts within a cashier's limits one by one went past them together, an
/// amount larger than its line took more as the line grew, and a discount after part of a check
/// was paid left change no one handed back.
#[test]
fn discounts_stay_within_the_limits_together_and_as_lines_grow() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let ticket = four_lattes(&mut runtime);
    let (order, line) = (ticket.order, ticket.lines[0].line);
    let tenth = DiscountValue::Percent(1_000);
    runtime.apply_discount(order, Some(line), tenth, reason("loyalty")).unwrap();
    // Two of 10% on one line are 19% of it.
    assert!(matches!(
        runtime.apply_discount(order, Some(line), tenth, reason("loyalty")),
        Err(RuntimeError::ApprovalRequired(Action::DiscountApply))
    ));
    // More than is left of the line is refused for what it is: $18.90 is left.
    assert!(matches!(
        runtime.apply_discount(order, Some(line), DiscountValue::Amount(usd(5_000)), reason("damaged")),
        Err(RuntimeError::DiscountTooLarge { left }) if left == usd(1_890)
    ));
    // Four lattes to forty takes $21.00 off, past a cashier's $5.00: asked for.
    assert!(matches!(
        runtime.change_quantity(order, line, NonZeroU32::new(40).unwrap()),
        Err(RuntimeError::ApprovalRequired(Action::DiscountApply))
    ));
    // Four to five takes $2.63: within it.
    runtime.change_quantity(order, line, NonZeroU32::new(5).unwrap()).unwrap();
    // Part of the check paid, its lines' discounts and prices stay as they were paid for.
    runtime.pay_check_cash(order, order.cast(), Some(usd(500)), usd(500)).unwrap();
    assert!(matches!(
        runtime.apply_discount(order, None, DiscountValue::Percent(500), reason("loyalty")),
        Err(RuntimeError::PartlyPaid(_))
    ));
    runtime.sign_in(id(0x501), "1234").unwrap();
    assert!(matches!(
        runtime.reprice_line(order, line, usd(100), reason("price_match")),
        Err(RuntimeError::PartlyPaid(_))
    ));
    assert!(matches!(runtime.remove_line(order, line), Err(RuntimeError::PartlyPaid(_))));
    assert!(matches!(
        runtime.change_quantity(order, line, NonZeroU32::new(1).unwrap()),
        Err(RuntimeError::PartlyPaid(_))
    ));
}

/// Found in review: a price lowered in steps was asked for one step at a time, so each step could
/// stay under a limit; it is asked for as how far it is from the price it was rung at.
#[test]
fn a_price_is_asked_for_from_the_rung_one() {
    let scratch = Scratch::new();
    let limited = crate::tests::demo_with(|data| {
        data.roles[1].permissions.push(keel_domain::staff::Permission {
            action: Action::PriceOverride,
            most_amount: Some(100),
            most_percent: None,
        });
    });
    let mut runtime = register(&scratch, limited);
    let ticket = two_espressos(&mut runtime);
    let (order, line) = (ticket.order, ticket.lines[0].line);
    // $3.25 to $2.80, for two, is $0.90 off: within the cashier's $1.00.
    runtime.reprice_line(order, line, usd(280), reason("price_match")).unwrap();
    // $2.80 to $2.70 is a step of $0.20, but $1.10 from $3.25: asked for.
    assert!(matches!(
        runtime.reprice_line(order, line, usd(270), reason("price_match")),
        Err(RuntimeError::ApprovalRequired(Action::PriceOverride))
    ));
    // More of a line priced by hand moves it further from the rung price: three at $2.80 are
    // $1.35 from three at $3.25.
    assert!(matches!(
        runtime.change_quantity(order, line, NonZeroU32::new(3).unwrap()),
        Err(RuntimeError::ApprovalRequired(Action::PriceOverride))
    ));
}
