//! Known answers for the runtime: the demo café's register, from signing in to paying cash.

use core::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use keel_domain::checkout::CheckoutError;
use keel_domain::order::{Channel, CommandError, Lease, Mode, Order, Ownership, Prefix};
use keel_domain::payment::{CashTendered, Payment};
use keel_domain::profile::{CatalogVariant, Choice, Profile, ProfileData, RingError, demo};
use keel_events::envelope::{Actor, Device};
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_pricing::{Dining, PricingError, Tax};
use keel_store::StoreKey;
use keel_types::{
    BusinessDate, Currency, Id, Locale, ManualClock, Money, Rate, RoundingMode, RoundingRule,
    SeededEntropy,
};

use super::*;

type TestRuntime = Runtime<SoftwareSigner, SeededEntropy, ManualClock, SeededEntropy>;

/// A directory of its own for a store, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let shm = std::path::Path::new("/dev/shm");
        let base = if shm.is_dir() { shm.to_path_buf() } else { std::env::temp_dir() };
        let dir = base.join(format!("keel-runtime-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn db(&self) -> PathBuf {
        self.0.join("store.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(cents: i64) -> Money {
    Money::from_minor(cents, Currency::USD)
}

fn one() -> NonZeroU32 {
    NonZeroU32::MIN
}

/// A runtime of the demo café's register, at 9 am in New York on 2 October 2026.
fn register(scratch: &Scratch, profile: Profile) -> TestRuntime {
    register_at(scratch, profile, "2026-10-02T13:00:00Z")
}

/// A runtime of the demo café's register, with its clock reading `now`.
fn register_at(scratch: &Scratch, profile: Profile, now: &str) -> TestRuntime {
    let identity = Identity {
        device: id::<Device>(0xd1),
        signer: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[7; 32]).unwrap(),
    };
    let clock = ManualClock::new(now.parse().unwrap());
    Runtime::open(
        scratch.db(),
        StoreKey::new(&mut [0x4b; 32]),
        identity,
        profile,
        clock,
        SeededEntropy::new(1),
        SeededEntropy::new(2),
        Locale::EN_US,
    )
    .unwrap()
}

/// The demo café's profile, changed by `change`.
fn demo_with(change: impl FnOnce(&mut ProfileData)) -> Profile {
    let mut data = demo().unwrap().data().clone();
    change(&mut data);
    Profile::new(data).unwrap()
}

/// The variant `n` of `data`'s catalog.
fn variant(data: &mut ProfileData, n: u64) -> &mut CatalogVariant {
    data.catalog
        .items
        .iter_mut()
        .flat_map(|item| item.variants.iter_mut())
        .find(|variant| variant.id == id(n))
        .unwrap()
}

fn signed_in(scratch: &Scratch) -> TestRuntime {
    let mut runtime = register(scratch, demo().unwrap());
    runtime.sign_in(id(0x501)).unwrap();
    runtime
}

/// A large latte with oat milk, two extra shots, vanilla and caramel: one syrup is free.
fn big_latte() -> Vec<Choice> {
    vec![
        Choice::of(id(0x202)),
        Choice { modifier: id(0x205), quantity: NonZeroU8::new(2).unwrap(), choices: vec![] },
        Choice::of(id(0x207)),
        Choice::of(id(0x206)),
    ]
}

fn texts(ticket: &Ticket) -> (String, String, String, String, String) {
    (
        ticket.subtotal.text.clone(),
        ticket.taxes.iter().map(|tax| tax.amount.text.clone()).collect::<Vec<_>>().join(" "),
        ticket.total.text.clone(),
        ticket.paid.text.clone(),
        ticket.due.text.clone(),
    )
}

#[test]
fn nothing_happens_until_someone_signs_in() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    assert!(matches!(runtime.start_order(Mode::Takeout), Err(RuntimeError::NotSignedIn)));
    assert!(matches!(runtime.sign_in(id(0x5ff)), Err(RuntimeError::UnknownMember(_))));
    runtime.sign_in(id(0x502)).unwrap();
    assert_eq!(runtime.signed_in(), Some(id(0x502)));
    assert!(runtime.start_order(Mode::Takeout).is_ok());
    runtime.sign_out();
    assert!(matches!(runtime.start_order(Mode::Takeout), Err(RuntimeError::NotSignedIn)));
}

