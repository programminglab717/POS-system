//! Known answers for availability at the register (ADR-0033): an item 86'd until the business day
//! ends, a time, or it is put back; put back; the menu and the modifier sheet showing it; ringing
//! refused while it is sold out; and the change gated as the member's role says.

use core::num::{NonZeroU8, NonZeroU32};
use core::time::Duration;

use keel_domain::availability::AvailabilityError;
use keel_domain::codec::ReasonCode;
use keel_domain::order::{Mode, Reason};
use keel_domain::profile::{Choice, demo};
use keel_domain::staff::Action;
use keel_types::{BusinessDate, Clock, LocalTime, Timestamp};

use crate::tests::{Scratch, TestRuntime, big_latte, demo_with, id, register};
use crate::{ItemRef, RuntimeError, SoldOut, SoldOutUntil, Until};

const ESPRESSO: u64 = 0x401;
const OAT_MILK: u64 = 0x202;

fn espresso() -> ItemRef {
    ItemRef::Variant(id(ESPRESSO))
}

fn oat_milk() -> ItemRef {
    ItemRef::Modifier(id(OAT_MILK))
}

/// A cashier signed in at the demo café's register, at 9 am on 2 October 2026.
fn cashier(scratch: &Scratch) -> TestRuntime {
    let mut runtime = register(scratch, demo().unwrap());
    runtime.sign_in(id(0x502), "2468").unwrap();
    runtime
}

/// 4 am on 3 October, when the business day of 2 October ends.
fn end_of_day() -> LocalTime {
    LocalTime { date: BusinessDate::new(2026, 10, 3).unwrap(), hour: 4, minute: 0 }
}

/// What ringing `variant` once, with `choices`, does, on a new order.
fn ring(runtime: &mut TestRuntime, variant: u64, choices: &[Choice]) -> Result<(), RuntimeError> {
    let order = runtime.start_order(Mode::Takeout)?.order;
    runtime.add_item(order, id(variant), choices, NonZeroU32::MIN).map(|_| ())
}

/// Whether the menu shows `variant` sold out, and until when.
fn on_menu(runtime: &TestRuntime, variant: u64) -> Option<SoldOutUntil> {
    let menu = runtime.menu().unwrap();
    let mut buttons = menu.pages.iter().flat_map(|page| page.buttons.iter());
    buttons.find(|button| button.variant == id(variant)).unwrap().sold_out
}

#[test]
fn an_86_lasts_the_business_day_and_ringing_is_refused_until_it_ends() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch);
    assert!(runtime.sold_out().unwrap().is_empty());
    assert_eq!(runtime.availability_version().unwrap(), 0);
    assert_eq!(on_menu(&runtime, ESPRESSO), None);
    // A cashier's role 86s: no approval.
    runtime.eighty_six(espresso(), Until::EndOfDay, None).unwrap();
    let name = keel_domain::codec::Name::new("Espresso").unwrap();
    assert_eq!(
        runtime.sold_out().unwrap(),
        [SoldOut { item: espresso(), name, until: Some(end_of_day()) }]
    );
    assert_eq!(runtime.availability_version().unwrap(), 1);
    assert_eq!(on_menu(&runtime, ESPRESSO), Some(SoldOutUntil(Some(end_of_day()))));
    assert_eq!(on_menu(&runtime, 0x402), None);
    assert!(matches!(
        ring(&mut runtime, ESPRESSO, &[]),
        Err(RuntimeError::SoldOut(name)) if name == "Espresso"
    ));
    // Another item still rings.
    ring(&mut runtime, 0x402, &[]).unwrap();
    // 86'd again until the same time changes nothing.
    assert!(matches!(
        runtime.eighty_six(espresso(), Until::EndOfDay, None),
        Err(RuntimeError::Availability(AvailabilityError::NoChange))
    ));
    // A minute before the day ends it is still sold out; when it ends, it is back, with no event.
    let ends = Timestamp::from_micros(1_791_014_400_000_000).unwrap(); // 2026-10-03T08:00:00Z
    runtime.clock().set(Timestamp::from_micros(ends.as_micros() - 60_000_000).unwrap());
    assert!(ring(&mut runtime, ESPRESSO, &[]).is_err());
    runtime.clock().set(ends);
    assert!(runtime.sold_out().unwrap().is_empty());
    assert_eq!(on_menu(&runtime, ESPRESSO), None);
    ring(&mut runtime, ESPRESSO, &[]).unwrap();
    assert_eq!(runtime.availability_version().unwrap(), 1);
}

