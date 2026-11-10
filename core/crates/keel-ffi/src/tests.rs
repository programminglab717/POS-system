//! Known answers for the bindings (ADR-0025, decision 8): conversions at the boundary, every
//! refusal's variant, and a register driven through a sale and its print jobs as a shell drives
//! it.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test code: a fake printer's bookkeeping"
)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use keel_doc::escpos;
use keel_domain::profile as domain;

use crate::register::{choices, drain, preferred_hub};
use crate::*;

/// A directory of its own for a store, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("keel-ffi-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The demo's identifier ending in `n`.
fn id<T>(n: u64) -> Id<T> {
    Id::parse(&demo_id(n)).unwrap()
}

fn demo_id(n: u64) -> String {
    format!("0192f0c1-0000-7000-8000-{n:012x}")
}

fn config(scratch: &Scratch) -> RegisterConfig {
    RegisterConfig {
        path: scratch.0.join("store.db").to_string_lossy().into_owned(),
        store_key: vec![0x4b; 32],
        device: demo_id(0xd1),
        signing_key: vec![7; 32],
        profile: demo_profile(),
        locale: "en-US".to_owned(),
    }
}

fn choice(n: u64, quantity: u8, parent: Option<u32>) -> Choice {
    Choice { modifier: demo_id(n), quantity, parent }
}

#[test]
fn flattened_choices_become_their_tree() {
    // A sandwich with fried egg and a hash brown, ketchup and hot sauce under the hash brown.
    let flat = [
        choice(0x20e, 1, None),
        choice(0x210, 1, None),
        choice(0x211, 1, Some(1)),
        choice(0x212, 2, Some(1)),
    ];
    let tree = choices(&flat).unwrap();
    let of = |n: u64| Id::parse(&demo_id(n)).unwrap();
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0], domain::Choice::of(of(0x20e)));
    assert_eq!(tree[1].modifier, of(0x210));
    assert_eq!(
        tree[1].choices.iter().map(|c| (c.modifier, c.quantity.get())).collect::<Vec<_>>(),
        [(of(0x211), 1), (of(0x212), 2)]
    );
    // Two deep: a choice under a choice under a choice.
    let deep =
        choices(&[choice(1, 1, None), choice(2, 1, Some(0)), choice(3, 1, Some(1))]).unwrap();
    assert_eq!(deep[0].choices[0].choices[0].modifier, of(3));
    // A parent must come before its child, and quantities be 1 or more.
    for bad in [
        vec![choice(1, 1, Some(0))],
        vec![choice(1, 1, Some(5))],
        vec![choice(1, 0, None)],
        vec![Choice { modifier: "latte".into(), quantity: 1, parent: None }],
    ] {
        assert!(matches!(choices(&bad), Err(KeelError::InvalidArgument { .. })), "{bad:?}");
    }
}

use keel_types::Id;

#[test]
fn a_register_opens_only_with_what_it_needs() {
    let scratch = Scratch::new();
    let bad = [
        RegisterConfig { store_key: vec![0; 31], ..config(&scratch) },
        RegisterConfig { signing_key: vec![0; 33], ..config(&scratch) },
        RegisterConfig { device: "a register".into(), ..config(&scratch) },
        RegisterConfig { profile: vec![0xa0], ..config(&scratch) },
        RegisterConfig { locale: "xx-XX".into(), ..config(&scratch) },
    ];
    for config in bad {
        assert!(matches!(Register::open(config), Err(KeelError::InvalidArgument { .. })));
    }
    drop(Register::open(config(&scratch)).unwrap());
    // Another key doesn't open the store.
    let wrong = RegisterConfig { store_key: vec![0x4c; 32], ..config(&scratch) };
    assert!(matches!(Register::open(wrong), Err(KeelError::WrongKey)));
    // Found in review: a store opened twice had two writers. The second open is refused until
    // the first register is gone.
    let first = Register::open(config(&scratch)).unwrap();
    assert!(matches!(Register::open(config(&scratch)), Err(KeelError::StoreInUse)));
    drop(first);
    drop(Register::open(config(&scratch)).unwrap());
    // Identifiers in their canonical form only: lowercase, as the register gives them.
    let upper = RegisterConfig { device: demo_id(0xd1).to_uppercase(), ..config(&scratch) };
    assert!(matches!(Register::open(upper), Err(KeelError::InvalidArgument { .. })));
}

