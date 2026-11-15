//! Known answers for card payments at the register (ADR-0035): a sale through the terminal, with
//! a tip on the pre-tax subtotal; approved, approved in part, declined, or unknown and asked
//! about, never sent twice; settled by hand on a manager's word; and refunded to the card.

use core::num::NonZeroU32;
use core::time::Duration;

use keel_doc::escpos::plain_text;
use keel_doc::{Columns, Doc};
use keel_domain::codec::{AuthCode, LastFour, Name, ProcessorRef, ReasonCode};
use keel_domain::order::{Mode, Order, Reason};
use keel_domain::payment::{Card, Entry, Payment, PaymentStatus, Tender};
use keel_domain::profile::{TipPrompt, demo};
use keel_domain::staff::Action;
use keel_types::{Id, Money, Quantity, Unit};

use crate::tests::{Scratch, TestRuntime, demo_with, id, register, register_again, usd};
use crate::{
    CardAsk, CardOutcome, CardRequest, CardState, CardTip, JobKind, RefundTo, ReportKind,
    RuntimeError,
};

fn card() -> Card {
    Card {
        brand: Name::new("Visa").unwrap(),
        last_four: LastFour::new("4242").unwrap(),
        entry: Entry::Tap,
        auth_code: Some(AuthCode::new("A1B2C3").unwrap()),
    }
}

fn approved(cents: i64) -> CardOutcome {
    CardOutcome::Approved {
        approved: usd(cents),
        reference: Some(ProcessorRef::new("pi_3Keel01").unwrap()),
        card: Some(card()),
    }
}

fn declined(code: &str) -> CardOutcome {
    CardOutcome::Declined { reason: ReasonCode::new(code).unwrap(), note: None, reference: None }
}

/// An espresso: $3.25, and $0.29 of tax at 8.875%, $3.54. The order.
fn an_espresso(runtime: &mut TestRuntime) -> Id<Order> {
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    order
}

fn manager(scratch: &Scratch) -> TestRuntime {
    let mut runtime = register(scratch, demo().unwrap());
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime
}

/// The payment of the order's only card payment.
fn the_payment(runtime: &TestRuntime, order: Id<Order>) -> Id<Payment> {
    runtime.card_payments(order).unwrap().last().unwrap().payment
}

/// The receipts queued, as plain text.
fn receipts(runtime: &TestRuntime) -> Vec<String> {
    runtime
        .print_jobs()
        .unwrap()
        .into_iter()
        .filter(|job| job.kind == JobKind::Receipt)
        .map(|job| {
            let queued = runtime.store().effect(&job.job.to_bytes()).unwrap().unwrap();
            plain_text(&Doc::decode(&queued.effect.payload).unwrap(), Columns::new(42).unwrap())
        })
        .collect()
}

fn sale(request: &CardRequest) -> (Money, Option<Money>, Vec<u16>, bool) {
    match &request.ask {
        CardAsk::Sale { amount, tip, presets, partial, .. } => {
            (*amount, *tip, presets.clone(), *partial)
        }
        CardAsk::Refund { .. } => panic!("a sale was asked for"),
    }
}

