//! Known answers for the kitchen at the register (ADR-0034): an order sent, or paid, reaching
//! the stations that make it and expo; a screen's tickets shown, bumped and recalled, and its
//! stations' times; voids struck through, and slipped where a ticket printed; and the tickets no
//! screen showed in 10 s, printed where they can be, or nowhere, and every device told.

use core::num::{NonZeroU8, NonZeroU16, NonZeroU32};
use core::time::Duration;
use std::collections::BTreeSet;

use keel_doc::Columns;
use keel_doc::escpos::plain_text;
use keel_domain::codec::{Name, ReasonCode};
use keel_domain::kitchen::{KitchenTicket, RerouteTo};
use keel_domain::order::{CommandError, Mode, Order, Prefix, Reason};
use keel_domain::profile::{Allergen, Choice, Printer, ProfileData, Route, Station, demo};
use keel_domain::refs::Station as StationRef;
use keel_types::Id;

use crate::tests::{Scratch, TestRuntime, big_latte, demo_with, id, register};
use crate::{Aging, JobKind, RuntimeError};

const BAR: u64 = 0x701;
const KITCHEN: u64 = 0x702;
const EXPO: u64 = 0x703;

fn station(n: u64) -> Id<StationRef> {
    id(n)
}

fn printer(host: &str) -> Printer {
    Printer { host: host.to_owned(), port: NonZeroU16::new(9100).unwrap() }
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

/// A cashier signed in at the demo café's register: Sam, whose role takes lines off.
fn cashier(scratch: &Scratch, profile: keel_domain::profile::Profile) -> TestRuntime {
    let mut runtime = register(scratch, profile);
    runtime.sign_in(id(0x502), "2468").unwrap();
    runtime
}

/// A bagel with cream cheese, toasted.
fn bagel() -> Vec<Choice> {
    vec![Choice::of(id(0x20a)), Choice::of(id(0x213))]
}

/// An order of a large latte, a bagel and a bottled water, its mode `mode`.
fn breakfast(runtime: &mut TestRuntime, mode: Mode) -> Id<Order> {
    let order = runtime.start_order(mode).unwrap().order;
    let one = NonZeroU32::MIN;
    runtime.add_item(order, id(0x403), &big_latte(), one).unwrap();
    runtime.add_item(order, id(0x407), &bagel(), one).unwrap();
    runtime.add_item(order, id(0x409), &[], one).unwrap();
    order
}

/// The stations `order`'s tickets went to, and what each holds, by name.
fn tickets(runtime: &TestRuntime, order: Id<Order>) -> Vec<(Id<StationRef>, Vec<String>)> {
    runtime
        .store()
        .order_kitchen_tickets(order)
        .unwrap()
        .into_iter()
        .map(|summary| {
            let ticket = runtime.store().load(KitchenTicket::new(summary.id)).unwrap();
            let (sent, _) = ticket.sent().unwrap();
            (sent.station, sent.items.iter().map(|item| item.name.to_string()).collect())
        })
        .collect()
}

/// The documents of the kitchen's print jobs queued, each with the printer it goes to.
fn kitchen_jobs(runtime: &TestRuntime) -> Vec<(String, String)> {
    runtime
        .print_jobs()
        .unwrap()
        .into_iter()
        .filter(|job| job.kind == JobKind::Kitchen)
        .map(|job| {
            let queued = runtime.store().effect(&job.job.to_bytes()).unwrap().unwrap();
            let (doc, to) = crate::print::document(&queued).unwrap();
            (to.unwrap().host, plain_text(&doc, Columns::PAPER_80MM))
        })
        .collect()
}

#[test]
fn an_order_paid_reaches_the_stations_that_make_it_and_expo() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch, demo().unwrap());
    let order = breakfast(&mut runtime, Mode::Takeout);
    assert!(runtime.ticket(order).unwrap().lines.iter().all(|line| !line.sent));
    assert!(tickets(&runtime, order).is_empty(), "nothing goes before it is sent or paid");
    let total = runtime.ticket(order).unwrap().total.money;
    runtime.pay_cash(order, total).unwrap();
    // The latte to the bar, the bagel to the kitchen, and all three to expo; water to no cook.
    assert_eq!(
        tickets(&runtime, order),
        [
            (station(BAR), vec!["Latte (Large)".to_owned()]),
            (station(KITCHEN), vec!["Bagel".to_owned()]),
            (
                station(EXPO),
                vec!["Latte (Large)".to_owned(), "Bagel".to_owned(), "Bottled water".to_owned()]
            ),
        ]
    );
    assert!(runtime.ticket(order).unwrap().lines.iter().all(|line| line.sent));
    // What the bar makes: its modifiers, each before those under it; the latte's milk.
    let screen = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    let [ticket] = screen.tickets.as_slice() else { panic!("one ticket: {screen:?}") };
    assert_eq!((ticket.mode, ticket.aging, ticket.age), (Mode::Takeout, Aging::Fresh, 0));
    assert_eq!(ticket.code, order.to_string()[30..].to_uppercase());
    let latte = &ticket.items[0].item;
    let modifiers: Vec<(String, Prefix, u8, u8)> = latte
        .modifiers
        .iter()
        .map(|m| (m.name.to_string(), m.prefix, m.quantity.get(), m.depth.get()))
        .collect();
    assert_eq!(
        modifiers,
        [
            ("Oat milk".to_owned(), Prefix::Plain, 1, 1),
            ("Shot".to_owned(), Prefix::Extra, 2, 1),
            ("Vanilla".to_owned(), Prefix::Plain, 1, 1),
            ("Caramel".to_owned(), Prefix::Plain, 1, 1),
        ]
    );
    assert_eq!(latte.allergens, BTreeSet::from([Allergen::Milk]));
    assert_eq!(ticket.items[0].ready, None, "only expo asks what is ready");
    // The bagel: gluten, and milk from its cream cheese.
    let screen = runtime.kitchen_screen(&[station(KITCHEN)]).unwrap();
    assert_eq!(
        screen.tickets[0].items[0].item.allergens,
        BTreeSet::from([Allergen::Gluten, Allergen::Milk])
    );
}