/// A printer that takes one connection, answers as a ready Epson, and gives back the job.
fn printer() -> (u16, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (mut all, mut buf) = (Vec::new(), [0_u8; 4096]);
        let mut answered = 0;
        loop {
            let n = stream.read(&mut buf).unwrap();
            all.extend_from_slice(&buf[..n]);
            let questions = all.windows(2).filter(|w| *w == [0x10, 0x04]).count();
            while answered < questions {
                stream.write_all(&[0x12]).unwrap();
                answered += 1;
            }
            let ask = [0x1D, 0x28, 0x48, 6, 0, 48, 48];
            if let Some(at) = all.windows(ask.len()).position(|w| w == ask)
                && all.len() >= at + 11
            {
                let id: [u8; 4] = all[at + 7..at + 11].try_into().unwrap();
                stream.write_all(&escpos::printed(id)).unwrap();
                return all;
            }
        }
    });
    (port, server)
}

#[test]
fn a_sale_rings_pays_and_prints_through_the_register() {
    let scratch = Scratch::new();
    let register: Arc<Register> = Register::open(config(&scratch)).unwrap();
    assert_eq!(register.start_order(Mode::Takeout), Err(KeelError::NotSignedIn));
    assert!(matches!(register.sign_in(demo_id(0x599)), Err(KeelError::UnknownMember { .. })));
    register.sign_in(demo_id(0x501)).unwrap();
    let menu = register.menu().unwrap();
    assert_eq!(menu.pages[0].name, "Coffee");
    assert_eq!(menu.pages[0].buttons[2].name, "Latte (Large)");
    assert_eq!(
        menu.pages[0].buttons[2].price,
        Amount { minor: 525, currency: "USD".into(), text: "$5.25".into() }
    );
    // The breakfast sandwich's groups, flattened: the sauce under the hash brown, two deep.
    let item = register.item(demo_id(0x408)).unwrap();
    let groups: Vec<_> =
        item.groups.iter().map(|g| (g.name.as_str(), g.depth, g.under.clone())).collect();
    assert_eq!(groups, [("Egg", 1, None), ("Side", 1, None), ("Sauce", 2, Some(demo_id(0x210)))]);

    let order = register.start_order(Mode::Takeout).unwrap().order;
    // A sandwich without its egg is refused by the catalog.
    let refused = register.add_item(order.clone(), demo_id(0x408), vec![], 1);
    assert!(matches!(refused, Err(KeelError::Catalog { .. })), "{refused:?}");
    let latte = vec![choice(0x202, 1, None), choice(0x205, 2, None)];
    let ticket = register.add_item(order.clone(), demo_id(0x403), latte, 1).unwrap();
    assert_eq!(ticket.lines[0].name, "Latte (Large)");
    assert_eq!(
        ticket.lines[0]
            .modifiers
            .iter()
            .map(|m| (m.name.as_str(), m.prefix, m.quantity))
            .collect::<Vec<_>>(),
        [("Oat milk", Prefix::Plain, 1), ("Shot", Prefix::Extra, 2)]
    );
    assert_eq!(ticket.total.text, "$8.60");
    let line = ticket.lines[0].line.clone();
    let ticket = register.change_quantity(order.clone(), line, 2).unwrap();
    assert_eq!(ticket.lines[0].quantity_millionths, 2_000_000);
    assert_eq!(ticket.lines[0].quantity_text, "2");
    assert_eq!(ticket.total.text, "$17.20");
    assert_eq!(register.open_orders().unwrap().len(), 1);

    // Short, negative, then enough.
    match register.pay_cash(order.clone(), 1_000) {
        Err(KeelError::TenderShort { due }) => {
            assert_eq!(due, Amount { minor: 1720, currency: "USD".into(), text: "$17.20".into() });
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(register.pay_cash(order.clone(), -1), Err(KeelError::NegativeTender));
    let paid = register.pay_cash(order.clone(), 2_000).unwrap();
    assert_eq!(paid.change.text, "$2.80");
    assert_eq!(paid.ticket.state, TicketState::Closed);
    assert!(matches!(
        register.add_item(order.clone(), demo_id(0x401), vec![], 1),
        Err(KeelError::Refused { .. })
    ));
    assert!(matches!(register.ticket(demo_id(0x999)), Err(KeelError::UnknownOrder { .. })));

    // The drawer and the receipt, through the driver to a printer.
    assert_eq!(
        register.print_jobs().unwrap().iter().map(|job| (job.kind, job.state)).collect::<Vec<_>>(),
        [(JobKind::Drawer, JobState::Queued), (JobKind::Receipt, JobState::Queued)]
    );
    assert_eq!(register.next_print_job().unwrap(), None);
    assert!(matches!(
        register.set_printer(Printer { columns: 23, cutter: true, confirm: Confirm::Printed }),
        Err(KeelError::InvalidArgument { .. })
    ));
    register.set_printer(Printer { columns: 48, cutter: true, confirm: Confirm::Printed }).unwrap();
    let waits = Waits { status_ms: 2_000, word_ms: 5_000 };
    let mut kinds = Vec::new();
    while let Some(job) = register.next_print_job().unwrap() {
        let (port, server) = printer();
        let outcome = print_tcp("127.0.0.1".into(), port, job.clone(), waits).unwrap();
        assert_eq!(outcome, Outcome::Printed { paper_low: false });
        assert!(server.join().unwrap().windows(job.bytes.len()).any(|w| w == job.bytes.as_slice()));
        register.print_outcome(job.job.clone(), outcome).unwrap();
        assert!(matches!(
            register.print_outcome(job.job, outcome),
            Err(KeelError::UnknownJob { .. })
        ));
        kinds.push(job.kind);
    }
    assert_eq!(kinds, [JobKind::Drawer, JobKind::Receipt]);
    assert!(register.print_jobs().unwrap().is_empty());

    // A reprint, sent where no printer listens: not sent, and queued to be tried again.
    let reprint = register.reprint(order).unwrap();
    let job = register.next_print_job().unwrap().unwrap();
    assert_eq!(job.job, reprint);
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let outcome = print_tcp("127.0.0.1".into(), closed, job.clone(), waits).unwrap();
    assert_eq!(outcome, Outcome::NotSent { reason: NotSent::Unreachable });
    register.print_outcome(job.job.clone(), outcome).unwrap();
    assert_eq!(register.print_jobs().unwrap()[0].state, JobState::Queued);
    assert!(matches!(
        register.resolve(job.job, Resolution::Printed),
        Err(KeelError::NotUnconfirmed { .. })
    ));
    register.sign_out().unwrap();
    assert_eq!(register.start_order(Mode::DineIn), Err(KeelError::NotSignedIn));
}

#[test]
fn outcomes_cross_the_boundary_and_back_unchanged() {
    use keel_print::{NotSent as N, Outcome as O, Unconfirmed as U};
    let outcomes = [
        O::Printed { paper_low: true },
        O::Printed { paper_low: false },
        O::NotSent(N::Unreachable),
        O::NotSent(N::NoAnswer),
        O::NotSent(N::CoverOpen),
        O::NotSent(N::PaperOut),
        O::NotSent(N::Error),
        O::NotSent(N::Offline),
        O::NotSent(N::SendFailed),
        O::Unconfirmed(U::SendFailed),
        O::Unconfirmed(U::Closed),
        O::Unconfirmed(U::NoWord),
    ];
    for outcome in outcomes {
        assert_eq!(keel_print::Outcome::from(Outcome::from(outcome)), outcome);
    }
    // A byte that isn't a status keeps its reason, not the byte.
    assert_eq!(
        Outcome::from(O::NotSent(N::NotAStatus(0xFF))),
        Outcome::NotSent { reason: NotSent::NotAStatus }
    );
}

#[test]
fn every_prefix_and_mode_crosses_as_itself() {
    use keel_domain::order;
    let prefixes = [
        (order::Prefix::Plain, Prefix::Plain),
        (order::Prefix::No, Prefix::No),
        (order::Prefix::Extra, Prefix::Extra),
        (order::Prefix::Light, Prefix::Light),
        (order::Prefix::Side, Prefix::Side),
        (order::Prefix::Sub, Prefix::Sub),
    ];
    for (domain, ffi) in prefixes {
        assert_eq!(Prefix::from(domain), ffi);
    }
    let modes = [
        (Mode::DineIn, order::Mode::DineIn),
        (Mode::Takeout, order::Mode::Takeout),
        (Mode::Pickup, order::Mode::Pickup),
        (Mode::Delivery, order::Mode::Delivery),
        (Mode::Ship, order::Mode::Ship),
        (Mode::Service, order::Mode::Service),
    ];
    for (ffi, domain) in modes {
        assert_eq!(order::Mode::from(ffi), domain);
    }
}

#[test]
fn every_store_failure_has_a_variant() {
    use keel_store::StoreError as S;
    let cases = [
        (S::Busy, KeelError::StoreInUse),
        (S::KeyRejected, KeelError::WrongKey),
        (S::NewerSchema { found: 9, known: 3 }, KeelError::StoreNewer),
        (S::NotThisDevice, KeelError::NotThisDevice),
        (S::WrongSigner, KeelError::NotThisDevice),
        (S::Damaged, KeelError::Damaged),
    ];
    for (store, keel) in cases {
        assert_eq!(KeelError::from(keel_runtime::RuntimeError::Store(store)), keel);
    }
    assert!(matches!(KeelError::from(S::Corrupt("a row")), KeelError::Device { .. }));
}

#[test]
fn every_refusal_a_cashier_meets_has_a_variant() {
    use keel_domain::checkout::CheckoutError;
    use keel_domain::order::CommandError;
    use keel_domain::payment::PaymentError;
    use keel_domain::profile::RingError;
    use keel_runtime::RuntimeError as R;
    let refused = |error: R| matches!(KeelError::from(error), KeelError::Refused { .. });
    assert!(refused(R::Order(CommandError::NotCreated)));
    assert!(refused(R::Checkout(CheckoutError::InvalidAmount)));
    assert!(refused(R::Payment(PaymentError::NotInitiated)));
    assert!(matches!(
        KeelError::from(R::Ring(RingError::UnknownVariant(id(1)))),
        KeelError::Catalog { .. }
    ));
    let cases = [
        (R::NotSignedIn, KeelError::NotSignedIn),
        (R::UnknownMember(id(2)), KeelError::UnknownMember { member: demo_id(2) }),
        (R::UnknownOrder(id(3)), KeelError::UnknownOrder { order: demo_id(3) }),
        (R::WrongCurrency, KeelError::WrongCurrency),
        (R::NegativeTender, KeelError::NegativeTender),
        (R::UnknownJob(id(4)), KeelError::UnknownJob { job: demo_id(4) }),
        (R::NotUnconfirmed(id(5)), KeelError::NotUnconfirmed { job: demo_id(5) }),
        (R::NoReceipt(id(6)), KeelError::NoReceipt { order: demo_id(6) }),
    ];
    for (runtime, keel) in cases {
        assert_eq!(KeelError::from(runtime), keel);
    }
}

fn amount(minor: i64) -> keel_runtime::Amount {
    keel_runtime::Amount::of(
        keel_types::Money::from_minor(minor, keel_types::Currency::USD),
        keel_types::Locale::EN_US,
    )
}

fn usd(minor: i64, text: &str) -> Amount {
    Amount { minor, currency: "USD".into(), text: text.into() }
}

/// Found in review: most conversions could lose or swap a field unnoticed. Every field of a
/// ticket, each a different value, crosses as itself.
#[test]
fn a_ticket_crosses_field_for_field() {
    use keel_runtime as rt;
    let ticket = rt::Ticket {
        order: id(1),
        state: rt::TicketState::Voided,
        lines: vec![rt::TicketLine {
            line: id(2),
            name: keel_domain::codec::Name::new("Latte").unwrap(),
            quantity: keel_types::Quantity::from_whole(3, keel_types::Unit::Each).unwrap(),
            quantity_text: "3".into(),
            modifiers: vec![rt::TicketModifier {
                depth: 2,
                modifier: id(3),
                prefix: keel_domain::order::Prefix::Side,
                name: keel_domain::codec::Name::new("Oat").unwrap(),
                quantity: 4,
                price: amount(75),
            }],
            amount: amount(1_000),
        }],
        subtotal: amount(1_001),
        taxes: vec![rt::TicketTax { tax: id(4), name: "Tax".into(), amount: amount(89) }],
        total: amount(1_090),
        paid: amount(500),
        due: amount(590),
        cash_due: amount(590 + 5),
    };
    let crossed = Ticket::from(&ticket);
    let expected = Ticket {
        order: demo_id(1),
        state: TicketState::Voided,
        lines: vec![TicketLine {
            line: demo_id(2),
            name: "Latte".into(),
            quantity_millionths: 3_000_000,
            quantity_text: "3".into(),
            modifiers: vec![TicketModifier {
                depth: 2,
                modifier: demo_id(3),
                prefix: Prefix::Side,
                name: "Oat".into(),
                quantity: 4,
                price: usd(75, "$0.75"),
            }],
            amount: usd(1_000, "$10.00"),
        }],
        subtotal: usd(1_001, "$10.01"),
        taxes: vec![TicketTax { tax: demo_id(4), name: "Tax".into(), amount: usd(89, "$0.89") }],
        total: usd(1_090, "$10.90"),
        paid: usd(500, "$5.00"),
        due: usd(590, "$5.90"),
        cash_due: usd(595, "$5.95"),
    };
    assert_eq!(crossed, expected);
    for (state, ffi) in [
        (rt::TicketState::Open, TicketState::Open),
        (rt::TicketState::Closed, TicketState::Closed),
        (rt::TicketState::Voided, TicketState::Voided),
        (rt::TicketState::Abandoned, TicketState::Abandoned),
    ] {
        assert_eq!(Ticket::from(&rt::Ticket { state, ..ticket.clone() }).state, ffi);
    }
}

#[test]
fn an_items_groups_cross_field_for_field() {
    let scratch = Scratch::new();
    let register = Register::open(config(&scratch)).unwrap();
    // The latte's milk, extra shots and syrup: each group's bounds, free applications and
    // whether it repeats, and each modifier's prefix and price.
    let item = register.item(demo_id(0x403)).unwrap();
    let bounds: Vec<_> =
        item.groups.iter().map(|g| (g.name.as_str(), g.min, g.max, g.free, g.repeat)).collect();
    assert_eq!(
        bounds,
        [("Milk", 0, 1, 0, false), ("Extra shots", 0, 4, 0, true), ("Syrup", 0, 3, 1, true)]
    );
    assert_eq!(
        item.groups[1].modifiers[0],
        ModifierOption {
            modifier: demo_id(0x205),
            name: "Shot".into(),
            prefix: Prefix::Extra,
            price: usd(95, "$0.95")
        }
    );
    assert_eq!(item.groups[0].modifiers[1].price, usd(75, "$0.75"));
    let bagel = register.item(demo_id(0x407)).unwrap();
    assert_eq!(bagel.groups[0].min, 1);
    assert_eq!(bagel.groups[0].modifiers[2].prefix, Prefix::No);
}

#[test]
fn every_job_enumeration_crosses_as_itself() {
    use keel_runtime as rt;
    for (state, ffi) in [
        (rt::JobState::Queued, JobState::Queued),
        (rt::JobState::Sending, JobState::Sending),
        (rt::JobState::Unconfirmed, JobState::Unconfirmed),
    ] {
        let view = rt::JobView {
            job: Id::parse(&demo_id(1)).unwrap(),
            kind: rt::JobKind::Receipt,
            state,
            attempts: 7,
        };
        assert_eq!(
            Job::from(&view),
            Job { job: demo_id(1), kind: JobKind::Receipt, state: ffi, attempts: 7 }
        );
    }
    assert_eq!(JobKind::from(rt::JobKind::Drawer), JobKind::Drawer);
    for confirm in [Confirm::Printed, Confirm::Processed, Confirm::Nothing] {
        assert_eq!(Confirm::from(keel_doc::Confirm::from(confirm)), confirm);
    }
    assert_eq!(keel_doc::Confirm::from(Confirm::Processed), keel_doc::Confirm::Processed);
    assert_eq!(keel_doc::Confirm::from(Confirm::Nothing), keel_doc::Confirm::Nothing);
    for (ffi, runtime) in [
        (Resolution::Printed, rt::Resolution::Printed),
        (Resolution::Again, rt::Resolution::Again),
        (Resolution::Dismiss, rt::Resolution::Dismiss),
    ] {
        assert_eq!(rt::Resolution::from(ffi), runtime);
    }
}

#[test]
fn a_printer_without_a_cutter_gets_feeds_and_its_confirmation_is_the_jobs() {
    let scratch = Scratch::new();
    let register = Register::open(config(&scratch)).unwrap();
    register.sign_in(demo_id(0x501)).unwrap();
    let order = register.start_order(Mode::Takeout).unwrap().order;
    register.add_item(order.clone(), demo_id(0x401), vec![], 1).unwrap();
    register.pay_cash(order, 1_000).unwrap();
    register
        .set_printer(Printer { columns: 32, cutter: false, confirm: Confirm::Processed })
        .unwrap();
    let drawer = register.next_print_job().unwrap().unwrap();
    assert_eq!(drawer.confirm, Confirm::Processed);
    assert!(drawer.bytes.ends_with(&escpos::DRAWER_KICK));
    register.print_outcome(drawer.job, Outcome::Printed { paper_low: false }).unwrap();
    let receipt = register.next_print_job().unwrap().unwrap();
    assert!(receipt.bytes.ends_with(b"\n\n\n\n\n") && !receipt.bytes.ends_with(&escpos::CUT));
    // A process ID that isn't four printable bytes is refused, not taken for an unreachable printer.
    for process_id in [vec![1, 2, 3], vec![1, 2, 3, 4], vec![b'0', b'0', b'0', 0x7F]] {
        let bad = PrintJob { process_id, ..receipt.clone() };
        let waits = Waits { status_ms: 10, word_ms: 10 };
        assert!(matches!(
            print_tcp("127.0.0.1".into(), 9, bad, waits),
            Err(KeelError::InvalidArgument { .. })
        ));
    }
    let bad = PrintJob { process_id: vec![1, 2, 3], ..receipt.clone() };
    assert!(matches!(
        print_tcp("127.0.0.1".into(), 9, bad, Waits { status_ms: 10, word_ms: 10 }),
        Err(KeelError::InvalidArgument { .. })
    ));
    // The waits cross as themselves: a printer that answers its status but never its word gives
    // up after the word's wait, not the status's.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0_u8; 16384];
        let mut answered = 0;
        let mut all = Vec::new();
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
            let questions = all.windows(2).filter(|w| *w == [0x10, 0x04]).count();
            while answered < questions {
                stream.write_all(&[0x12]).unwrap();
                answered += 1;
            }
        }
    });
    let started = std::time::Instant::now();
    let outcome =
        print_tcp("127.0.0.1".into(), port, receipt, Waits { status_ms: 3_000, word_ms: 300 })
            .unwrap();
    assert_eq!(outcome, Outcome::Unconfirmed { reason: Unconfirmed::NoWord });
    assert!(started.elapsed() < std::time::Duration::from_millis(2_500), "{:?}", started.elapsed());
    server.join().unwrap();
}

