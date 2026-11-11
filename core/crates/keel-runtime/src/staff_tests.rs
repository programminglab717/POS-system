//! Known answers for staff in the runtime (ADR-0029): PINs and the lockout, approvals held,
//! spent and refused, and the intents they gate.

use core::num::NonZeroU32;
use core::time::Duration;

use keel_domain::order::Mode;
use keel_domain::profile::{Profile, demo};
use keel_domain::schema::DomainEvent;
use keel_domain::staff::{Action, ApprovalEvent, Request};
use keel_events::envelope::Device;

use crate::tests::{Scratch, TestRuntime, demo_with, id, register, usd};
use crate::{APPROVAL_LIFE, FIRST_LOCK, LONGEST_LOCK, PIN_TRIES, RuntimeError};

/// Five wrong PINs: the lock's first round.
fn five_wrong(runtime: &mut TestRuntime) {
    for _ in 0..PIN_TRIES {
        assert!(matches!(runtime.sign_in(id(0x502), "0000"), Err(RuntimeError::WrongPin)));
    }
}

fn later(runtime: &TestRuntime, by: Duration) {
    runtime.clock().advance(by).unwrap();
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
    five_wrong(&mut runtime);
    // Locked: even the right PIN waits.
    assert!(matches!(
        runtime.sign_in(id(0x502), "2468"),
        Err(RuntimeError::LockedOut { seconds: 30 })
    ));
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NotSignedIn)));
    later(&runtime, FIRST_LOCK.checked_sub(Duration::from_secs(1)).unwrap());
    assert!(matches!(
        runtime.sign_in(id(0x501), "1234"),
        Err(RuntimeError::LockedOut { seconds: 1 })
    ));
    later(&runtime, Duration::from_secs(1));
    // Five more wrong: twice as long.
    five_wrong(&mut runtime);
    assert!(matches!(
        runtime.sign_in(id(0x502), "2468"),
        Err(RuntimeError::LockedOut { seconds: 60 })
    ));
    later(&runtime, FIRST_LOCK * 2);
    // A right PIN starts the count again: one wrong after it locks nothing.
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert!(matches!(runtime.sign_in(id(0x502), "0000"), Err(RuntimeError::WrongPin)));
    runtime.sign_in(id(0x502), "2468").unwrap();
    // Many rounds: never longer than the longest lock.
    for _ in 0..8 {
        five_wrong(&mut runtime);
        let Err(RuntimeError::LockedOut { seconds }) = runtime.sign_in(id(0x502), "2468") else {
            panic!("locked");
        };
        assert!(seconds <= LONGEST_LOCK.as_secs());
        later(&runtime, Duration::from_secs(seconds));
    }
    five_wrong(&mut runtime);
    assert!(matches!(
        runtime.sign_in(id(0x502), "2468"),
        Err(RuntimeError::LockedOut { seconds: 900 })
    ));
    // A clock set back a day leaves at most the longest lock.
    runtime.clock().set("2026-10-01T13:00:00Z".parse().unwrap());
    assert!(matches!(
        runtime.sign_in(id(0x502), "2468"),
        Err(RuntimeError::LockedOut { seconds: 900 })
    ));
}

/// A cashier's order with a line on it, `Sam` signed in.
fn sams_order(runtime: &mut TestRuntime) -> keel_domain::order::Order {
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.store().load(keel_domain::order::Order::new(order)).unwrap()
}