#[test]
fn a_card_sale_with_a_tip_on_the_screen_is_approved_and_closes_the_order() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    // The demo tips 10%, 15% and 20% of the pre-tax subtotal, $3.25: half up, $0.33, $0.49, $0.65.
    let check = order.cast();
    let offers = runtime.tip_offers(order, check, None).unwrap();
    assert_eq!(offers.prompt, Some(TipPrompt::Screen));
    assert_eq!(offers.base.money, usd(325));
    let presets: Vec<(u16, Money)> =
        offers.presets.iter().map(|offer| (offer.percent, offer.amount.money)).collect();
    assert_eq!(presets, [(1_000, usd(33)), (1_500, usd(49)), (2_000, usd(65))]);
    // The payment waits for the terminal: nothing is paid yet, and the order is open.
    let ticket = runtime.pay_card(order, CardTip::Amount(usd(49)), true).unwrap();
    assert_eq!((ticket.paid.money, ticket.due.money), (usd(0), usd(354)));
    assert_eq!(runtime.card_payments(order).unwrap()[0].state, CardState::Waiting);
    // The check takes no other payment meanwhile.
    assert!(runtime.pay_cash(order, usd(400)).is_err());
    let request = runtime.next_card_request().unwrap().unwrap();
    assert!(!request.status);
    assert_eq!(sale(&request), (usd(354), Some(usd(49)), vec![], true));
    assert_eq!(request.key, the_payment(&runtime, order).to_string());
    // One request at a time.
    assert_eq!(runtime.next_card_request().unwrap(), None);
    runtime.card_outcome(&request.key, approved(403)).unwrap();
    let ticket = runtime.ticket(order).unwrap();
    assert_eq!((ticket.paid.money, ticket.due.money), (usd(354), usd(0)));
    assert_eq!(ticket.state, crate::TicketState::Closed);
    let view = &runtime.card_payments(order).unwrap()[0];
    assert_eq!(view.card.as_deref(), Some("Visa •••• 4242"));
    let CardState::Approved { applied, tip, by_hand } = &view.state else {
        panic!("approved: {:?}", view.state)
    };
    assert_eq!(
        (applied.money, tip.as_ref().map(|tip| tip.money), *by_hand),
        (usd(354), Some(usd(49)), false)
    );
    // The capture tells of the card, and the receipt shows it, its tip and what it charged.
    let payment = runtime.store().load(Payment::new(view.payment)).unwrap();
    let PaymentStatus::Captured(captured) = payment.status() else { panic!("captured") };
    assert_eq!(captured.card, Some(card()));
    let receipt = receipts(&runtime).pop().unwrap();
    assert!(receipt.contains("Visa •••• 4242 (Tap)"), "{receipt}");
    assert!(receipt.contains("Tip") && receipt.contains("$0.49"), "{receipt}");
    assert!(receipt.contains("Charged") && receipt.contains("$4.03"), "{receipt}");
    assert!(receipt.contains("Auth A1B2C3"), "{receipt}");
    // The outcome is recorded once: the request is no longer with the shell.
    assert!(matches!(
        runtime.card_outcome(&request.key, approved(403)),
        Err(RuntimeError::UnknownCardRequest)
    ));
    assert_eq!(runtime.next_card_request().unwrap(), None);
    // The tip is the manager's, who took the payment.
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    assert_eq!(report.tips.len(), 1);
    assert_eq!(report.tips[0].name.as_ref().map(Name::as_str), Some("Alex"));
    assert_eq!((report.tips[0].payments, report.tips[0].amount.money), (1, usd(49)));
}

#[test]
fn a_partial_approval_pays_the_check_first_and_leaves_the_rest_for_cash() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::Amount(usd(49)), true).unwrap();
    let request = runtime.next_card_request().unwrap().unwrap();
    // $2.00 of $4.03: it pays the check, and drops the tip.
    runtime.card_outcome(&request.key, approved(200)).unwrap();
    let ticket = runtime.ticket(order).unwrap();
    assert_eq!((ticket.paid.money, ticket.due.money), (usd(200), usd(154)));
    let CardState::Approved { applied, tip, .. } = &runtime.card_payments(order).unwrap()[0].state
    else {
        panic!("approved")
    };
    assert_eq!((applied.money, tip), (usd(200), &None));
    // The rest in cash, rounded as the location rounds cash.
    let paid = runtime.pay_cash(order, usd(200)).unwrap();
    assert_eq!(paid.ticket.state, crate::TicketState::Closed);
}

#[test]
fn a_decline_fails_the_payment_and_the_check_can_be_paid_again() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::None, false).unwrap();
    let request = runtime.next_card_request().unwrap().unwrap();
    assert_eq!(sale(&request), (usd(354), None, vec![], false));
    runtime.card_outcome(&request.key, declined("insufficient_funds")).unwrap();
    assert_eq!(
        runtime.card_payments(order).unwrap()[0].state,
        CardState::Declined { reason: "insufficient_funds".to_owned() }
    );
    assert_eq!(runtime.ticket(order).unwrap().due.money, usd(354));
    runtime.pay_card(order, CardTip::None, false).unwrap();
    let again = runtime.next_card_request().unwrap().unwrap();
    assert_ne!(again.key, request.key, "a new payment, with a key of its own");
    runtime.card_outcome(&again.key, approved(354)).unwrap();
    assert_eq!(runtime.ticket(order).unwrap().state, crate::TicketState::Closed);
}