#[test]
fn a_screen_shows_bumps_and_recalls_and_expo_sees_what_is_made() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch, demo().unwrap());
    let order = breakfast(&mut runtime, Mode::DineIn);
    runtime.send(order).unwrap();
    let version = runtime.kitchen_version().unwrap();
    assert_eq!(version, 3, "three tickets sent");
    // Shown for the first time: recorded, once.
    let bar = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert_eq!(runtime.kitchen_version().unwrap(), 4);
    runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert_eq!(runtime.kitchen_version().unwrap(), 4, "shown once");
    let latte = bar.tickets[0].ticket;
    // Expo: nothing is ready until its station bumps it; water no cook makes is.
    let ready = |runtime: &mut TestRuntime| -> Vec<Option<bool>> {
        let screen = runtime.kitchen_screen(&[station(EXPO)]).unwrap();
        screen.tickets[0].items.iter().map(|item| item.ready).collect()
    };
    assert_eq!(ready(&mut runtime), [Some(false), Some(false), Some(true)]);
    // No one need be signed in to bump.
    runtime.sign_out();
    runtime.bump(latte).unwrap();
    assert!(matches!(runtime.bump(latte), Err(RuntimeError::Kitchen(_))), "bumped already");
    assert_eq!(ready(&mut runtime), [Some(true), Some(false), Some(true)]);
    let bar = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert!(bar.tickets.is_empty());
    assert_eq!(bar.last_bumped, Some(latte));
    assert_eq!((bar.times[0].bumped, bar.times[0].average), (1, Some(0)));
    // A minute on, it ages; recalled, it is back.
    runtime.recall_ticket(latte).unwrap();
    runtime.clock().advance(Duration::from_secs(301)).unwrap();
    let bar = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert_eq!(
        (bar.tickets.len(), bar.tickets[0].aging, bar.tickets[0].age),
        (1, Aging::LateIsh, 301)
    );
    runtime.clock().advance(Duration::from_secs(300)).unwrap();
    assert_eq!(runtime.kitchen_screen(&[station(BAR)]).unwrap().tickets[0].aging, Aging::Late);
    runtime.bump(latte).unwrap();
    let bar = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert_eq!(bar.times[0].average, Some(601));
    // A screen of two stations shows both, and their times.
    let both = runtime.kitchen_screen(&[station(BAR), station(KITCHEN)]).unwrap();
    assert_eq!(both.tickets.len(), 1);
    assert_eq!(both.times.len(), 2);
}