/// The approvals in the device's log, and the events that name each.
fn approvals(runtime: &TestRuntime) -> Vec<(keel_domain::staff::Granted, usize)> {
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
    let order = sams_order(&mut runtime).id();
    let refused = runtime.abandon(order);
    assert!(
        matches!(refused, Err(RuntimeError::ApprovalRequired(Action::OrderVoid))),
        "{refused:?}"
    );
    // Their own approval is no approval.
    runtime.approve(id(0x502), "2468").unwrap();
    assert!(matches!(
        runtime.abandon(order),
        Err(RuntimeError::NotApprover { action: Action::OrderVoid, .. })
    ));
    // A wrong PIN approves nothing.
    assert!(matches!(runtime.approve(id(0x501), "9999"), Err(RuntimeError::WrongPin)));
    runtime.approve(id(0x501), "1234").unwrap();
    assert_eq!(runtime.approval_waiting(), Some(id(0x501)));
    let ticket = runtime.abandon(order).unwrap();
    assert_eq!(ticket.state, crate::TicketState::Abandoned);
    // Spent: in the log, named by the line's removal and the abandonment.
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
    assert_eq!(approval.body().actor, keel_events::envelope::Actor::TeamMember(id(0x502)));
    // The next void needs another.
    let next = sams_order(&mut runtime).id();
    assert!(matches!(
        runtime.abandon(next),
        Err(RuntimeError::ApprovalRequired(Action::OrderVoid))
    ));
    // An empty order is dropped by whoever began it.
    let empty = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.abandon(empty).unwrap();
    assert_eq!(approvals(&runtime).len(), 1);
    // A manager voids alone.
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime.abandon(next).unwrap();
    assert_eq!(approvals(&runtime).len(), 1, "no approval for what a role permits");
}

#[test]
fn an_approval_waits_a_minute_for_the_one_who_asked_and_survives_a_refusal() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let order = sams_order(&mut runtime).id();
    runtime.approve(id(0x501), "1234").unwrap();
    later(&runtime, APPROVAL_LIFE + Duration::from_millis(1));
    assert_eq!(runtime.approval_waiting(), None);
    assert!(matches!(runtime.abandon(order), Err(RuntimeError::ApprovalRequired(_))));
    // Signing in or out drops one: it was for whoever asked.
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert_eq!(runtime.approval_waiting(), None);
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.sign_out();
    assert!(matches!(runtime.approve(id(0x501), "1234"), Err(RuntimeError::NotSignedIn)));
    // An intent that fails for another reason leaves it waiting.
    runtime.sign_in(id(0x502), "2468").unwrap();
    runtime.approve(id(0x501), "1234").unwrap();
    let unknown = id(0xbad);
    assert!(runtime.abandon(unknown).is_err());
    assert_eq!(runtime.approval_waiting(), Some(id(0x501)));
    runtime.abandon(order).unwrap();
    assert_eq!(approvals(&runtime).len(), 1);
}

#[test]
fn removing_a_line_and_reprinting_are_gated_as_the_role_says() {
    let scratch = Scratch::new();
    // A cashier who may neither take a line off nor print a receipt again.
    let strict: Profile = demo_with(|data| {
        data.roles[1].permissions.retain(|permission| {
            !matches!(permission.action, Action::LineRemove | Action::ReceiptReprint)
        });
    });
    let mut runtime = register(&scratch, strict);
    let loaded = sams_order(&mut runtime);
    let (order, line) = (loaded.id(), loaded.live_lines().next().unwrap().id());
    assert!(matches!(
        runtime.remove_line(order, line),
        Err(RuntimeError::ApprovalRequired(Action::LineRemove))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.remove_line(order, line).unwrap();
    assert_eq!(approvals(&runtime)[0].0.request(), Request::of(Action::LineRemove));
    assert_eq!(approvals(&runtime)[0].1, 1);
    // A sale, then its receipt again.
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.pay_cash(order, usd(1_000)).unwrap();
    assert!(matches!(
        runtime.reprint(order),
        Err(RuntimeError::ApprovalRequired(Action::ReceiptReprint))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.reprint(order).unwrap();
    let found = approvals(&runtime);
    assert_eq!(found.len(), 2);
    assert_eq!(found[1].0.request(), Request::of(Action::ReceiptReprint));
    assert_eq!(found[1].1, 0, "a reprint writes no event but its approval");
    // In the demo, a cashier does both alone.
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    let loaded = sams_order(&mut runtime);
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