#[test]
fn an_item_86d_until_put_back_is_sold_out_until_it_is() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch);
    let reason = Reason { code: ReasonCode::new("ran_out").unwrap(), note: None };
    runtime.eighty_six(espresso(), Until::PutBack, Some(reason)).unwrap();
    assert_eq!(runtime.sold_out().unwrap()[0].until, None);
    assert_eq!(on_menu(&runtime, ESPRESSO), Some(SoldOutUntil(None)));
    // A week later it is still sold out.
    runtime.clock().advance(Duration::from_secs(7 * 24 * 3_600)).unwrap();
    assert!(matches!(ring(&mut runtime, ESPRESSO, &[]), Err(RuntimeError::SoldOut(_))));
    runtime.put_back(espresso()).unwrap();
    assert!(runtime.sold_out().unwrap().is_empty());
    assert_eq!(on_menu(&runtime, ESPRESSO), None);
    assert_eq!(runtime.availability_version().unwrap(), 2);
    ring(&mut runtime, ESPRESSO, &[]).unwrap();
    // Put back again, it changes nothing.
    assert!(matches!(
        runtime.put_back(espresso()),
        Err(RuntimeError::Availability(AvailabilityError::NoChange))
    ));
    // Nor does putting back what was never 86'd.
    assert!(matches!(
        runtime.put_back(ItemRef::Variant(id(0x402))),
        Err(RuntimeError::Availability(AvailabilityError::NoChange))
    ));
}

#[test]
fn an_86_until_a_time_ends_then_and_a_time_already_past_is_refused() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch);
    let now = runtime.clock().now();
    let at_eleven = Timestamp::from_micros(now.as_micros() + 2 * 3_600_000_000).unwrap();
    assert!(matches!(
        runtime.eighty_six(espresso(), Until::Time(now), None),
        Err(RuntimeError::Availability(AvailabilityError::UntilPast))
    ));
    assert_eq!(runtime.availability_version().unwrap(), 0);
    runtime.eighty_six(espresso(), Until::Time(at_eleven), None).unwrap();
    let eleven = LocalTime { date: BusinessDate::new(2026, 10, 2).unwrap(), hour: 11, minute: 0 };
    assert_eq!(runtime.sold_out().unwrap()[0].until, Some(eleven));
    // 86'd until the day ends instead: a later time.
    runtime.eighty_six(espresso(), Until::EndOfDay, None).unwrap();
    assert_eq!(runtime.sold_out().unwrap()[0].until, Some(end_of_day()));
    runtime.eighty_six(espresso(), Until::Time(at_eleven), None).unwrap();
    runtime.clock().set(at_eleven);
    assert!(runtime.sold_out().unwrap().is_empty());
    ring(&mut runtime, ESPRESSO, &[]).unwrap();
}