/// A printer that answers each status question after `delay`, and `GS r 1` with its paper's
/// state, but never `GS ( H`.
fn slow_printer(delay: std::time::Duration) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (mut all, mut buf) = (Vec::new(), [0_u8; 16384]);
        let (mut answered, mut processed) = (0, 0);
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
            let questions = all.windows(2).filter(|w| *w == [0x10, 0x04]).count();
            while answered < questions {
                thread::sleep(delay);
                stream.write_all(&[0x12]).unwrap();
                answered += 1;
            }
            let asked = all.windows(3).filter(|w| *w == escpos::PROCESSED).count();
            while processed < asked {
                stream.write_all(&[0x00]).unwrap();
                processed += 1;
            }
        }
    });
    (port, server)
}

#[test]
fn print_tcp_waits_and_confirms_as_the_job_says() {
    let scratch = Scratch::new();
    let register = Register::open(config(&scratch)).unwrap();
    register.sign_in(demo_id(0x501)).unwrap();
    let order = register.start_order(Mode::Takeout).unwrap().order;
    register.add_item(order.clone(), demo_id(0x401), vec![], 1).unwrap();
    register.pay_cash(order, 1_000).unwrap();
    register
        .set_printer(Printer { columns: 48, cutter: true, confirm: Confirm::Processed })
        .unwrap();
    let job = register.next_print_job().unwrap().unwrap();
    // The printer answers `GS r 1`: printed, as the job asked for that word.
    let (port, server) = slow_printer(std::time::Duration::ZERO);
    let waits = Waits { status_ms: 2_000, word_ms: 2_000 };
    let outcome = print_tcp("127.0.0.1".into(), port, job.clone(), waits).unwrap();
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
    server.join().unwrap();
    // Each status takes half a second: within the status's wait, past the word's.
    let (port, server) = slow_printer(std::time::Duration::from_millis(500));
    let waits = Waits { status_ms: 2_000, word_ms: 200 };
    let outcome = print_tcp("127.0.0.1".into(), port, job, waits).unwrap();
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
    server.join().unwrap();
}

