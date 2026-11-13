//! Known answers for returns at the register (ADR-0032): an order found from its receipt, lines
//! returned in parts and refunded what was paid, in cash out of the drawer, a refund gated as the
//! member's role says, its receipt, and the day's reports.

use core::num::NonZeroU32;

use keel_doc::escpos::plain_text;
use keel_doc::{Block, Columns, Doc};
use keel_domain::codec::ReasonCode;
use keel_domain::order::{CommandError, Mode, Order, Reason};
use keel_domain::profile::demo;
use keel_domain::staff::Action;
use keel_types::{Quantity, Unit};

use crate::tests::{Scratch, TestRuntime, id, register, usd};
use crate::{JobKind, ReportKind, RuntimeError};

fn reason() -> Reason {
    Reason { code: ReasonCode::new("damaged").unwrap(), note: None }
}

fn each(count: i64) -> Quantity {
    Quantity::from_whole(count, Unit::Each).unwrap()
}

/// The documents queued of `kind`, in order.
fn docs(runtime: &TestRuntime, kind: JobKind) -> Vec<Doc> {
    runtime
        .print_jobs()
        .unwrap()
        .into_iter()
        .filter(|job| job.kind == kind)
        .map(|job| {
            let queued = runtime.store().effect(&job.job.to_bytes()).unwrap().unwrap();
            Doc::decode(&queued.effect.payload).unwrap()
        })
        .collect()
}

/// Three espressos sold in exact cash: $9.75 with $0.87 of tax at 8.875%, $10.62. The order.
fn three_espressos(runtime: &mut TestRuntime) -> keel_types::Id<Order> {
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::new(3).unwrap()).unwrap();
    runtime.pay_cash(order, usd(1_062)).unwrap();
    order
}

#[test]
fn a_cashier_returns_espressos_once_a_manager_approves_each_refunded_what_was_paid() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x502), "2468").unwrap();
    runtime.open_drawer(usd(10_000)).unwrap();
    let order = three_espressos(&mut runtime);
    let line = runtime.ticket(order).unwrap().lines[0].line;
    // Found from its receipt's code, whatever its case.
    let text = order.to_string();
    let code = &text[text.len() - 6..];
    assert_eq!(runtime.find_orders(&code.to_uppercase()).unwrap(), [order]);
    assert_eq!(runtime.find_orders(code).unwrap(), [order]);
    assert!(runtime.find_orders("zzzzzz").unwrap().is_empty());
    let returnable = runtime.returnable(order).unwrap();
    assert_eq!(returnable.lines.len(), 1);
    assert_eq!(returnable.lines[0].left_count, Some(3));
    assert_eq!(returnable.lines[0].charged.money, usd(1_062));
    assert!(returnable.cash_only);
    // A cashier's role refunds nothing: a manager approves it.
    assert!(matches!(
        runtime.return_lines(order, &[(line, each(1))], reason()),
        Err(RuntimeError::ApprovalRequired(Action::RefundIssue))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    // A third of $9.75 and of $0.87: $3.25 and $0.29.
    let first = runtime.return_lines(order, &[(line, each(1))], reason()).unwrap();
    assert_eq!((first.amount.money, first.handed.money), (usd(354), usd(354)));
    assert_eq!(first.lines[0].amount.money, usd(354));
    // The other two: what is left, $6.50 and $0.58.
    let rest = runtime.return_lines(order, &[(line, each(2))], reason());
    assert!(matches!(rest, Err(RuntimeError::ApprovalRequired(Action::RefundIssue))));
    runtime.approve(id(0x501), "1234").unwrap();
    let rest = runtime.return_lines(order, &[(line, each(2))], reason()).unwrap();
    assert_eq!(rest.amount.money, usd(708));
    let returnable = runtime.returnable(order).unwrap();
    assert_eq!(returnable.lines[0].left_count, Some(0));
    assert_eq!(returnable.refunded.money, usd(1_062), "every cent paid, and no more");
    // Nothing is left: the order's refusal, asking for no approval.
    assert!(matches!(
        runtime.return_lines(order, &[(line, each(1))], reason()),
        Err(RuntimeError::Order(CommandError::ReturnBeyond(_)))
    ));
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NothingToApprove)));
    // The drawer paid them out: $100.00 + $10.62 − $10.62, counted exactly.
    let closed = runtime.close_drawer(usd(10_000)).unwrap();
    assert_eq!(closed.refunded.unwrap().money, usd(1_062));
    assert_eq!(closed.variance.unwrap().money, usd(0));
    // Each refund kicked the drawer and printed its receipt.
    let receipts = docs(&runtime, JobKind::Receipt);
    let refund = plain_text(&receipts[receipts.len() - 2], Columns::PAPER_80MM);
    assert!(refund.contains("REFUND"), "{refund}");
    assert!(refund.contains(&format!("Order {}", code.to_uppercase())), "{refund}");
    assert!(refund.contains("2 Espresso"), "{refund}");
    assert!(refund.contains("REFUNDED                                   $7.08"), "{refund}");
    assert!(!refund.contains("Cash handed back"), "{refund}");
    let kicks = docs(&runtime, JobKind::Drawer);
    assert!(kicks.iter().filter(|kick| kick.blocks() == [Block::DrawerKick]).count() >= 3);
}

