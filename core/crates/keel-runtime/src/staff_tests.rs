//! Known answers for staff in the runtime (ADR-0029): PINs and the lockout, approvals asked,
//! held, spent and refused, and the intents they gate.

use core::num::NonZeroU32;
use core::time::Duration;

use keel_domain::order::{Mode, Order};
use keel_domain::profile::{Profile, demo};
use keel_domain::schema::DomainEvent;
use keel_domain::staff::{Action, ApprovalEvent, Granted, Request};
use keel_events::envelope::{Actor, Device};

use crate::tests::{Scratch, TestRuntime, demo_with, id, register, usd};
use crate::{APPROVAL_LIFE, FIRST_LOCK, LONGEST_LOCK, PIN_TRIES, QUIET, RuntimeError};

/// Wrong PINs, `n` of them, for `member`.
fn wrong(runtime: &mut TestRuntime, member: u64, n: u32) {
    for _ in 0..n {
        assert!(matches!(runtime.sign_in(id(member), "0000"), Err(RuntimeError::WrongPin)));
    }
}

fn later(runtime: &TestRuntime, by: Duration) {
    runtime.clock().advance(by).unwrap();
}

fn locked(runtime: &mut TestRuntime) -> Option<u64> {
    match runtime.sign_in(id(0x502), "2468") {
        Err(RuntimeError::LockedOut { seconds }) => Some(seconds),
        Ok(()) => None,
        Err(other) => panic!("{other:?}"),
    }
}

#[test]
fn a_member_signs_in_with_their_pin_alone() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    assert!(matches!(runtime.sign_in(id(0x502), "1234"), Err(RuntimeError::WrongPin)));
    assert!(matches!(runtime.sign_in(id(0x502), ""), Err(RuntimeError::WrongPin)));
    assert_eq!(runtime.signed_in(), None);
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert_eq!(runtime.signed_in(), Some(id(0x502)));
    // A wrong PIN for another leaves the one signed in as they were.
    assert!(runtime.sign_in(id(0x501), "2468").is_err());
    assert_eq!(runtime.signed_in(), Some(id(0x502)));
    assert!(matches!(runtime.sign_in(id(0x5ff), "1234"), Err(RuntimeError::UnknownMember(_))));
}

#[test]
fn wrong_pins_lock_pin_entry_for_longer_each_time() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    wrong(&mut runtime, 0x502, PIN_TRIES);
    // Locked: even the right PIN waits, and so does an approval.
    assert_eq!(locked(&mut runtime), Some(30));
    later(&runtime, FIRST_LOCK.checked_sub(Duration::from_secs(1)).unwrap());
    assert!(matches!(
        runtime.sign_in(id(0x501), "1234"),
        Err(RuntimeError::LockedOut { seconds: 1 })
    ));
    later(&runtime, Duration::from_secs(1));
    // Five more wrong: twice as long.
    wrong(&mut runtime, 0x502, PIN_TRIES);
    assert_eq!(locked(&mut runtime), Some(60));
    later(&runtime, FIRST_LOCK * 2);
    // Many rounds: never longer than the longest lock.
    for _ in 0..8 {
        wrong(&mut runtime, 0x502, PIN_TRIES);
        let seconds = locked(&mut runtime).unwrap();
        assert!(seconds <= LONGEST_LOCK.as_secs());
        later(&runtime, Duration::from_secs(seconds));
    }
    wrong(&mut runtime, 0x502, PIN_TRIES);
    assert_eq!(locked(&mut runtime), Some(900));
    // A clock set back a day leaves at most the longest lock.
    runtime.clock().set("2026-10-01T13:00:00Z".parse().unwrap());
    assert_eq!(locked(&mut runtime), Some(900));
}

/// Found in review: any right PIN started the count again, so a cashier could guess at a
/// manager's PIN four times between each of their own sign-ins, and never be locked.
#[test]
fn a_right_pin_never_starts_the_count_again_but_quiet_does() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    wrong(&mut runtime, 0x501, PIN_TRIES - 1);
    runtime.sign_in(id(0x502), "2468").unwrap();
    wrong(&mut runtime, 0x501, 1);
    assert_eq!(locked(&mut runtime), Some(30), "the fifth wrong PIN locks, whoever's they were");
    later(&runtime, FIRST_LOCK);
    // Guessing through approvals counts the same.
    let order = cashiers_order(&mut runtime).id();
    assert!(runtime.abandon(order).is_err());
    for _ in 0..PIN_TRIES {
        assert!(matches!(runtime.approve(id(0x501), "0000"), Err(RuntimeError::WrongPin)));
    }
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::LockedOut { .. })));
    // Two more once the lock ends, ten wrong in all: three more would lock.
    later(&runtime, LONGEST_LOCK);
    wrong(&mut runtime, 0x501, 2);
    // Fifteen minutes with no wrong PIN start the count again: four wrong lock nothing.
    later(&runtime, QUIET + Duration::from_secs(1));
    wrong(&mut runtime, 0x501, PIN_TRIES - 1);
    assert_eq!(locked(&mut runtime), None);
}