#[test]
fn a_new_device_opens_a_register_of_its_own() {
    let (first, second) = (new_device().unwrap(), new_device().unwrap());
    // Every part drawn on its own.
    assert_ne!(first.device, second.device);
    assert_ne!(first.signing_key, second.signing_key);
    assert_ne!(first.store_key, second.store_key);
    assert_ne!(first.signing_key, first.store_key);
    assert_eq!((first.signing_key.len(), first.store_key.len()), (32, 32));
    let scratch = Scratch::new();
    let register = Register::open(RegisterConfig {
        store_key: first.store_key.clone(),
        device: first.device.clone(),
        signing_key: first.signing_key.clone(),
        ..config(&scratch)
    })
    .unwrap();
    register.sign_in(domain::demo().unwrap().data().team[0].id.to_string()).unwrap();
    assert!(register.start_order(Mode::Takeout).is_ok());
}

#[test]
fn the_team_and_todays_orders_cross_the_boundary() {
    let scratch = Scratch::new();
    let register = Register::open(config(&scratch)).unwrap();
    let team = register.team().unwrap();
    assert_eq!(
        team,
        vec![
            Member { id: demo_id(0x501), name: "Alex".to_owned() },
            Member { id: demo_id(0x502), name: "Sam".to_owned() },
        ]
    );
    register.sign_in(team[1].id.clone()).unwrap();
    let first = register.start_order(Mode::Takeout).unwrap();
    register.add_item(first.order.clone(), demo_id(0x409), vec![], 1).unwrap();
    register.pay_cash(first.order.clone(), 1_000).unwrap();
    let second = register.start_order(Mode::DineIn).unwrap();
    let today: Vec<(String, TicketState)> =
        register.orders_today().unwrap().into_iter().map(|t| (t.order, t.state)).collect();
    assert_eq!(today, vec![(first.order, TicketState::Closed), (second.order, TicketState::Open)]);
}