#[test]
fn an_unknown_outcome_blocks_the_check_and_is_asked_about_never_sent_again() {
    let scratch = Scratch::new();
    let order = {
        let mut runtime = manager(&scratch);
        let order = an_espresso(&mut runtime);
        runtime.pay_card(order, CardTip::None, true).unwrap();
        let request = runtime.next_card_request().unwrap().unwrap();
        runtime.card_outcome(&request.key, CardOutcome::Unknown { retry_after: None }).unwrap();
        assert_eq!(runtime.card_payments(order).unwrap()[0].state, CardState::Checking);
        // Not asked about for 5 s; the check takes nothing else meanwhile.
        assert_eq!(runtime.next_card_request().unwrap(), None);
        assert!(runtime.pay_cash(order, usd(400)).is_err());
        assert!(runtime.pay_card(order, CardTip::None, true).is_err());
        runtime.clock().advance(Duration::from_secs(5)).unwrap();
        let asked = runtime.next_card_request().unwrap().unwrap();
        assert!(asked.status, "asked about, not sent again");
        assert_eq!(asked.key, request.key);
        runtime.card_outcome(&asked.key, CardOutcome::Unknown { retry_after: None }).unwrap();
        // The next question waits twice as long.
        runtime.clock().advance(Duration::from_secs(9)).unwrap();
        assert_eq!(runtime.next_card_request().unwrap(), None);
        runtime.clock().advance(Duration::from_secs(1)).unwrap();
        // The register closes with the question with the shell: in doubt.
        assert!(runtime.next_card_request().unwrap().unwrap().status);
        order
    };
    let mut runtime = register_again(&scratch, "2026-10-02T13:05:00Z", 11);
    runtime.sign_in(id(0x501), "1234").unwrap();
    let asked = runtime.next_card_request().unwrap().unwrap();
    assert!(asked.status, "a request in doubt is asked about");
    runtime.card_outcome(&asked.key, CardOutcome::NotFound).unwrap();
    assert_eq!(
        runtime.card_payments(order).unwrap()[0].state,
        CardState::Declined { reason: "not_received".to_owned() }
    );
    runtime.pay_cash(order, usd(400)).unwrap();
}

#[test]
fn a_sale_in_doubt_when_the_register_opens_is_asked_about() {
    let scratch = Scratch::new();
    let order = {
        let mut runtime = manager(&scratch);
        let order = an_espresso(&mut runtime);
        runtime.pay_card(order, CardTip::None, true).unwrap();
        let request = runtime.next_card_request().unwrap().unwrap();
        assert!(!request.status);
        order
    };
    let mut runtime = register_again(&scratch, "2026-10-02T13:05:00Z", 11);
    runtime.sign_in(id(0x501), "1234").unwrap();
    let asked = runtime.next_card_request().unwrap().unwrap();
    assert!(asked.status);
    // The terminal had charged it: it is captured, once.
    runtime.card_outcome(&asked.key, approved(354)).unwrap();
    assert_eq!(runtime.ticket(order).unwrap().state, crate::TicketState::Closed);
}

#[test]
fn tips_are_taken_as_the_location_takes_them() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    // The demo asks on the screen: not on the terminal.
    assert!(matches!(runtime.pay_card(order, CardTip::Terminal, true), Err(RuntimeError::NoTips)));
    // A tip of nothing, or more than what it tips on, is refused.
    assert!(matches!(
        runtime.pay_card(order, CardTip::Amount(usd(0)), true),
        Err(RuntimeError::TipTooLarge)
    ));
    assert!(matches!(
        runtime.pay_card(order, CardTip::Amount(usd(355)), true),
        Err(RuntimeError::TipTooLarge)
    ));
    runtime.pay_card(order, CardTip::Amount(usd(354)), true).unwrap();
    // With no tipping, there are no tips.
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo_with(|data| data.tipping = None));
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = an_espresso(&mut runtime);
    let offers = runtime.tip_offers(order, order.cast(), None).unwrap();
    assert_eq!((offers.prompt, offers.presets.len()), (None, 0));
    assert!(matches!(
        runtime.pay_card(order, CardTip::Amount(usd(50)), true),
        Err(RuntimeError::NoTips)
    ));
    // Asked on the terminal, the request carries the presets, and the tip is what it approved
    // beyond the check.
    let scratch = Scratch::new();
    let mut runtime = register(
        &scratch,
        demo_with(|data| data.tipping.as_mut().unwrap().prompt = TipPrompt::Terminal),
    );
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::Terminal, true).unwrap();
    let request = runtime.next_card_request().unwrap().unwrap();
    assert_eq!(sale(&request), (usd(354), None, vec![1_000, 1_500, 2_000], true));
    runtime.card_outcome(&request.key, approved(420)).unwrap();
    let CardState::Approved { tip, .. } = &runtime.card_payments(order).unwrap()[0].state else {
        panic!("approved")
    };
    assert_eq!(tip.as_ref().map(|tip| tip.money), Some(usd(66)));
}

