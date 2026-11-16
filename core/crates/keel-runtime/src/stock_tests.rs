//! Known answers for stock at the register (ADR-0036): received, sold, returned, wasted and
//! counted; adjusting gated as the member's role says; a return that doesn't go back to stock;
//! and the day's waste and counts.

use core::num::NonZeroU32;
use core::time::Duration;

use keel_domain::codec::ReasonCode;
use keel_domain::order::{Mode, Order, Reason};
use keel_domain::profile::demo;
use keel_domain::refs::Variant;
use keel_domain::staff::Action;
use keel_domain::stock::Movement;
use keel_types::{Clock, Id, Quantity, Unit};

use crate::tests::{Scratch, TestRuntime, id, register, usd};
use crate::{RefundTo, ReportKind, RuntimeError, StockChange};

fn each(n: i64) -> Quantity {
    Quantity::from_whole(n, Unit::Each).unwrap()
}

fn espresso() -> Id<Variant> {
    id(0x401)
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

fn manager(scratch: &Scratch) -> TestRuntime {
    let mut runtime = register(scratch, demo().unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime
}

/// `n` espressos sold in exact cash: the order.
fn sell(runtime: &mut TestRuntime, n: u32) -> Id<Order> {
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, espresso(), &[], NonZeroU32::new(n).unwrap()).unwrap();
    let due = runtime.ticket(order).unwrap().cash_due.money;
    runtime.pay_cash(order, due).unwrap();
    order
}

#[test]
fn stock_received_comes_off_as_it_sells_and_back_as_it_is_returned() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    assert!(runtime.stock().unwrap().is_empty(), "nothing tracked yet");
    // Sold before it is tracked: it doesn't count.
    sell(&mut runtime, 1);
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    let view = runtime
        .move_stock(espresso(), Movement::Received, each(20), Some(reason("delivery")))
        .unwrap();
    assert_eq!(
        (view.name.as_str(), view.on_hand, view.on_hand_text.as_str()),
        ("Espresso", each(20), "20")
    );
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    let order = sell(&mut runtime, 3);
    assert_eq!(runtime.stock_view(espresso()).unwrap().unwrap().on_hand, each(17));
    let line = runtime.ticket(order).unwrap().lines[0].line;
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    runtime.return_lines(order, &[(line, each(1))], reason("changed_mind")).unwrap();
    assert_eq!(runtime.stock_view(espresso()).unwrap().unwrap().on_hand, each(18));
    // Returned damaged: it doesn't go back; it is wasted, for the return's reason.
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    runtime
        .return_lines_with(order, &[(line, each(1))], reason("damaged"), RefundTo::Cash, false)
        .unwrap();
    let view = runtime.stock_view(espresso()).unwrap().unwrap();
    assert_eq!(view.on_hand, each(18), "returned and wasted: nothing back");
    let history: Vec<StockChange> =
        runtime.stock_history(espresso()).unwrap().iter().map(|line| line.change).collect();
    assert_eq!(
        history,
        [
            StockChange::Moved(Movement::Wasted),
            StockChange::Returned,
            StockChange::Returned,
            StockChange::Sold,
            StockChange::Moved(Movement::Received)
        ]
    );
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    assert_eq!(report.wasted.len(), 1);
    assert_eq!((report.wasted[0].name.as_str(), report.wasted[0].quantity), ("Espresso", each(1)));
}

#[test]
fn a_count_as_of_a_time_keeps_what_sold_after_it_and_reports_its_variance() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    runtime.move_stock(espresso(), Movement::Received, each(10), None).unwrap();
    runtime.clock().advance(Duration::from_secs(60)).unwrap();
    sell(&mut runtime, 2);
    let as_of = runtime.clock().now();
    runtime.clock().advance(Duration::from_secs(60)).unwrap();
    // Two sold after the count's time, while counting.
    sell(&mut runtime, 2);
    runtime.clock().advance(Duration::from_secs(60)).unwrap();
    // 7 there as of then, where 8 were expected: one short.
    let view = runtime.count_stock(espresso(), each(7), Some(as_of), None).unwrap();
    assert_eq!(view.on_hand, each(5));
    assert_eq!(view.counted_at, Some(as_of));
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    assert_eq!(report.counts.len(), 1);
    let count = &report.counts[0];
    assert_eq!((count.counted, count.expected, count.variance), (each(7), each(8), each(-1)));
    // A count as of a time to come is refused.
    let later = runtime.clock().now().checked_add(Duration::from_secs(60)).unwrap();
    assert!(matches!(
        runtime.count_stock(espresso(), each(1), Some(later), None),
        Err(RuntimeError::Stock(_))
    ));
}