#[test]
fn cash_offers_and_amounts_cross_the_boundary() {
    let scratch = Scratch::new();
    let register = Register::open(config(&scratch)).unwrap();
    register.sign_in(demo_id(0x501)).unwrap();
    let order = register.start_order(Mode::Takeout).unwrap().order;
    register.add_item(order.clone(), demo_id(0x409), vec![], 1).unwrap();
    let offers: Vec<(i64, String)> =
        register.cash_offers(order).unwrap().into_iter().map(|a| (a.minor, a.text)).collect();
    assert_eq!(
        offers,
        [(218, "$2.18"), (300, "$3.00"), (500, "$5.00"), (1_000, "$10.00"), (2_000, "$20.00")]
            .map(|(minor, text)| (minor, text.to_owned()))
    );
    let typed = register.amount(-5).unwrap();
    assert_eq!((typed.minor, typed.currency.as_str(), typed.text.as_str()), (-5, "USD", "-$0.05"));
    assert!(matches!(
        register.cash_offers("nope".to_owned()),
        Err(KeelError::InvalidArgument { .. })
    ));
}

/// A register for `kit`'s device, in `scratch`.
fn kit_register(kit: &keel_provision::Kit, scratch: &Scratch) -> Arc<Register> {
    Register::open(RegisterConfig {
        path: scratch.0.join("store.db").to_string_lossy().into_owned(),
        store_key: kit.store_key.to_vec(),
        device: kit.device.to_string(),
        signing_key: kit.signing_key.to_vec(),
        profile: kit.profile.clone(),
        locale: "en-US".to_owned(),
    })
    .unwrap()
}