#[test]
fn a_manager_settles_by_hand_a_payment_no_terminal_answers_for() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::Amount(usd(33)), true).unwrap();
    let payment = the_payment(&runtime, order);
    let request = runtime.next_card_request().unwrap().unwrap();
    // While the terminal has it, its answer comes first.
    assert!(matches!(runtime.settle_card(payment, true), Err(RuntimeError::CardBusy)));
    runtime.card_outcome(&request.key, CardOutcome::Unknown { retry_after: None }).unwrap();
    // A cashier's role doesn't settle it: a manager approves.
    assert!(matches!(
        runtime.settle_card(payment, true),
        Err(RuntimeError::ApprovalRequired(Action::PaymentResolve))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    let ticket = runtime.settle_card(payment, true).unwrap();
    assert_eq!(ticket.state, crate::TicketState::Closed);
    let CardState::Approved { applied, tip, by_hand } =
        &runtime.card_payments(order).unwrap()[0].state
    else {
        panic!("approved")
    };
    assert_eq!(
        (applied.money, tip.as_ref().map(|tip| tip.money), *by_hand),
        (usd(354), Some(usd(33)), true)
    );
    assert!(receipts(&runtime).pop().unwrap().contains("Settled by hand"));
    // Nothing asks about it again, and it can't be settled twice.
    runtime.clock().advance(Duration::from_secs(60)).unwrap();
    assert_eq!(runtime.next_card_request().unwrap(), None);
    assert!(matches!(runtime.settle_card(payment, false), Err(RuntimeError::NotCardPayment)));
}

#[test]
fn a_card_paid_order_is_refunded_to_the_card_and_a_failed_refund_tried_again() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::Amount(usd(49)), true).unwrap();
    let request = runtime.next_card_request().unwrap().unwrap();
    runtime.card_outcome(&request.key, approved(403)).unwrap();
    let payment = the_payment(&runtime, order);
    let line = runtime.ticket(order).unwrap().lines[0].line;
    let returnable = runtime.returnable(order).unwrap();
    assert!(!returnable.cash_only);
    assert_eq!(returnable.cash_left.as_ref().map(|left| left.money), Some(usd(0)));
    assert_eq!(returnable.cards.len(), 1);
    // What it applied, its tip left out.
    assert_eq!(returnable.cards[0].left.money, usd(354));
    assert_eq!(returnable.cards[0].card.as_deref(), Some("Visa •••• 4242"));
    let reason = Reason { code: ReasonCode::new("damaged").unwrap(), note: None };
    let one = Quantity::from_whole(1, Unit::Each).unwrap();
    // No cash was taken, so none is handed back; the cash refund of old is refused.
    assert!(matches!(
        runtime.return_lines_to(order, &[(line, one)], reason.clone(), RefundTo::Cash),
        Err(RuntimeError::RefundTooLarge { left }) if left == usd(0)
    ));
    assert!(matches!(
        runtime.return_lines(order, &[(line, one)], reason.clone()),
        Err(RuntimeError::CardRefund)
    ));
    let refunded = runtime
        .return_lines_to(order, &[(line, one)], reason.clone(), RefundTo::Card(payment))
        .unwrap();
    assert_eq!((refunded.amount.money, refunded.handed.money), (usd(354), usd(0)));
    let before = receipts(&runtime).len();
    let request = runtime.next_card_request().unwrap().unwrap();
    let CardAsk::Refund { refund, attempt, amount, reference } = &request.ask else {
        panic!("a refund")
    };
    assert_eq!(
        (*refund, *attempt, *amount, reference.as_str()),
        (refunded.refund, 1, usd(354), "pi_3Keel01")
    );
    runtime.card_outcome(&request.key, declined("expired_card")).unwrap();
    let owed = runtime.owed_card_refunds().unwrap();
    assert_eq!(owed.len(), 1);
    assert_eq!(
        (owed[0].failures, owed[0].failure.as_deref(), owed[0].pending),
        (1, Some("expired_card"), false)
    );
    assert_eq!(receipts(&runtime).len(), before, "no receipt until it is paid back");
    // Tried again: a new attempt, with a key of its own.
    runtime.retry_refund(refunded.refund).unwrap();
    let again = runtime.next_card_request().unwrap().unwrap();
    assert_eq!(again.key, format!("{}-2", refunded.refund));
    runtime.card_outcome(&again.key, approved(354)).unwrap();
    assert!(runtime.owed_card_refunds().unwrap().is_empty());
    let receipt = receipts(&runtime).pop().unwrap();
    assert!(receipt.contains("REFUND") && receipt.contains("Visa •••• 4242"), "{receipt}");
    // Nothing is left to refund to it.
    assert_eq!(runtime.returnable(order).unwrap().cards[0].left.money, usd(0));
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    assert!(
        report
            .refunds
            .iter()
            .any(|line| line.tender == Tender::Card && line.amount.money == usd(354))
    );
    assert_eq!(report.card_refunds_owed, 0);
}

