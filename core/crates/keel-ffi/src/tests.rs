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

use crate::register::choices;
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
    assert!(matches!(Register::open(wrong), Err(KeelError::Device { .. })));
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
        let outcome = print_tcp("127.0.0.1".into(), port, job.clone(), waits);
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
    let outcome = print_tcp("127.0.0.1".into(), closed, job.clone(), waits);
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