fn sync_kit(kit: &keel_provision::Kit) -> SyncKit {
    SyncKit {
        roster: kit.roster.clone(),
        ca: kit.ca.clone(),
        certificate: kit.certificate.clone(),
        tls_key: kit.tls_key.clone(),
    }
}

#[test]
fn a_hub_and_a_register_sync_through_the_bindings() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let provisioned = keel_provision::provision(&keel_provision::Spec {
        profile: demo_profile(),
        hub_host: "127.0.0.1".to_owned(),
        hub_port: core::num::NonZeroU16::new(port).unwrap(),
        hub_name: "hub.keel-cafe.local".to_owned(),
        cloud: None,
        devices: vec![
            keel_provision::DeviceSpec {
                name: "Back office".to_owned(),
                hub: core::num::NonZeroU8::new(1),
            },
            keel_provision::DeviceSpec { name: "Front counter".to_owned(), hub: None },
        ],
    })
    .unwrap();
    let kit = |name: &str| provisioned.kits.iter().find(|kit| kit.name == name).unwrap();
    let (hub_scratch, register_scratch) = (Scratch::new(), Scratch::new());
    let hub = kit_register(kit("Back office"), &hub_scratch);
    let register = kit_register(kit("Front counter"), &register_scratch);
    assert_eq!(register.sync_status().unwrap(), None, "not joined yet");
    assert!(matches!(register.sync_step(0), Err(KeelError::InvalidArgument { .. })));
    hub.start_sync(sync_kit(kit("Back office"))).unwrap();
    register.start_sync(sync_kit(kit("Front counter"))).unwrap();
    assert!(matches!(
        register.start_sync(sync_kit(kit("Front counter"))),
        Err(KeelError::InvalidArgument { .. })
    ));
    // The roster's hub listens; the register connects.
    assert!(hub.sync_status().unwrap().unwrap().listening);
    assert!(!register.sync_status().unwrap().unwrap().listening);
    // The profile's version, the provisioned one's, in hexadecimal; nothing written, nothing
    // backed up.
    let status = register.sync_status().unwrap().unwrap();
    let demo = keel_domain::profile::demo().unwrap();
    let version = keel_domain::publication::version(&demo);
    assert_eq!(status.profile_version, version.to_hex());
    assert_eq!(register.profile_version(), version.to_hex());
    assert_eq!(status.profile_version.len(), 64);
    assert_eq!((status.backed_up, status.head), (0, 0));
    let turn = |until: &dyn Fn() -> bool| {
        let started = std::time::Instant::now();
        while !until() {
            assert!(started.elapsed() < core::time::Duration::from_secs(30), "within 30 s");
            hub.sync_step(10).unwrap();
            register.sync_step(10).unwrap();
        }
    };
    turn(&|| {
        let status = register.sync_status().unwrap().unwrap();
        status.settled && status.hub_reachable
    });
    register.sign_in(demo_id(0x501)).unwrap();
    let order = register.start_order(Mode::Takeout).unwrap().order;
    register.add_item(order.clone(), demo_id(0x409), vec![], 1).unwrap();
    register.pay_cash(order.clone(), 1_000).unwrap();
    // The hub holds the sale, closed.
    turn(&|| {
        hub.orders_today()
            .unwrap()
            .iter()
            .any(|ticket| ticket.order == order && ticket.state == TicketState::Closed)
    });
    assert!(hub.sync_status().unwrap().unwrap().serving);
}

