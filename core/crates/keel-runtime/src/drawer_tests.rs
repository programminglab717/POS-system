//! Known answers for the device's drawer at the register (ADR-0031): opened with a float, cash
//! sales counted into it, paid in, out and dropped, a no-sale, and its blind close, gated as the
//! member's role says.

use core::num::NonZeroU32;

use keel_doc::escpos::plain_text;
use keel_doc::{Block, Columns, Doc};
use keel_domain::codec::ReasonCode;
use keel_domain::order::{Mode, Reason};
use keel_domain::profile::demo;
use keel_domain::staff::Action;

use crate::tests::{Scratch, TestRuntime, id, register, usd};
use crate::{JobKind, RuntimeError};
use keel_types::Money;

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
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

/// A sale of one espresso, $3.25 with $0.29 of tax at 8.875%, paid with `tendered`.
fn espresso(runtime: &mut TestRuntime, tendered: i64) {
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.pay_cash(order, usd(tendered)).unwrap();
}

#[test]
fn a_drawer_counts_its_cash_from_its_float_to_its_blind_close() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert_eq!(runtime.drawer().unwrap(), None);
    assert!(matches!(runtime.pay_in(usd(100), reason("change")), Err(RuntimeError::NoDrawer)));
    let opened = runtime.open_drawer(usd(15_000)).unwrap();
    assert!(opened.open);
    assert_eq!(opened.float.text, "$150.00");
    assert!(matches!(runtime.open_drawer(usd(100)), Err(RuntimeError::DrawerOpen)));
    // Two espressos sold in cash: $3.54 each stays in the drawer, the change given back.
    espresso(&mut runtime, 1_000);
    espresso(&mut runtime, 354);
    let drawer = runtime.drawer().unwrap().unwrap();
    // Blind: neither what it should hold, nor what moved through it, from which that is a sum.
    // Found in review.
    assert_eq!((drawer.taken, drawer.expected), (None, None));
    assert_eq!(drawer.float.money, usd(15_000));
    // $20.00 paid in and $100.00 dropped need no one.
    runtime.pay_in(usd(2_000), reason("change_from_safe")).unwrap();
    runtime.drop_cash(usd(10_000), reason("safe_drop")).unwrap();
    // A cashier's role pays nothing out: $4.00 and $6.00 each wait for a manager, each for its
    // own amount.
    for amount in [400, 600] {
        assert!(matches!(
            runtime.pay_out(usd(amount), reason("supplier")),
            Err(RuntimeError::ApprovalRequired(Action::DrawerPaidOut))
        ));
        runtime.approve(id(0x501), "1234").unwrap();
        runtime.pay_out(usd(amount), reason("supplier")).unwrap();
    }
    // $150.00 + $7.08 + $20.00 − $100.00 − $4.00 − $6.00 = $67.08 expected. Counted $67.00:
    // $0.08 short, within a cashier's $5.00.
    let closed = runtime.close_drawer(usd(6_700)).unwrap();
    assert!(!closed.open);
    assert_eq!(closed.expected.as_ref().unwrap().money, usd(6_708));
    assert_eq!(closed.counted.as_ref().unwrap().money, usd(6_700));
    assert_eq!(closed.variance.as_ref().unwrap().money, usd(-8));
    assert_eq!(closed.taken.as_ref().unwrap().money, usd(708));
    assert_eq!(closed.dropped.as_ref().unwrap().money, usd(10_000));
    assert_eq!(runtime.drawer().unwrap(), None);
    // Its report prints.
    let reports = docs(&runtime, JobKind::Receipt);
    let report = plain_text(reports.last().unwrap(), Columns::PAPER_80MM);
    assert!(report.contains("DRAWER CLOSE"), "{report}");
    assert!(report.contains("Cash sales                                 $7.08"), "{report}");
    assert!(report.contains("Expected                                  $67.08"), "{report}");
    assert!(report.contains("SHORT                                     -$0.08"), "{report}");
    // A sale with no drawer open goes on.
    espresso(&mut runtime, 1_000);
}

#[test]
fn a_close_over_or_short_beyond_the_role_waits_for_a_manager() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x502), "2468").unwrap();
    runtime.open_drawer(usd(10_000)).unwrap();
    // $5.01 short is beyond a cashier's $5.00.
    assert!(matches!(
        runtime.close_drawer(usd(9_499)),
        Err(RuntimeError::ApprovalRequired(Action::DrawerVariance))
    ));
    assert!(runtime.drawer().unwrap().unwrap().open);
    runtime.approve(id(0x501), "1234").unwrap();
    let closed = runtime.close_drawer(usd(9_499)).unwrap();
    assert_eq!(closed.variance.unwrap().money, usd(-501));
    // The approval is for the size of the variance, whatever its sign.
    let approvals = crate::staff_tests::approvals(&runtime);
    let request = approvals.last().unwrap().0.request();
    assert_eq!((request.action, request.amount), (Action::DrawerVariance, Some(usd(501))));
    // An exact close needs nothing.
    runtime.open_drawer(usd(10_000)).unwrap();
    let closed = runtime.close_drawer(usd(10_000)).unwrap();
    assert_eq!(closed.variance.unwrap().money, usd(0));
}