#[test]
fn adjusting_wasting_and_counting_are_gated_receiving_and_setting_aside_arent() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x502), "2468").unwrap();
    runtime.move_stock(espresso(), Movement::Received, each(5), None).unwrap();
    runtime.move_stock(espresso(), Movement::Reserved, each(2), None).unwrap();
    let view = runtime.stock_view(espresso()).unwrap().unwrap();
    assert_eq!((view.on_hand, view.reserved, view.available), (each(5), each(2), each(3)));
    assert!(matches!(
        runtime.move_stock(espresso(), Movement::Released, each(3), None),
        Err(RuntimeError::Stock(_))
    ));
    runtime.move_stock(espresso(), Movement::Released, each(1), None).unwrap();
    for refused in [
        runtime.move_stock(espresso(), Movement::Wasted, each(1), None),
        runtime.move_stock(espresso(), Movement::Adjusted, each(-1), None),
        runtime.count_stock(espresso(), each(4), None, None),
    ] {
        assert!(matches!(refused, Err(RuntimeError::ApprovalRequired(Action::StockAdjust))));
    }
    runtime.approve(id(0x501), "1234").unwrap();
    let view = runtime
        .move_stock(espresso(), Movement::Adjusted, each(-1), Some(reason("miscount")))
        .unwrap();
    assert_eq!(view.on_hand, each(4));
    // An item the catalog doesn't have, or a quantity in another unit, is refused.
    assert!(matches!(
        runtime.move_stock(id(0x4ff), Movement::Received, each(1), None),
        Err(RuntimeError::UnknownItem(_))
    ));
    let kilos = Quantity::from_micros(1_000_000, Unit::Kilogram);
    assert!(matches!(
        runtime.move_stock(espresso(), Movement::Received, kilos, None),
        Err(RuntimeError::Stock(_))
    ));
}

#[test]
fn below_zero_is_allowed_and_listed_first() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    runtime.move_stock(id(0x402), Movement::Received, each(3), None).unwrap();
    runtime.move_stock(espresso(), Movement::Received, each(1), None).unwrap();
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    sell(&mut runtime, 3);
    let stock = runtime.stock().unwrap();
    assert_eq!(stock[0].variant, espresso());
    assert_eq!(stock[0].on_hand, each(-2));
    assert_eq!(stock[1].on_hand, each(3));
    let _ = usd(0);
}

/// A count's default time is its event's, by the clock sales are timed by: found in review, a
/// device's clock behind the log's, as after hearing from a fast one, took a sale twice.
#[test]
fn a_count_is_as_of_the_logs_clock_when_the_devices_is_behind() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    runtime.move_stock(espresso(), Movement::Received, each(10), None).unwrap();
    let start = runtime.clock().now();
    runtime.clock().advance(Duration::from_secs(60)).unwrap();
    sell(&mut runtime, 1);
    // The device's clock goes back half a minute: the log's is still at the sale.
    runtime.clock().set(start.checked_add(Duration::from_secs(30)).unwrap());
    let view = runtime.count_stock(espresso(), each(9), None, None).unwrap();
    assert_eq!(view.on_hand, each(9), "the sale was before the count, by the log's clock");
    assert_eq!(view.conflicts, 0);
}

/// A return kept out of stock wastes only an item whose stock is tracked; and a quantity past a
/// billion is refused, as a barcode scanned into it would be. Found in review.
#[test]
fn a_return_of_an_untracked_item_wastes_nothing_and_too_much_is_refused() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = sell(&mut runtime, 2);
    let line = runtime.ticket(order).unwrap().lines[0].line;
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    runtime
        .return_lines_with(order, &[(line, each(1))], reason("damaged"), RefundTo::Cash, false)
        .unwrap();
    assert!(runtime.stock().unwrap().is_empty(), "not tracked, so not wasted either");
    let too_much = Quantity::from_micros(
        i64::try_from(keel_domain::stock::MAX_QUANTITY).unwrap() + 1,
        Unit::Each,
    );
    assert!(matches!(
        runtime.move_stock(espresso(), Movement::Received, too_much, None),
        Err(RuntimeError::Stock(keel_domain::stock::StockError::TooMuch))
    ));
    let most =
        Quantity::from_micros(i64::try_from(keel_domain::stock::MAX_QUANTITY).unwrap(), Unit::Each);
    runtime.move_stock(espresso(), Movement::Received, most, None).unwrap();
}

/// Two counts as of one time: the later recorded is the one that stands, and its variance is
/// against the earlier, as the levels take them. Found in review: each left the other out.
#[test]
fn of_two_counts_as_of_one_time_the_later_is_against_the_earlier() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    runtime.move_stock(espresso(), Movement::Received, each(10), None).unwrap();
    runtime.clock().advance(Duration::from_secs(60)).unwrap();
    let as_of = runtime.clock().now();
    runtime.clock().advance(Duration::from_secs(60)).unwrap();
    runtime.count_stock(espresso(), each(5), Some(as_of), None).unwrap();
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    let view = runtime.count_stock(espresso(), each(7), Some(as_of), None).unwrap();
    assert_eq!(view.on_hand, each(7));
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    let found: Vec<_> =
        report.counts.iter().map(|count| (count.counted, count.expected, count.variance)).collect();
    assert_eq!(found, [(each(5), each(10), each(-5)), (each(7), each(5), each(2))]);
}