#[test]
fn only_those_who_may_approve_are_approvers() {
    let scratch = Scratch::new();
    // Kim, another cashier, who may approve no void.
    let three = demo_with(|data| {
        let mut kim = data.team[1].clone();
        kim.id = id(0x503);
        kim.name = keel_domain::codec::Name::new("Kim").unwrap();
        data.team.push(kim);
    });
    let mut runtime = register(&scratch, three);
    let order = cashiers_order(&mut runtime).id();
    assert!(runtime.abandon(order).is_err());
    assert_eq!(runtime.approvers(), [id(0x501)]);
    assert!(matches!(runtime.approve(id(0x503), "2468"), Err(RuntimeError::NotApprover { .. })));
}

/// Sam, a cashier, signed in, and an order of theirs with an espresso on it.
fn cashiers_order(runtime: &mut TestRuntime) -> Order {
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.store().load(Order::new(order)).unwrap()
}

/// The approvals in the device's log, and how many events name each.
fn approvals(runtime: &TestRuntime) -> Vec<(Granted, usize)> {
    let log = runtime.store().log(id::<Device>(0xd1), 0, 1_000).unwrap();
    log.iter()
        .filter(|event| event.body().stream.kind.as_str() == keel_domain::staff::STREAM)
        .map(|event| {
            let body = event.body();
            let ApprovalEvent::Granted(granted) =
                ApprovalEvent::decode(&body.schema, &body.payload).unwrap();
            let naming =
                log.iter().filter(|other| other.body().approval == Some(body.event_id)).count();
            (granted, naming)
        })
        .collect()
}

#[test]
fn a_cashier_voids_an_order_once_a_manager_approves() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let order = cashiers_order(&mut runtime).id();
    // Nothing asked for, nothing to approve.
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NothingToApprove)));
    let refused = runtime.abandon(order);
    assert!(
        matches!(refused, Err(RuntimeError::ApprovalRequired(Action::OrderVoid))),
        "{refused:?}"
    );
    assert_eq!(runtime.approvers(), [id(0x501)], "a manager may approve it, and no one else");
    // Their own approval is no approval, and a wrong PIN approves nothing.
    assert!(matches!(
        runtime.approve(id(0x502), "2468"),
        Err(RuntimeError::NotApprover { action: Action::OrderVoid, .. })
    ));
    assert!(matches!(runtime.approve(id(0x501), "9999"), Err(RuntimeError::WrongPin)));
    assert_eq!(runtime.approval_waiting(), None);
    runtime.approve(id(0x501), "1234").unwrap();
    assert_eq!(runtime.approval_waiting(), Some(id(0x501)));
    let ticket = runtime.abandon(order).unwrap();
    assert_eq!(ticket.state, crate::TicketState::Abandoned);
    // Spent: in the log once, named by the line's removal and the abandonment.
    assert_eq!(runtime.approval_waiting(), None);
    let found = approvals(&runtime);
    assert_eq!(found.len(), 1);
    let (granted, naming) = &found[0];
    assert_eq!(granted.approver(), id(0x501));
    assert_eq!(granted.request(), Request::of(Action::OrderVoid));
    assert_eq!(granted.subject(), Some(order.cast()));
    assert_eq!(*naming, 2);
    // The approval's actor is the member who asked.
    let log = runtime.store().log(id::<Device>(0xd1), 0, 1_000).unwrap();
    let approval = log.iter().find(|e| e.body().stream.kind.as_str() == "approval").unwrap();
    assert_eq!(approval.body().actor, Actor::TeamMember(id(0x502)));
    // The next void needs another.
    let next = cashiers_order(&mut runtime).id();
    assert!(matches!(
        runtime.abandon(next),
        Err(RuntimeError::ApprovalRequired(Action::OrderVoid))
    ));
    // An order that never had a line is dropped by whoever began it.
    let empty = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.abandon(empty).unwrap();
    // A manager voids alone.
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime.abandon(next).unwrap();
    assert_eq!(approvals(&runtime).len(), 1, "no approval for what a role permits");
}

#[test]
fn an_approval_waits_a_minute_for_the_one_who_asked_and_survives_a_refusal() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let order = cashiers_order(&mut runtime).id();
    assert!(runtime.abandon(order).is_err());
    runtime.approve(id(0x501), "1234").unwrap();
    later(&runtime, APPROVAL_LIFE + Duration::from_millis(1));
    assert_eq!(runtime.approval_waiting(), None);
    assert!(matches!(runtime.abandon(order), Err(RuntimeError::ApprovalRequired(_))));
    // Signing in or out drops one, and what was asked.
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert_eq!(runtime.approval_waiting(), None);
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NothingToApprove)));
    assert!(runtime.abandon(order).is_err());
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.sign_out();
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NotSignedIn)));
    // An intent that fails for another reason leaves it waiting.
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert!(runtime.abandon(order).is_err());
    runtime.approve(id(0x501), "1234").unwrap();
    assert!(runtime.abandon(id(0xbad)).is_err());
    assert_eq!(runtime.approval_waiting(), Some(id(0x501)));
    runtime.abandon(order).unwrap();
    assert_eq!(approvals(&runtime).len(), 1);
}