#[test]
fn an_order_belongs_to_this_device_and_the_member_signed_in() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.sign_in(id(0x502)).unwrap();
    let order = runtime.start_order(Mode::DineIn).unwrap().order;
    let loaded = runtime.store().load(Order::new(order)).unwrap();
    let info = loaded.info().unwrap();
    assert_eq!(
        (info.location, info.channel, info.mode, info.currency, info.owner),
        (id(0xc0fe), Channel::Pos, Mode::DineIn, Currency::USD, Some(id(0x502)))
    );
    assert_eq!(loaded.ownership(), Some(Ownership { device: id(0xd1), lease: Lease::FIRST }));
}

#[test]
fn events_record_the_business_date_and_who_did_what() {
    let scratch = Scratch::new();
    let date = |text: &str| text.parse::<BusinessDate>().unwrap();
    // A second before 4 am in New York on 3 October is still the business of 2 October.
    let order = {
        let mut runtime = register_at(&scratch, demo().unwrap(), "2026-10-03T07:59:59Z");
        runtime.sign_in(id(0x502)).unwrap();
        let order = runtime.start_order(Mode::Takeout).unwrap().order;
        runtime.add_item(order, id(0x401), &[], one()).unwrap();
        order
    };
    let mut runtime = register_at(&scratch, demo().unwrap(), "2026-10-03T08:00:00Z");
    runtime.sign_in(id(0x501)).unwrap();
    runtime.pay_cash(order, usd(500)).unwrap();

    // The order is created and the line added; then the payment is started and captured, and
    // the check and the order closed, in one intent.
    let events = runtime.store().log(id(0xd1), 0, 100).unwrap();
    let recorded: Vec<(BusinessDate, Actor)> = events
        .iter()
        .map(|event| (event.body().business_date, event.body().actor.clone()))
        .collect();
    let before = (date("2026-10-02"), Actor::TeamMember(id(0x502)));
    let after = (date("2026-10-03"), Actor::TeamMember(id(0x501)));
    assert_eq!(
        recorded,
        [before.clone(), before, after.clone(), after.clone(), after.clone(), after]
    );
    let correlations: Vec<_> = events.iter().map(|event| event.body().correlation).collect();
    assert!(correlations.iter().all(Option::is_some));
    assert_ne!(correlations[0], correlations[1]);
    assert_ne!(correlations[1], correlations[2]);
    assert_eq!(correlations[2..], [correlations[2]; 4]);
}

#[test]
fn an_order_that_costs_nothing_closes_without_a_payment() {
    let scratch = Scratch::new();
    let profile = demo_with(|data| variant(data, 0x409).price = usd(0));
    let mut runtime = register(&scratch, profile);
    runtime.sign_in(id(0x501)).unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let ticket = runtime.add_item(order, id(0x409), &[], one()).unwrap();
    assert_eq!(
        texts(&ticket),
        ("$0.00".into(), String::new(), "$0.00".into(), "$0.00".into(), "$0.00".into())
    );
    // Nothing is owed, but a tender is never less than nothing.
    assert!(matches!(runtime.pay_cash(order, usd(-500)), Err(RuntimeError::NegativeTender)));
    let paid = runtime.pay_cash(order, usd(0)).unwrap();
    assert_eq!(paid.change.text, "$0.00");
    assert_eq!(paid.ticket.state, TicketState::Closed);
    assert!(runtime.store().payments_of(order).unwrap().is_empty());
}

#[test]
fn a_sale_that_cash_rounds_to_nothing_takes_nothing() {
    let scratch = Scratch::new();
    let profile = demo_with(|data| {
        // Coffee beans, untaxed, at two cents: no cash at all, rounded to five cents.
        variant(data, 0x40a).price = usd(2);
        data.cash_rounding =
            Some(RoundingRule::new(RoundingMode::HalfAwayFromZero, NonZeroU32::new(5).unwrap()));
    });
    let mut runtime = register(&scratch, profile);
    runtime.sign_in(id(0x501)).unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let ticket = runtime.add_item(order, id(0x40a), &[], one()).unwrap();
    assert_eq!((ticket.due.text.as_str(), ticket.cash_due.text.as_str()), ("$0.02", "$0.00"));
    let paid = runtime.pay_cash(order, usd(0)).unwrap();
    assert_eq!((paid.change.text.as_str(), paid.ticket.paid.text.as_str()), ("$0.00", "$0.02"));
    let payments = runtime.store().payments_of(order).unwrap();
    let payment = runtime.store().load(Payment::new(payments[0].id)).unwrap();
    let captured = payment.captured().unwrap();
    assert_eq!(
        (captured.amount, captured.cash),
        (usd(2), Some(CashTendered { tendered: usd(0), rounding: Some(usd(-2)) }))
    );
}