#[test]
fn a_refund_in_cash_is_rounded_as_the_location_rounds_cash() {
    use keel_types::{RoundingMode, RoundingRule};
    let scratch = Scratch::new();
    let mut data = demo().unwrap().data().clone();
    data.cash_rounding =
        Some(RoundingRule::new(RoundingMode::HalfAwayFromZero, NonZeroU32::new(5).unwrap()));
    let mut runtime = register(&scratch, keel_domain::profile::Profile::new(data).unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime.open_drawer(usd(10_000)).unwrap();
    // An espresso, $3.54, paid with $3.55 in cash.
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let line = runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap().lines[0].line;
    runtime.pay_cash(order, usd(500)).unwrap();
    let refunded = runtime.return_lines(order, &[(line, each(1))], reason()).unwrap();
    assert_eq!((refunded.amount.money, refunded.handed.money), (usd(354), usd(355)));
    // The day's report has what was handed back.
    let today = runtime.today().unwrap();
    let x = runtime.day_report(today, ReportKind::X).unwrap();
    assert_eq!(x.refunds[0].amount.money, usd(355));
    let receipt = plain_text(docs(&runtime, JobKind::Receipt).last().unwrap(), Columns::PAPER_80MM);
    assert!(receipt.contains("Cash handed back                           $3.55"), "{receipt}");
    // The drawer took $3.55 and handed back $3.55.
    let closed = runtime.close_drawer(usd(10_000)).unwrap();
    assert_eq!(closed.variance.unwrap().money, usd(0));
}

#[test]
fn a_return_of_a_free_line_refunds_nothing_and_opens_no_drawer() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let ticket = runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    let line = ticket.lines[0].line;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    // Given away: all of it off.
    let free = keel_domain::order::DiscountValue::Percent(10_000);
    runtime.apply_discount(order, Some(line), free, reason()).unwrap();
    runtime.pay_cash(order, usd(354)).unwrap();
    let kicks = docs(&runtime, JobKind::Drawer).len();
    let refunded = runtime.return_lines(order, &[(line, each(1))], reason()).unwrap();
    assert_eq!((refunded.amount.money, refunded.handed.money), (usd(0), usd(0)));
    assert_eq!(docs(&runtime, JobKind::Drawer).len(), kicks, "no cash, no kick");
    assert!(runtime.store().refunds_of(order).unwrap().is_empty(), "nothing to pay back");
    let receipt = plain_text(docs(&runtime, JobKind::Receipt).last().unwrap(), Columns::PAPER_80MM);
    assert!(receipt.contains("REFUNDED                                   $0.00"), "{receipt}");
}

#[test]
fn an_open_order_isnt_returned() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let line = runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap().lines[0].line;
    assert!(matches!(
        runtime.return_lines(order, &[(line, each(1))], reason()),
        Err(RuntimeError::Order(CommandError::NotClosed))
    ));
    assert!(
        runtime.find_orders(&order.to_string()[30..]).unwrap().is_empty(),
        "only closed orders"
    );
    assert!(matches!(
        runtime.return_lines(id(0x999), &[(line, each(1))], reason()),
        Err(RuntimeError::UnknownOrder(_))
    ));
}

#[test]
fn a_days_reports_list_its_refunds() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    let today = runtime.today().unwrap();
    // A refund with no drawer open, then one with a drawer open.
    let order = three_espressos(&mut runtime);
    let line = runtime.ticket(order).unwrap().lines[0].line;
    runtime.return_lines(order, &[(line, each(1))], reason()).unwrap();
    runtime.open_drawer(usd(10_000)).unwrap();
    runtime.return_lines(order, &[(line, each(1))], reason()).unwrap();
    let x = runtime.day_report(today, ReportKind::X).unwrap();
    // The sale stands as it was rung; its refunds are listed apart.
    assert_eq!(x.total.money, usd(1_062));
    assert_eq!(x.refunds.len(), 1);
    assert_eq!((x.refunds[0].payments, x.refunds[0].amount.money), (2, usd(708)));
    assert_eq!((x.refunds_without_drawer, x.cash_refunded_without_drawer.money), (1, usd(354)));
    runtime.close_drawer(usd(9_646)).unwrap();
    runtime.print_report(today, ReportKind::Z).unwrap();
    let printed = plain_text(docs(&runtime, JobKind::Receipt).last().unwrap(), Columns::PAPER_80MM);
    assert!(printed.contains("Cash refunds (2)                           $7.08"), "{printed}");
    assert!(printed.contains("Refunded with no drawer (1)                $3.54"), "{printed}");
}

/// A role that refunds up to $5.00 refunds an espresso alone, and needs a manager for two: a
/// refund is asked for as what it refunds.
#[test]
fn a_refund_beyond_the_role_waits_for_a_manager() {
    use keel_domain::staff::Permission;
    let scratch = Scratch::new();
    let refunds = crate::tests::demo_with(|data| {
        data.roles[1].permissions.push(Permission {
            action: Action::RefundIssue,
            most_amount: Some(500),
            most_percent: None,
        });
    });
    let mut runtime = register(&scratch, refunds);
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = three_espressos(&mut runtime);
    let line = runtime.ticket(order).unwrap().lines[0].line;
    runtime.return_lines(order, &[(line, each(1))], reason()).unwrap();
    assert!(matches!(
        runtime.return_lines(order, &[(line, each(2))], reason()),
        Err(RuntimeError::ApprovalRequired(Action::RefundIssue))
    ));
}