#[test]
fn a_register_that_hasnt_settled_says_so() {
    let error = KeelError::from(keel_runtime::RuntimeError::NotSettled);
    assert_eq!(error, KeelError::NotSettled);
}

#[test]
fn the_roster_s_most_preferred_hub_listens() {
    let provisioned = keel_provision::provision(&keel_provision::Spec {
        profile: demo_profile(),
        hub_host: "127.0.0.1".to_owned(),
        hub_port: core::num::NonZeroU16::new(7443).unwrap(),
        hub_name: "hub.keel-cafe.local".to_owned(),
        cloud: None,
        devices: [("A", Some(2)), ("B", Some(5)), ("C", Some(5)), ("D", None)]
            .map(|(name, hub)| keel_provision::DeviceSpec {
                name: name.to_owned(),
                hub: hub.and_then(core::num::NonZeroU8::new),
            })
            .to_vec(),
    })
    .unwrap();
    // The highest priority, and of the two at 5 the lower identifier: the roster lists its devices
    // in the ascending order of their identifiers.
    let expected = provisioned
        .roster
        .data()
        .devices
        .iter()
        .find(|device| device.hub.map(core::num::NonZeroU8::get) == Some(5))
        .unwrap()
        .id;
    assert_eq!(preferred_hub(&provisioned.roster), Some(expected));
}