#[test]
fn a_sent_line_is_voided_for_a_reason_and_struck_through() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch, demo().unwrap());
    let order = runtime.start_order(Mode::DineIn).unwrap().order;
    let one = NonZeroU32::MIN;
    let first = runtime.add_item(order, id(0x403), &big_latte(), one).unwrap().lines[0].line;
    // Voiding a line not sent asks nothing, and is refused: it is taken off.
    assert!(matches!(
        runtime.void_line(order, first, reason("customer")),
        Err(RuntimeError::Order(CommandError::LineNotFired(_)))
    ));
    let sent = runtime.send(order).unwrap();
    assert!(sent.lines[0].sent);
    // Sent, it can't be changed or taken off.
    assert!(matches!(
        runtime.change_quantity(order, first, NonZeroU32::new(2).unwrap()),
        Err(RuntimeError::Order(_))
    ));
    assert!(matches!(runtime.remove_line(order, first), Err(RuntimeError::Order(_))));
    // A second send sends only what is new.
    let second = runtime.add_item(order, id(0x401), &[], one).unwrap().lines[1].line;
    runtime.send(order).unwrap();
    let bar = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert_eq!(bar.tickets.len(), 2);
    assert_eq!(bar.tickets[1].items[0].item.line, second);
    // Sending with nothing pending sends nothing.
    runtime.send(order).unwrap();
    assert_eq!(runtime.kitchen_screen(&[station(BAR)]).unwrap().tickets.len(), 2);
    runtime.void_line(order, first, reason("customer")).unwrap();
    let bar = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert!(bar.tickets[0].items[0].void);
    assert!(!bar.tickets[1].items[0].void);
    // Dropping the order voids it, once a manager approves: every item on every ticket.
    assert!(matches!(runtime.abandon(order), Err(RuntimeError::ApprovalRequired(_))));
    runtime.approve(id(0x501), "1234").unwrap();
    let dropped = runtime.abandon(order).unwrap();
    assert_eq!(dropped.state, crate::TicketState::Voided);
    let bar = runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert!(bar.tickets.iter().flat_map(|ticket| &ticket.items).all(|item| item.void));
}

/// The demo's kitchen, its kitchen station printed and shown on no screen, and the bar printed
/// as well as shown.
fn printed() -> keel_domain::profile::Profile {
    demo_with(|data: &mut ProfileData| {
        let kitchen = data.kitchen.as_mut().unwrap();
        kitchen.stations[0].printer = Some(printer("10.0.0.21"));
        kitchen.stations[1].screen = false;
        kitchen.stations[1].printer = Some(printer("10.0.0.22"));
    })
}

#[test]
fn a_station_no_screen_shows_prints_and_a_void_slips_where_it_printed() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch, printed());
    let order = breakfast(&mut runtime, Mode::DineIn);
    let bagel = runtime.ticket(order).unwrap().lines[1].line;
    let sandwich = vec![Choice::of(id(0x20d))];
    runtime.add_item(order, id(0x408), &sandwich, NonZeroU32::MIN).unwrap();
    runtime.send(order).unwrap();
    // The kitchen's ticket prints at once, at its printer.
    let jobs = kitchen_jobs(&runtime);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].0, "10.0.0.22");
    let text = &jobs[0].1;
    for words in
        ["Kitchen", "DINE IN", "1 x Bagel", "Cream cheese", "Toasted", "ALLERGENS: gluten, milk"]
    {
        assert!(text.contains(words), "{words:?} in\n{text}");
    }
    // The bagel voided: a slip where its ticket printed.
    runtime.void_line(order, bagel, reason("customer")).unwrap();
    let jobs = kitchen_jobs(&runtime);
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[1].0, "10.0.0.22");
    assert!(jobs[1].1.contains("VOID") && jobs[1].1.contains("1 x Bagel"), "{}", jobs[1].1);
    assert!(jobs[0].1.contains("Breakfast sandwich"), "the ticket had both");
    assert!(!jobs[1].1.contains("Breakfast sandwich"), "the slip, the bagel alone: {}", jobs[1].1);
    // The print loop hands a kitchen job over with its printer, with none of the register's own.
    let job = runtime.next_print_job().unwrap().unwrap();
    assert_eq!(job.kind, JobKind::Kitchen);
    assert_eq!(job.to, Some(printer("10.0.0.22")));
}