#[test]
fn an_intent_whose_ticket_cant_be_priced_is_refused_whole() {
    let scratch = Scratch::new();
    // Bottled water at forty quadrillion dollars: one prices, three overflow.
    let profile = demo_with(|data| variant(data, 0x409).price = usd(4_000_000_000_000_000_000));
    let mut runtime = register(&scratch, profile);
    runtime.sign_in(id(0x501)).unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x409), &[], one()).unwrap();
    let before = runtime.store().log(id(0xd1), 0, 100).unwrap().len();
    assert!(matches!(
        runtime.add_item(order, id(0x409), &[], NonZeroU32::new(2).unwrap()),
        Err(RuntimeError::Pricing(PricingError::Overflow))
    ));
    // Nothing was written: the order still shows, with its one line.
    assert_eq!(runtime.store().log(id(0xd1), 0, 100).unwrap().len(), before);
    assert_eq!(runtime.ticket(order).unwrap().lines.len(), 1);
}

#[test]
fn a_paid_ticket_lists_its_taxes_in_the_locations_order() {
    let scratch = Scratch::new();
    // A tax on food eaten on the premises, after the sales tax, with an identifier sorting before
    // it.
    let profile = demo_with(|data| {
        data.rules.taxes.push(Tax {
            id: id(0x1f),
            name: "Dine-in tax".to_owned(),
            rate: Rate::from_fraction("0.01".parse().unwrap()),
            categories: vec![id(0x10)],
            dining: Some(Dining::OnPremises),
        });
    });
    let mut runtime = register(&scratch, profile);
    runtime.sign_in(id(0x501)).unwrap();
    let order = runtime.start_order(Mode::DineIn).unwrap().order;
    let open = runtime.add_item(order, id(0x403), &big_latte(), one()).unwrap();
    let taxes = |ticket: &Ticket| -> Vec<(String, String)> {
        ticket.taxes.iter().map(|tax| (tax.name.clone(), tax.amount.text.clone())).collect()
    };
    let expected = vec![
        ("NYC sales tax".to_owned(), "$0.75".to_owned()),
        ("Dine-in tax".to_owned(), "$0.09".to_owned()),
    ];
    assert_eq!(taxes(&open), expected);
    let paid = runtime.pay_cash(order, usd(1000)).unwrap();
    assert_eq!(taxes(&paid.ticket), expected);
    assert_eq!(paid.ticket.total.text, "$9.34");
}