#[test]
fn a_no_sale_is_gated_and_kicks_the_drawer() {
    let scratch = Scratch::new();
    let strict = crate::tests::demo_with(|data| {
        data.roles[1].permissions.retain(|permission| permission.action != Action::DrawerNoSale);
    });
    let mut runtime = register(&scratch, strict);
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert!(matches!(runtime.no_sale(reason("change_for_customer")), Err(RuntimeError::NoDrawer)));
    runtime.open_drawer(usd(10_000)).unwrap();
    assert!(matches!(
        runtime.no_sale(reason("change_for_customer")),
        Err(RuntimeError::ApprovalRequired(Action::DrawerNoSale))
    ));
    assert!(docs(&runtime, JobKind::Drawer).is_empty());
    runtime.approve(id(0x501), "1234").unwrap();
    let drawer = runtime.no_sale(reason("change_for_customer")).unwrap();
    assert_eq!(drawer.no_sales, 1);
    assert_eq!(docs(&runtime, JobKind::Drawer).last().unwrap().blocks(), [Block::DrawerKick]);
    // A paid-out of nothing is the drawer's refusal, asking for no approval.
    assert!(matches!(runtime.pay_out(usd(0), reason("supplier")), Err(RuntimeError::Drawer(_))));
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NothingToApprove)));
}