/// Found in review: an approval covered whatever gated intent came next, as another order's.
#[test]
fn an_approval_is_for_the_request_asked_and_no_other() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let first = cashiers_order(&mut runtime).id();
    let second = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(second, id(0x401), &[], NonZeroU32::MIN).unwrap();
    assert!(runtime.abandon(first).is_err());
    runtime.approve(id(0x501), "1234").unwrap();
    // The approval is the first order's: the second's void needs its own.
    assert!(matches!(
        runtime.abandon(second),
        Err(RuntimeError::ApprovalRequired(Action::OrderVoid))
    ));
    assert!(approvals(&runtime).is_empty());
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.abandon(second).unwrap();
    let found = approvals(&runtime);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0.subject(), Some(second.cast()));
}

/// Found in review: a cashier took each line off, which their role permits, then dropped the
/// empty order, which needed no approval.
#[test]
fn an_order_emptied_line_by_line_is_still_voided() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let loaded = cashiers_order(&mut runtime);
    let order = loaded.id();
    runtime.remove_line(order, loaded.live_lines().next().unwrap().id()).unwrap();
    assert!(matches!(
        runtime.abandon(order),
        Err(RuntimeError::ApprovalRequired(Action::OrderVoid))
    ));
}

#[test]
fn removing_lowering_and_reprinting_are_gated_as_the_role_says() {
    let scratch = Scratch::new();
    // A cashier who may neither take a line off nor print a receipt again.
    let strict: Profile = demo_with(|data| {
        data.roles[1].permissions.retain(|permission| {
            !matches!(permission.action, Action::LineRemove | Action::ReceiptReprint)
        });
    });
    let mut runtime = register(&scratch, strict);
    let loaded = cashiers_order(&mut runtime);
    let (order, line) = (loaded.id(), loaded.live_lines().next().unwrap().id());
    assert!(matches!(
        runtime.remove_line(order, line),
        Err(RuntimeError::ApprovalRequired(Action::LineRemove))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.remove_line(order, line).unwrap();
    assert_eq!(approvals(&runtime)[0].0.request(), Request::of(Action::LineRemove));
    assert_eq!(approvals(&runtime)[0].1, 1);
    // Fewer of a line takes some of it off: four to two needs an approval, more needs none.
    // Found in review: a line of ten went to one with no approval.
    let three = runtime.add_item(order, id(0x401), &[], NonZeroU32::new(3).unwrap()).unwrap();
    let line = three.lines[0].line;
    runtime.change_quantity(order, line, NonZeroU32::new(4).unwrap()).unwrap();
    assert!(matches!(
        runtime.change_quantity(order, line, NonZeroU32::new(2).unwrap()),
        Err(RuntimeError::ApprovalRequired(Action::LineRemove))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.change_quantity(order, line, NonZeroU32::new(2).unwrap()).unwrap();
    assert_eq!(approvals(&runtime).len(), 2);
    // A sale, then its receipt again.
    runtime.pay_cash(order, usd(10_000)).unwrap();
    // No receipt, no approval asked for. Found in review: one was, and then covered a void.
    let empty = runtime.start_order(Mode::Takeout).unwrap().order;
    assert!(matches!(runtime.reprint(empty), Err(RuntimeError::NoReceipt(_))));
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NothingToApprove)));
    assert!(matches!(
        runtime.reprint(order),
        Err(RuntimeError::ApprovalRequired(Action::ReceiptReprint))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.reprint(order).unwrap();
    let found = approvals(&runtime);
    assert_eq!(found.len(), 3);
    assert_eq!(found[2].0.request(), Request::of(Action::ReceiptReprint));
    assert_eq!(found[2].1, 0, "a reprint writes no event but its approval");
    // In the demo, a cashier does all of it alone.
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let loaded = cashiers_order(&mut runtime);
    runtime.remove_line(loaded.id(), loaded.live_lines().next().unwrap().id()).unwrap();
    assert!(approvals(&runtime).is_empty());
}

#[test]
fn a_profile_without_roles_lets_anyone_signed_in_do_anything_with_no_pin() {
    let scratch = Scratch::new();
    let open = demo_with(|data| {
        data.roles.clear();
        for member in &mut data.team {
            member.role = None;
            member.pin = None;
        }
    });
    let mut runtime = register(&scratch, open);
    assert!(matches!(runtime.sign_in(id(0x502), "2468"), Err(RuntimeError::WrongPin)));
    runtime.sign_in(id(0x502), "").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.abandon(order).unwrap();
    assert!(approvals(&runtime).is_empty());
}
