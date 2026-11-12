//! Known answers for split checks and partial payments at the register (ADR-0030): splitting
//! evenly, moving a line, paying part of a check, and each check closing, with the order closing
//! with the last.

use core::num::NonZeroU32;

use keel_doc::escpos::plain_text;
use keel_doc::{Columns, Doc};
use keel_domain::order::Mode;
use keel_domain::profile::demo;

use crate::tests::{Scratch, TestRuntime, id, register, usd};
use crate::{JobKind, RuntimeError, Ticket, TicketState};

/// Sam, a cashier, signed in, with two espressos, $6.50, on an order.
fn two_espressos(runtime: &mut TestRuntime) -> Ticket {
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::new(2).unwrap()).unwrap()
}

/// The receipts queued, as text.
fn receipts(runtime: &TestRuntime) -> Vec<String> {
    runtime
        .print_jobs()
        .unwrap()
        .into_iter()
        .filter(|job| job.kind == JobKind::Receipt)
        .map(|job| {
            let queued = runtime.store().effect(&job.job.to_bytes()).unwrap().unwrap();
            plain_text(&Doc::decode(&queued.effect.payload).unwrap(), Columns::PAPER_80MM)
        })
        .collect()
}

fn drawers(runtime: &TestRuntime) -> usize {
    runtime.print_jobs().unwrap().iter().filter(|job| job.kind == JobKind::Drawer).count()
}

#[test]
fn a_split_order_is_paid_check_by_check_and_closes_with_the_last() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let order = two_espressos(&mut runtime).order;

    // $6.50 three ways is $2.17, $2.17 and $2.16, each taxed on its own at 8.875%: $0.19 each,
    // so the checks come to $2.36, $2.36 and $2.35. The order costs what its checks do, $7.07,
    // a cent less than it would priced whole.
    let ticket = runtime.split_evenly(order, 3).unwrap();
    let parts: Vec<(u32, i64, i64)> = ticket
        .checks
        .iter()
        .map(|check| {
            (
                check.number,
                check.ticket.lines[0].amount.money.minor(),
                check.ticket.total.money.minor(),
            )
        })
        .collect();
    assert_eq!(parts, [(1, 217, 236), (2, 217, 236), (3, 216, 235)]);
    assert_eq!(ticket.total.money, usd(707));
    assert_eq!(ticket.lines[0].amount.money, usd(650), "the whole line is the parts added up");
    assert_eq!(ticket.due.money, usd(707));
    let checks: Vec<_> = ticket.checks.iter().map(|check| check.check).collect();

    // Part of the first: $1.00 of $2.36, in cash. The drawer opens; no receipt yet.
    let paid = runtime.pay_check_cash(order, checks[0], Some(usd(100)), usd(100)).unwrap();
    assert_eq!(paid.change.money, usd(0));
    assert_eq!(paid.ticket.checks[0].ticket.due.money, usd(136));
    assert_eq!(paid.ticket.checks[0].ticket.state, TicketState::Open);
    assert_eq!(paid.ticket.due.money, usd(607));
    assert_eq!((drawers(&runtime), receipts(&runtime).len()), (1, 0));
    // More than it owes is refused.
    assert!(matches!(
        runtime.pay_check_cash(order, checks[0], Some(usd(137)), usd(500)),
        Err(RuntimeError::Checkout(_))
    ));
    // The rest of it, from $5.00: $3.64 back. It closes, and its receipt prints, as check 1.
    let paid = runtime.pay_check_cash(order, checks[0], None, usd(500)).unwrap();
    assert_eq!(paid.change.money, usd(364));
    assert_eq!(paid.ticket.checks[0].ticket.state, TicketState::Closed);
    assert_eq!(paid.ticket.state, TicketState::Open);
    let printed = receipts(&runtime);
    assert_eq!(printed.len(), 1);
    assert!(printed[0].contains("Check 1"), "{}", printed[0]);
    assert!(
        printed[0].contains("TOTAL                                      $2.36"),
        "{}",
        printed[0]
    );
    assert!(
        printed[0].contains("Cash                                       $1.00"),
        "{}",
        printed[0]
    );
    assert!(
        printed[0].contains("Cash                                       $5.00"),
        "{}",
        printed[0]
    );
    // A closed check holds the split: another way is refused.
    assert!(matches!(runtime.split_evenly(order, 2), Err(RuntimeError::Order(_))));

    // The other two, and the order closes with the last.
    runtime.pay_check_cash(order, checks[1], None, usd(236)).unwrap();
    let paid = runtime.pay_check_cash(order, checks[2], None, usd(300)).unwrap();
    assert_eq!(paid.change.money, usd(65));
    assert_eq!(paid.ticket.state, TicketState::Closed);
    assert_eq!(paid.ticket.total.money, usd(707));
    assert_eq!(paid.ticket.paid.money, usd(707));
    assert_eq!(paid.ticket.due.money, usd(0));
    assert_eq!((drawers(&runtime), receipts(&runtime).len()), (4, 3));
    // Each check's receipt shows its own payments, not the others'. Found by a planted bug: the
    // last check's receipt listed the first's.
    let last = &receipts(&runtime)[2];
    assert!(last.contains("Check 3"), "{last}");
    assert!(last.contains("Cash                                       $3.00"), "{last}");
    assert!(!last.contains("$1.00") && !last.contains("$5.00"), "{last}");
}

#[test]
fn a_line_moves_to_a_check_of_its_own_and_back() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let ticket = two_espressos(&mut runtime);
    let order = ticket.order;
    // A second line: another espresso, rung on its own.
    let ticket = runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    let (first, second) = (ticket.lines[0].line, ticket.lines[1].line);
    assert!(ticket.checks.is_empty(), "one check: none listed");

    let ticket = runtime.move_line(order, second, None).unwrap();
    assert_eq!(ticket.checks.len(), 2);
    assert_eq!(ticket.checks[0].ticket.lines.len(), 1);
    assert_eq!(ticket.checks[0].ticket.lines[0].line, first);
    assert_eq!(ticket.checks[1].ticket.lines[0].line, second);
    // Back to the first check: the second is empty, so none are listed again.
    let main = ticket.checks[0].check;
    let ticket = runtime.move_line(order, second, Some(main)).unwrap();
    assert!(ticket.checks.is_empty());
    // Where it is already is no move.
    assert!(matches!(runtime.move_line(order, second, Some(main)), Err(RuntimeError::Order(_))));
    // A split of one way, or of more than the most, is nothing to split.
    assert!(matches!(runtime.split_evenly(order, 1), Err(RuntimeError::NothingToSplit)));
    assert!(matches!(runtime.split_evenly(order, 21), Err(RuntimeError::NothingToSplit)));
    // The empty check is used by the next split, with none more opened than it takes.
    let ticket = runtime.split_evenly(order, 2).unwrap();
    assert_eq!(ticket.checks.len(), 2);
    let empty = runtime.start_order(Mode::Takeout).unwrap().order;
    assert!(matches!(runtime.split_evenly(empty, 2), Err(RuntimeError::NothingToSplit)));
}