/// A day's X and Z reports, from what the device holds (ADR-0031, decision 6): two espressos
/// in cash with the drawer open, one with none, one discounted, and a void.
#[test]
fn a_days_reports_add_up_its_sales_tenders_and_drawers() {
    use crate::ReportKind;
    use keel_domain::order::DiscountValue;
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    let today = runtime.today().unwrap();
    // Cash with no drawer open: $3.54, reported.
    espresso(&mut runtime, 1_000);
    runtime.open_drawer(usd(10_000)).unwrap();
    espresso(&mut runtime, 1_000);
    // $3.25 less 10%, $0.325 rounded half away from zero to $0.33: $2.92, with $0.26 of tax,
    // $3.18.
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let ticket = runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    let line = Some(ticket.lines[0].line);
    runtime.apply_discount(order, line, DiscountValue::Percent(1_000), reason("loyalty")).unwrap();
    runtime.pay_cash(order, usd(500)).unwrap();
    // The same 10% off the whole order: $3.18 again.
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.apply_discount(order, None, DiscountValue::Percent(1_000), reason("loyalty")).unwrap();
    runtime.pay_cash(order, usd(500)).unwrap();
    // A void counts apart.
    let voided = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(voided, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.abandon(voided).unwrap();
    let x = runtime.day_report(today, ReportKind::X).unwrap();
    assert_eq!(x.orders, 4);
    // Gross $13.00; $0.66 off; net $12.34; tax $0.29, $0.29, $0.26 and $0.26; total $13.44.
    assert_eq!(
        [x.gross.money, x.discounts.money, x.comps.money, x.net.money, x.tax.money, x.total.money],
        [usd(1_300), usd(66), usd(0), usd(1_234), usd(110), usd(1_344)]
    );
    assert_eq!(x.tenders.len(), 1);
    assert_eq!((x.tenders[0].payments, x.tenders[0].amount.money), (4, usd(1_344)));
    assert_eq!(x.voided, 1);
    assert_eq!(x.drawers.len(), 1);
    assert_eq!(x.drawers[0].taken, None, "an open drawer is blind, on a report too");
    assert!(x.drawers[0].open);
    assert_eq!((x.payments_without_drawer, x.cash_without_drawer.money), (1, usd(354)));
    // The Z waits for the drawer to close.
    assert!(matches!(runtime.day_report(today, ReportKind::Z), Err(RuntimeError::DrawerOpen)));
    runtime.close_drawer(usd(10_990)).unwrap();
    let z = runtime.day_report(today, ReportKind::Z).unwrap();
    assert_eq!(z.total.money, usd(1_344));
    assert_eq!(z.drawers[0].variance.as_ref().unwrap().money, usd(0));
    assert_eq!(z.drawers[0].taken.as_ref().unwrap().money, usd(990));
    // Printed, it says so, and every figure.
    runtime.print_report(today, ReportKind::Z).unwrap();
    let printed = plain_text(docs(&runtime, JobKind::Receipt).last().unwrap(), Columns::PAPER_80MM);
    assert!(printed.contains("Z REPORT"), "{printed}");
    assert!(printed.contains("TOTAL                                     $13.44"), "{printed}");
    assert!(printed.contains("Cash (4)                                  $13.44"), "{printed}");
    assert!(printed.contains("Cash with no drawer (1)                    $3.54"), "{printed}");
    // Another day has nothing.
    let tomorrow = "2099-01-01".parse().unwrap();
    assert_eq!(runtime.day_report(tomorrow, ReportKind::X).unwrap().orders, 0);
}

#[test]
fn a_drawer_takes_the_cash_as_rounded() {
    use keel_types::{RoundingMode, RoundingRule};
    let scratch = Scratch::new();
    let mut data = demo().unwrap().data().clone();
    data.cash_rounding =
        Some(RoundingRule::new(RoundingMode::HalfAwayFromZero, NonZeroU32::new(5).unwrap()));
    let mut runtime = register(&scratch, keel_domain::profile::Profile::new(data).unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime.open_drawer(usd(10_000)).unwrap();
    // An espresso, $3.54, is $3.55 in cash: the drawer holds the $3.55 the customer handed over
    // less their change.
    espresso(&mut runtime, 500);
    let closed = runtime.close_drawer(usd(10_355)).unwrap();
    assert_eq!(closed.taken.unwrap().money, usd(355));
    assert_eq!(closed.variance.unwrap().money, usd(0));
    // The cash tender is what the customer paid, rounded, as the drawer has it. Found in review.
    let today = runtime.today().unwrap();
    let x = runtime.day_report(today, crate::ReportKind::X).unwrap();
    assert_eq!(x.tenders[0].amount.money, usd(355));
}

/// A day's report counts each payment on the day it was taken, of every order, closed or not;
/// and a drawer left open past the day's end holds the next day's cash, so the next day's Z
/// waits for it. Found in review: payments went by their order's day, of closed orders only, and
/// the next day's Z passed with the drawer open, calling its cash drawerless.
#[test]
fn a_report_counts_each_payment_on_the_day_it_was_taken() {
    use crate::ReportKind;
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    let first = runtime.today().unwrap();
    runtime.open_drawer(usd(10_000)).unwrap();
    // Rung on the first day, paid on the next.
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.clock().set("2026-10-03T13:00:00Z".parse().unwrap());
    let second = runtime.today().unwrap();
    assert_ne!(first, second);
    runtime.pay_cash(order, usd(1_000)).unwrap();
    // $1.00 toward an order that stays open.
    let open = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(open, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.pay_check_cash(open, open.cast(), Some(usd(100)), usd(100)).unwrap();
    let x = runtime.day_report(second, ReportKind::X).unwrap();
    assert_eq!(x.orders, 0, "the order closed was rung the day before");
    assert_eq!((x.tenders[0].payments, x.tenders[0].amount.money), (2, usd(454)));
    assert_eq!((x.payments_without_drawer, x.cash_without_drawer.money), (0, usd(0)));
    assert!(x.drawers.is_empty(), "the drawer opened the day before");
    let before = runtime.day_report(first, ReportKind::X).unwrap();
    assert_eq!((before.orders, before.tenders.len(), before.drawers.len()), (1, 0, 1));
    // The drawer opened the day before holds the day's cash: no Z until it closes.
    assert!(matches!(runtime.day_report(second, ReportKind::Z), Err(RuntimeError::DrawerOpen)));
    let closed = runtime.close_drawer(usd(10_454)).unwrap();
    assert_eq!(closed.variance.unwrap().money, usd(0));
    runtime.day_report(second, ReportKind::Z).unwrap();
}

/// A drawer opened before the location's profile changed its currency would refuse every cash
/// sale: the sale goes on without it, the register opens another, and the first is left open,
/// for a person to look at. Found in review: the register was stuck.
#[test]
fn a_drawer_the_register_cant_work_here_doesnt_stop_a_sale() {
    use crate::ReportKind;
    use keel_types::Currency;
    let scratch = Scratch::new();
    {
        let mut runtime = register(&scratch, demo().unwrap());
        runtime.sign_in(id(0x501), "1234").unwrap();
        runtime.open_drawer(usd(10_000)).unwrap();
    }
    let cad = |minor| Money::from_minor(minor, Currency::CAD);
    let moved = crate::tests::demo_with(|data| {
        data.currency = Currency::CAD;
        let cad = |money: Money| Money::from_minor(money.minor(), Currency::CAD);
        for item in &mut data.catalog.items {
            for variant in &mut item.variants {
                variant.price = cad(variant.price);
            }
        }
        for group in &mut data.catalog.groups {
            for modifier in &mut group.modifiers {
                modifier.price = cad(modifier.price);
            }
        }
    });
    let mut runtime = crate::tests::register_at(&scratch, moved, "2026-10-02T14:00:00Z");
    runtime.sign_in(id(0x501), "1234").unwrap();
    assert_eq!(runtime.drawer().unwrap(), None);
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.pay_cash(order, cad(1_000)).unwrap();
    let today = runtime.today().unwrap();
    let x = runtime.day_report(today, ReportKind::X).unwrap();
    assert_eq!((x.payments_without_drawer, x.cash_without_drawer.money), (1, cad(354)));
    runtime.open_drawer(cad(5_000)).unwrap();
    let closed = runtime.close_drawer(cad(5_000)).unwrap();
    assert_eq!(closed.variance.unwrap().money, cad(0));
}