#[test]
fn a_sale_rings_up_and_is_paid_in_cash() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let ticket = runtime.start_order(Mode::Takeout).unwrap();
    let order = ticket.order;
    assert_eq!(ticket.state, TicketState::Open);
    assert_eq!(
        texts(&ticket),
        ("$0.00".into(), String::new(), "$0.00".into(), "$0.00".into(), "$0.00".into())
    );

    // $5.25 + oat milk $0.75 + 2 × $0.95 + vanilla (free) + caramel $0.60 = $8.50, and 8.875% tax.
    let ticket = runtime.add_item(order, id(0x403), &big_latte(), one()).unwrap();
    let line = &ticket.lines[0];
    assert_eq!(line.name.as_str(), "Latte (Large)");
    assert_eq!(line.quantity_text, "1");
    assert_eq!(line.amount.text, "$8.50");
    let modifiers: Vec<(u8, Prefix, &str, u8, &str)> = line
        .modifiers
        .iter()
        .map(|m| (m.depth, m.prefix, m.name.as_str(), m.quantity, m.price.text.as_str()))
        .collect();
    assert_eq!(
        modifiers,
        [
            (1, Prefix::Plain, "Oat milk", 1, "$0.75"),
            (1, Prefix::Extra, "Shot", 2, "$0.95"),
            (1, Prefix::Plain, "Vanilla", 1, "$0.00"),
            (1, Prefix::Plain, "Caramel", 1, "$0.60"),
        ]
    );
    assert_eq!(
        texts(&ticket),
        ("$8.50".into(), "$0.75".into(), "$9.25".into(), "$0.00".into(), "$9.25".into())
    );
    assert_eq!(ticket.taxes[0].name, "NYC sales tax");

    // Two bags of beans, untaxed.
    let ticket = runtime.add_item(order, id(0x40a), &[], NonZeroU32::new(2).unwrap()).unwrap();
    assert_eq!(
        texts(&ticket),
        ("$36.50".into(), "$0.75".into(), "$37.25".into(), "$0.00".into(), "$37.25".into())
    );
    let beans = ticket.lines[1].line;
    let ticket = runtime.change_quantity(order, beans, one()).unwrap();
    assert_eq!(ticket.total.text, "$23.25");
    let ticket = runtime.remove_line(order, beans).unwrap();
    assert_eq!(ticket.lines.len(), 1);
    assert_eq!(ticket.total.text, "$9.25");

    assert!(matches!(
        runtime.pay_cash(order, usd(500)),
        Err(RuntimeError::TenderShort { due }) if due == usd(925)
    ));
    let paid = runtime.pay_cash(order, usd(2000)).unwrap();
    assert_eq!(paid.change.text, "$10.75");
    let ticket = paid.ticket;
    assert_eq!(ticket.state, TicketState::Closed);
    assert_eq!(
        texts(&ticket),
        ("$8.50".into(), "$0.75".into(), "$9.25".into(), "$9.25".into(), "$0.00".into())
    );
    assert_eq!(ticket.lines.len(), 1);
    assert_eq!(ticket.lines[0].amount.text, "$8.50");

    // A closed order takes nothing more, and isn't open.
    assert!(matches!(
        runtime.add_item(order, id(0x401), &[], one()),
        Err(RuntimeError::Order(CommandError::OrderClosed))
    ));
    assert!(runtime.open_orders().unwrap().is_empty());
}

#[test]
fn what_the_catalog_refuses_rings_nothing() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    // A bagel needs a spread.
    assert!(matches!(
        runtime.add_item(order, id(0x407), &[], one()),
        Err(RuntimeError::Ring(RingError::TooFew { .. }))
    ));
    assert!(matches!(
        runtime.add_item(id(0x9999), id(0x401), &[], one()),
        Err(RuntimeError::UnknownOrder(_))
    ));
    assert!(runtime.ticket(order).unwrap().lines.is_empty());
}

#[test]
fn an_order_dropped_takes_its_lines_off_first() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let order = runtime.start_order(Mode::DineIn).unwrap().order;
    runtime.add_item(order, id(0x401), &[], one()).unwrap();
    runtime.add_item(order, id(0x409), &[], one()).unwrap();
    assert_eq!(runtime.open_orders().unwrap().len(), 1);
    let ticket = runtime.abandon(order).unwrap();
    assert_eq!(ticket.state, TicketState::Abandoned);
    assert!(ticket.lines.is_empty());
    assert_eq!(ticket.due.text, "$0.00");
    assert!(runtime.open_orders().unwrap().is_empty());
}

#[test]
fn an_order_with_nothing_on_it_isnt_paid() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    assert!(matches!(
        runtime.pay_cash(order, usd(0)),
        Err(RuntimeError::Checkout(CheckoutError::Order(_)))
    ));
    assert!(matches!(
        runtime.pay_cash(order, Money::from_minor(0, Currency::EUR)),
        Err(RuntimeError::WrongCurrency)
    ));
}