/// What the card approved goes to the kitchen in the same write, as cash does (ADR-0034): before
/// the terminal answers, nothing is sent.
#[test]
fn an_approved_card_sale_sends_its_lines_to_the_kitchen() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::None, true).unwrap();
    assert!(!runtime.ticket(order).unwrap().lines[0].sent, "nothing is sent while it waits");
    let request = runtime.next_card_request().unwrap().unwrap();
    runtime.card_outcome(&request.key, approved(354)).unwrap();
    assert!(runtime.ticket(order).unwrap().lines[0].sent);
}

/// An order paid part by card and part in cash: each tender is refunded no more than it took,
/// and a refund to the card hands back no cash (ADR-0035).
#[test]
fn a_return_goes_back_to_each_tender_no_more_than_it_took() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    runtime.open_drawer(usd(10_000)).unwrap();
    // Two espressos: $6.50 and $0.58 of tax, $7.08; $4.00 by card, the rest in cash.
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::new(2).unwrap()).unwrap();
    let check = order.cast();
    runtime.pay_check_card(order, check, Some(usd(400)), CardTip::None, true).unwrap();
    let request = runtime.next_card_request().unwrap().unwrap();
    runtime.card_outcome(&request.key, approved(400)).unwrap();
    runtime.pay_cash(order, usd(500)).unwrap();
    let payment = runtime.card_payments(order).unwrap()[0].payment;
    let line = runtime.ticket(order).unwrap().lines[0].line;
    let reason = Reason { code: ReasonCode::new("damaged").unwrap(), note: None };
    let one = Quantity::from_whole(1, Unit::Each).unwrap();
    let first = runtime
        .return_lines_to(order, &[(line, one)], reason.clone(), RefundTo::Card(payment))
        .unwrap();
    assert_eq!(
        (first.amount.money, first.handed.money),
        (usd(354), usd(0)),
        "no cash for a card refund"
    );
    // $0.46 is left of the card: the other espresso doesn't fit.
    assert!(matches!(
        runtime.return_lines_to(order, &[(line, one)], reason.clone(), RefundTo::Card(payment)),
        Err(RuntimeError::RefundTooLarge { left }) if left == usd(46)
    ));
    // Nor in cash: the order took $3.08 in cash.
    assert!(matches!(
        runtime.return_lines_to(order, &[(line, one)], reason, RefundTo::Cash),
        Err(RuntimeError::RefundTooLarge { left }) if left == usd(308)
    ));
}