#[test]
fn a_ticket_no_screen_shows_in_10_seconds_prints_rerouted_and_every_device_is_told() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch, printed());
    let order = breakfast(&mut runtime, Mode::Takeout);
    runtime.send(order).unwrap();
    // Expo's screen shows its ticket; the bar's shows none. Nothing reroutes before 10 s.
    runtime.kitchen_screen(&[station(EXPO)]).unwrap();
    runtime.clock().advance(Duration::from_secs(9)).unwrap();
    runtime.next_print_job().unwrap();
    assert!(runtime.kitchen_alerts().unwrap().is_empty());
    runtime.clock().advance(Duration::from_secs(1)).unwrap();
    let _ = runtime.next_print_job().unwrap();
    // The bar's prints at its printer, marked; expo's, shown, doesn't; the kitchen's printed.
    let alerts = runtime.kitchen_alerts().unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!((alerts[0].station.as_str(), alerts[0].to), ("Bar", RerouteTo::StationPrinter));
    assert_eq!(alerts[0].code, order.to_string()[30..].to_uppercase());
    let jobs = kitchen_jobs(&runtime);
    let bar = jobs.iter().find(|(host, _)| host == "10.0.0.21").unwrap();
    assert!(bar.1.contains("REROUTED: Bar's screen didn't show it"), "{}", bar.1);
    // Shown now, the screen records nothing: it was rerouted.
    let version = runtime.kitchen_version().unwrap();
    runtime.kitchen_screen(&[station(BAR)]).unwrap();
    assert_eq!(runtime.kitchen_version().unwrap(), version);
    // Not rerouted twice; ten minutes on, no longer an alert.
    runtime.next_print_job().ok();
    assert_eq!(runtime.kitchen_alerts().unwrap().len(), 1);
    runtime.clock().advance(Duration::from_secs(601)).unwrap();
    assert!(runtime.kitchen_alerts().unwrap().is_empty());
}

#[test]
fn a_ticket_with_nowhere_to_print_is_rerouted_nowhere_and_told() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch, demo().unwrap());
    let order = breakfast(&mut runtime, Mode::Takeout);
    runtime.send(order).unwrap();
    runtime.clock().advance(Duration::from_secs(11)).unwrap();
    assert_eq!(runtime.next_print_job().unwrap(), None, "nothing to print");
    let alerts = runtime.kitchen_alerts().unwrap();
    assert_eq!(alerts.len(), 3);
    assert!(alerts.iter().all(|alert| alert.to == RerouteTo::Nowhere));
    // A fallback printer takes what a station without one would drop.
    let scratch = Scratch::new();
    let profile = demo_with(|data| {
        data.kitchen.as_mut().unwrap().fallback_printer = Some(printer("10.0.0.30"));
    });
    let mut runtime = cashier(&scratch, profile);
    let order = breakfast(&mut runtime, Mode::Takeout);
    runtime.send(order).unwrap();
    runtime.clock().advance(Duration::from_secs(10)).unwrap();
    runtime.next_print_job().unwrap();
    assert!(
        runtime
            .kitchen_alerts()
            .unwrap()
            .iter()
            .all(|alert| alert.to == RerouteTo::FallbackPrinter)
    );
    assert_eq!(kitchen_jobs(&runtime).iter().filter(|(host, _)| host == "10.0.0.30").count(), 3);
}