#[test]
fn cash_is_rounded_as_the_location_rounds_it() {
    let scratch = Scratch::new();
    let mut data = demo().unwrap().data().clone();
    data.cash_rounding =
        Some(RoundingRule::new(RoundingMode::HalfAwayFromZero, NonZeroU32::new(5).unwrap()));
    let mut runtime = register(&scratch, Profile::new(data).unwrap());
    runtime.sign_in(id(0x501)).unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    // An espresso, $3.25, with $0.29 tax: $3.54, or $3.55 in cash.
    let ticket = runtime.add_item(order, id(0x401), &[], one()).unwrap();
    assert_eq!((ticket.total.text.as_str(), ticket.cash_due.text.as_str()), ("$3.54", "$3.55"));
    assert!(matches!(
        runtime.pay_cash(order, usd(354)),
        Err(RuntimeError::TenderShort { due }) if due == usd(355)
    ));
    let paid = runtime.pay_cash(order, usd(500)).unwrap();
    assert_eq!(paid.change.text, "$1.45");
    assert_eq!(paid.ticket.paid.text, "$3.54");
    assert_eq!(paid.ticket.due.text, "$0.00");
    // The payment covers $3.54, and records the cash handed over and the cent it was rounded.
    let payments = runtime.store().payments_of(order).unwrap();
    assert_eq!(payments.len(), 1);
    let payment = runtime.store().load(Payment::new(payments[0].id)).unwrap();
    let captured = payment.captured().unwrap();
    assert_eq!(
        (captured.amount, captured.cash),
        (usd(354), Some(CashTendered { tendered: usd(500), rounding: Some(usd(1)) }))
    );
}

#[test]
fn a_store_opened_again_shows_the_same_tickets() {
    let scratch = Scratch::new();
    let (order, before) = {
        let mut runtime = signed_in(&scratch);
        let order = runtime.start_order(Mode::Takeout).unwrap().order;
        runtime.add_item(order, id(0x403), &big_latte(), one()).unwrap();
        (order, runtime.ticket(order).unwrap())
    };
    let runtime = register(&scratch, demo().unwrap());
    assert_eq!(runtime.ticket(order).unwrap(), before);
}

#[test]
fn the_menu_shows_each_page_with_prices_and_what_must_be_chosen() {
    let scratch = Scratch::new();
    let runtime = register(&scratch, demo().unwrap());
    let menu = runtime.menu();
    let pages: Vec<(&str, usize)> =
        menu.pages.iter().map(|page| (page.name.as_str(), page.buttons.len())).collect();
    assert_eq!(pages, [("Coffee", 6), ("Food", 3), ("Retail", 1)]);
    let buttons = |page: usize| -> Vec<(&str, &str, bool, bool)> {
        menu.pages[page]
            .buttons
            .iter()
            .map(|b| (b.name.as_str(), b.price.text.as_str(), b.choices, b.required))
            .collect()
    };
    // Coffee offers modifiers, none of which must be chosen; a bagel needs its spread.
    assert_eq!(
        buttons(0),
        [
            ("Espresso", "$3.25", true, false),
            ("Latte (Small)", "$4.50", true, false),
            ("Latte (Large)", "$5.25", true, false),
            ("Drip coffee (Small)", "$2.50", true, false),
            ("Drip coffee (Medium)", "$2.95", true, false),
            ("Drip coffee (Large)", "$3.45", true, false),
        ]
    );
    assert_eq!(
        buttons(1),
        [
            ("Bagel", "$2.75", true, true),
            ("Breakfast sandwich", "$6.95", true, true),
            ("Bottled water", "$2.00", false, false),
        ]
    );
    assert_eq!(buttons(2), [("Coffee beans, 12 oz", "$14.00", false, false)]);
    let latte = runtime.item(id(0x403)).unwrap();
    assert_eq!((latte.name.as_str(), latte.price.text.as_str()), ("Latte (Large)", "$5.25"));
    let sandwich = runtime.item(id(0x408)).unwrap();
    let groups: Vec<(&str, u8, u8)> =
        sandwich.groups.iter().map(|g| (g.name.as_str(), g.min, g.max)).collect();
    assert_eq!(groups, [("Egg", 1, 1), ("Side", 0, 1)]);
    let hash_brown = &sandwich.groups[1].modifiers[1];
    assert_eq!((hash_brown.name.as_str(), hash_brown.price.text.as_str()), ("Hash brown", "$2.00"));
    assert_eq!(hash_brown.groups[0].name.as_str(), "Sauce");
    assert!(matches!(
        runtime.item(id(0x4ff)),
        Err(RuntimeError::Ring(RingError::UnknownVariant(_)))
    ));
}