/// A member's tips are added up: two tipped payments, one line (ADR-0035).
#[test]
fn a_members_tips_are_added_up() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    for tip in [33, 49] {
        let order = an_espresso(&mut runtime);
        runtime.pay_card(order, CardTip::Amount(usd(tip)), true).unwrap();
        let request = runtime.next_card_request().unwrap().unwrap();
        runtime.card_outcome(&request.key, approved(354 + tip)).unwrap();
    }
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    assert_eq!(report.tips.len(), 1);
    assert_eq!((report.tips[0].payments, report.tips[0].amount.money), (2, usd(82)));
}

/// An answer that can't be recorded, or a shell that stops answering, holds up nothing: the
/// request is asked about later, never sent again. Found in review.
#[test]
fn an_answer_that_cant_be_recorded_is_asked_about_later() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::None, true).unwrap();
    let request = runtime.next_card_request().unwrap().unwrap();
    assert!(matches!(
        runtime.card_outcome(&request.key, approved(0)),
        Err(RuntimeError::BadOutcome)
    ));
    assert_eq!(runtime.next_card_request().unwrap(), None, "asked about after a wait");
    runtime.clock().advance(Duration::from_secs(5)).unwrap();
    let asked = runtime.next_card_request().unwrap().unwrap();
    assert!(asked.status);
    // The shell gives it back unanswered: asked about again, later.
    runtime.release_card_request().unwrap();
    runtime.clock().advance(Duration::from_secs(5)).unwrap();
    let again = runtime.next_card_request().unwrap().unwrap();
    assert!(again.status);
    runtime.card_outcome(&again.key, approved(354)).unwrap();
    assert_eq!(runtime.ticket(order).unwrap().state, crate::TicketState::Closed);
}

/// A card settled by hand has no reference: its order isn't refunded at the register, neither to
/// the card nor in cash. Found in review: it passed for cash, and a return refunded nothing.
#[test]
fn an_order_a_card_settled_by_hand_paid_isnt_refunded_at_the_register() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::None, true).unwrap();
    let payment = the_payment(&runtime, order);
    let request = runtime.next_card_request().unwrap().unwrap();
    runtime.card_outcome(&request.key, CardOutcome::Unknown { retry_after: None }).unwrap();
    runtime.settle_card(payment, true).unwrap();
    let returnable = runtime.returnable(order).unwrap();
    assert!(!returnable.cash_only);
    assert!(returnable.cards.is_empty());
    assert_eq!(returnable.cash_left, None);
    let line = runtime.ticket(order).unwrap().lines[0].line;
    let reason = Reason { code: ReasonCode::new("damaged").unwrap(), note: None };
    let one = Quantity::from_whole(1, Unit::Each).unwrap();
    assert!(matches!(
        runtime.return_lines(order, &[(line, one)], reason),
        Err(RuntimeError::CardRefund)
    ));
}

/// A card refund the terminal approves for less than asked completes for what moved, and the
/// day's report counts the rest as owed (ADR-0035).
#[test]
fn a_card_refund_approved_short_leaves_the_rest_owed_in_the_report() {
    let scratch = Scratch::new();
    let mut runtime = manager(&scratch);
    let order = an_espresso(&mut runtime);
    runtime.pay_card(order, CardTip::None, true).unwrap();
    let request = runtime.next_card_request().unwrap().unwrap();
    runtime.card_outcome(&request.key, approved(354)).unwrap();
    let payment = the_payment(&runtime, order);
    let line = runtime.ticket(order).unwrap().lines[0].line;
    let reason = Reason { code: ReasonCode::new("damaged").unwrap(), note: None };
    let one = Quantity::from_whole(1, Unit::Each).unwrap();
    runtime.return_lines_to(order, &[(line, one)], reason, RefundTo::Card(payment)).unwrap();
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    assert_eq!((report.card_refunds_owed, report.card_refunds_owed_amount.money), (1, usd(354)));
    let request = runtime.next_card_request().unwrap().unwrap();
    runtime.card_outcome(&request.key, approved(300)).unwrap();
    let report = runtime.day_report(runtime.today().unwrap(), ReportKind::X).unwrap();
    assert_eq!((report.card_refunds_owed, report.card_refunds_owed_amount.money), (1, usd(54)));
}