#[test]
fn a_modifier_routed_on_its_own_goes_as_a_component() {
    let scratch = Scratch::new();
    let profile = demo_with(|data| {
        let kitchen = data.kitchen.as_mut().unwrap();
        kitchen.stations.push(Station {
            id: id(0x704),
            name: Name::new("Fry").unwrap(),
            expo: false,
            screen: true,
            printer: None,
        });
        kitchen.routes.push(Route {
            station: id(0x704),
            items: BTreeSet::new(),
            modifiers: [id(0x210)].into(),
            modes: BTreeSet::new(),
        });
    });
    let mut runtime = cashier(&scratch, profile);
    let order = runtime.start_order(Mode::DineIn).unwrap().order;
    let sandwich = vec![
        Choice::of(id(0x20d)),
        Choice {
            modifier: id(0x210),
            quantity: NonZeroU8::MIN,
            choices: vec![Choice::of(id(0x211))],
        },
    ];
    runtime.add_item(order, id(0x408), &sandwich, NonZeroU32::new(2).unwrap()).unwrap();
    runtime.send(order).unwrap();
    let fry = runtime.kitchen_screen(&[id(0x704)]).unwrap();
    let item = &fry.tickets[0].items[0].item;
    assert_eq!(item.name.as_str(), "Hash brown");
    assert_eq!(item.component_of.as_ref().map(Name::as_str), Some("Breakfast sandwich"));
    assert_eq!(item.quantity.micros(), 2_000_000, "two sandwiches, a hash brown each");
    assert_eq!(item.modifiers.iter().map(|m| m.name.to_string()).collect::<Vec<_>>(), ["Ketchup"]);
    // The sandwich itself goes to the kitchen whole, its eggs among its allergens.
    let kitchen = runtime.kitchen_screen(&[station(KITCHEN)]).unwrap();
    let whole = &kitchen.tickets[0].items[0].item;
    assert_eq!(whole.name.as_str(), "Breakfast sandwich");
    assert!(whole.allergens.contains(&Allergen::Eggs));
}

#[test]
fn a_location_without_a_kitchen_sends_nothing() {
    let scratch = Scratch::new();
    let mut runtime = cashier(&scratch, demo_with(|data| data.kitchen = None));
    let order = breakfast(&mut runtime, Mode::Takeout);
    let total = runtime.ticket(order).unwrap().total.money;
    let paid = runtime.pay_cash(order, total).unwrap();
    assert!(paid.ticket.lines.iter().all(|line| !line.sent));
    assert!(tickets(&runtime, order).is_empty());
    assert!(runtime.kitchen_screen(&[station(BAR)]).unwrap().tickets.is_empty());
    assert!(runtime.kitchen_alerts().unwrap().is_empty());
}

#[test]
fn a_modifier_left_out_adds_no_allergens_and_a_component_counts_its_applications() {
    let scratch = Scratch::new();
    let profile = demo_with(|data| {
        // The bagel's "no spread" given sesame, which leaving it out mustn't add; and the shots
        // made on their own station.
        for modifier in data.catalog.groups.iter_mut().flat_map(|group| group.modifiers.iter_mut())
        {
            if modifier.id == id(0x20c) {
                modifier.allergens.insert(Allergen::Sesame);
            }
        }
        let kitchen = data.kitchen.as_mut().unwrap();
        kitchen.stations.push(Station {
            id: id(0x704),
            name: Name::new("Shots").unwrap(),
            expo: false,
            screen: true,
            printer: None,
        });
        kitchen.routes.push(Route {
            station: id(0x704),
            items: BTreeSet::new(),
            modifiers: [id(0x205)].into(),
            modes: BTreeSet::new(),
        });
    });
    let mut runtime = cashier(&scratch, profile);
    let order = runtime.start_order(Mode::DineIn).unwrap().order;
    runtime.add_item(order, id(0x407), &[Choice::of(id(0x20c))], NonZeroU32::MIN).unwrap();
    let shots =
        Choice { modifier: id(0x205), quantity: NonZeroU8::new(2).unwrap(), choices: Vec::new() };
    runtime.add_item(order, id(0x401), &[shots], NonZeroU32::new(3).unwrap()).unwrap();
    runtime.send(order).unwrap();
    let kitchen = runtime.kitchen_screen(&[station(KITCHEN)]).unwrap();
    assert_eq!(kitchen.tickets[0].items[0].item.allergens, BTreeSet::from([Allergen::Gluten]));
    // Three espressos, two shots each: six shots.
    let shots = runtime.kitchen_screen(&[id(0x704)]).unwrap();
    let item = &shots.tickets[0].items[0].item;
    assert_eq!((item.name.as_str(), item.quantity.micros()), ("Shot", 6_000_000));
}