#[test]
fn a_modifier_86d_shows_in_the_sheet_and_refuses_what_chooses_it_however_deep() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch);
    // A large latte with oat milk, rung before the 86: it stays.
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x403), &big_latte(), NonZeroU32::MIN).unwrap();
    runtime.eighty_six(oat_milk(), Until::EndOfDay, None).unwrap();
    assert_eq!(runtime.ticket(order).unwrap().lines.len(), 1);
    let name = keel_domain::codec::Name::new("Oat milk").unwrap();
    assert_eq!(
        runtime.sold_out().unwrap(),
        [SoldOut { item: oat_milk(), name, until: Some(end_of_day()) }]
    );
    let sheet = runtime.item(id(0x403)).unwrap();
    let milk = &sheet.groups[0];
    let flags: Vec<(u64, bool)> = milk
        .modifiers
        .iter()
        .map(|modifier| {
            let n = u64::from_str_radix(&modifier.modifier.to_string()[24..], 16).unwrap();
            (n, modifier.sold_out.is_some())
        })
        .collect();
    assert_eq!(flags, [(0x201, false), (0x202, true), (0x203, false), (0x204, false)]);
    assert_eq!(milk.modifiers[1].sold_out, Some(SoldOutUntil(Some(end_of_day()))));
    // The latte isn't sold out; with oat milk it can't be rung, with whole milk it can.
    assert_eq!(on_menu(&runtime, 0x403), None);
    assert!(matches!(
        ring(&mut runtime, 0x403, &big_latte()),
        Err(RuntimeError::SoldOut(name)) if name == "Oat milk"
    ));
    ring(&mut runtime, 0x403, &[Choice::of(id(0x201))]).unwrap();
    // Ketchup, chosen on a hash brown, chosen on a breakfast sandwich.
    runtime.eighty_six(ItemRef::Modifier(id(0x211)), Until::PutBack, None).unwrap();
    let sandwich = |sauce: u64| {
        vec![
            Choice::of(id(0x20d)),
            Choice {
                modifier: id(0x210),
                quantity: NonZeroU8::MIN,
                choices: vec![Choice::of(id(sauce))],
            },
        ]
    };
    assert!(matches!(
        ring(&mut runtime, 0x408, &sandwich(0x211)),
        Err(RuntimeError::SoldOut(name)) if name == "Ketchup"
    ));
    ring(&mut runtime, 0x408, &sandwich(0x212)).unwrap();
    let side = &runtime.item(id(0x408)).unwrap().groups[1];
    let sauces = &side.modifiers[1].groups[0];
    assert_eq!(sauces.modifiers[0].sold_out, Some(SoldOutUntil(None)));
    assert_eq!(sauces.modifiers[1].sold_out, None);
}

#[test]
fn an_86_is_gated_as_the_role_says() {
    let scratch = Scratch::new();
    let profile = demo_with(|data| {
        let cashier = data.roles.iter_mut().find(|role| role.id == id(0x602)).unwrap();
        cashier.permissions.retain(|permission| permission.action != Action::ItemAvailability);
    });
    let mut runtime = register(&scratch, profile);
    // No one signed in: nothing changes.
    assert!(matches!(
        runtime.eighty_six(espresso(), Until::EndOfDay, None),
        Err(RuntimeError::NotSignedIn)
    ));
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert!(matches!(
        runtime.eighty_six(espresso(), Until::EndOfDay, None),
        Err(RuntimeError::ApprovalRequired(Action::ItemAvailability))
    ));
    assert_eq!(runtime.availability_version().unwrap(), 0);
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.eighty_six(espresso(), Until::EndOfDay, None).unwrap();
    // The approval is spent: putting it back needs another.
    assert!(matches!(
        runtime.put_back(espresso()),
        Err(RuntimeError::ApprovalRequired(Action::ItemAvailability))
    ));
    runtime.approve(id(0x501), "1234").unwrap();
    runtime.put_back(espresso()).unwrap();
    // A change refused by the item's own rules asks for no approval.
    assert!(matches!(
        runtime.put_back(espresso()),
        Err(RuntimeError::Availability(AvailabilityError::NoChange))
    ));
}

#[test]
fn an_item_the_catalog_lacks_is_refused() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch);
    assert!(matches!(
        runtime.eighty_six(ItemRef::Variant(id(0x4ff)), Until::EndOfDay, None),
        Err(RuntimeError::UnknownItem(_))
    ));
    // A variant's identifier named as a modifier is no modifier.
    assert!(matches!(
        runtime.eighty_six(ItemRef::Modifier(id(ESPRESSO)), Until::EndOfDay, None),
        Err(RuntimeError::UnknownItem(_))
    ));
    assert_eq!(runtime.availability_version().unwrap(), 0);
}