/// Found in review: a standby the roster lets host the hub, but which doesn't listen at its
/// address, took the role when the hub fell silent, though no device could reach it.
#[test]
fn a_standby_that_isnt_listening_never_takes_the_hubs_role() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let provisioned = keel_provision::provision(&keel_provision::Spec {
        profile: demo_profile(),
        hub_host: "127.0.0.1".to_owned(),
        hub_port: core::num::NonZeroU16::new(port).unwrap(),
        hub_name: "hub.keel-cafe.local".to_owned(),
        cloud: None,
        devices: [("Hub", Some(5)), ("Standby", Some(2)), ("Register", None)]
            .map(|(name, hub)| keel_provision::DeviceSpec {
                name: name.to_owned(),
                hub: hub.and_then(core::num::NonZeroU8::new),
            })
            .to_vec(),
    })
    .unwrap();
    let kit = |name: &str| provisioned.kits.iter().find(|kit| kit.name == name).unwrap();
    let scratches = [Scratch::new(), Scratch::new(), Scratch::new()];
    let hub = kit_register(kit("Hub"), &scratches[0]);
    let standby = kit_register(kit("Standby"), &scratches[1]);
    let register = kit_register(kit("Register"), &scratches[2]);
    hub.start_sync(sync_kit(kit("Hub"))).unwrap();
    standby.start_sync(sync_kit(kit("Standby"))).unwrap();
    register.start_sync(sync_kit(kit("Register"))).unwrap();
    assert!(!standby.sync_status().unwrap().unwrap().listening);
    let started = std::time::Instant::now();
    loop {
        for device in [&hub, &standby, &register] {
            device.sync_step(5).unwrap();
        }
        let both = [&standby, &register].map(|device| device.sync_status().unwrap().unwrap());
        if hub.sync_status().unwrap().unwrap().serving
            && both.iter().all(|s| s.settled && s.hub_reachable)
        {
            break;
        }
        assert!(started.elapsed() < core::time::Duration::from_secs(30), "all three joined");
    }
    // The hub goes away. Long past three silent heartbeats, the standby still doesn't serve.
    drop(hub);
    let gone = std::time::Instant::now();
    while gone.elapsed() < core::time::Duration::from_secs(8) {
        standby.sync_step(5).unwrap();
        register.sync_step(5).unwrap();
    }
    let status = standby.sync_status().unwrap().unwrap();
    assert!(!status.serving && !status.is_hub, "{status:?}");
}

#[test]
fn a_sync_step_takes_in_what_is_waiting_within_its_budget() {
    // Ten events waiting: eleven turns, the last finding nothing; only the first waits.
    let mut left = 10_u32;
    let mut waits = Vec::new();
    let turns = drain(
        |wait| -> Result<bool, ()> {
            waits.push(wait);
            Ok(left.checked_sub(1).map(|rest| left = rest).is_some())
        },
        core::time::Duration::from_millis(20),
    )
    .unwrap();
    assert_eq!(turns, 11);
    assert_eq!(waits[0], core::time::Duration::from_millis(20));
    assert!(waits[1..].iter().all(core::time::Duration::is_zero));
    // Always more: no more than the budget's turns.
    assert_eq!(
        drain(|_| -> Result<bool, ()> { Ok(true) }, core::time::Duration::ZERO).unwrap(),
        64
    );
    // Nothing: one turn. A failure ends it.
    assert_eq!(
        drain(|_| -> Result<bool, ()> { Ok(false) }, core::time::Duration::ZERO).unwrap(),
        1
    );
    assert_eq!(
        drain(|_| -> Result<bool, &str> { Err("damaged") }, core::time::Duration::ZERO),
        Err("damaged")
    );
}
