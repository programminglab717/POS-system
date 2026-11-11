//! Known answers for print jobs (ADR-0024): the demo café's receipt, and each job's way from
//! enqueued to printed, retried, unconfirmed and resolved.

use core::num::NonZeroU32;
use core::time::Duration;

use keel_doc::escpos::{plain_text, render};
use keel_doc::{Block, Columns, Doc, PrinterProfile};
use keel_domain::order::{Mode, Order};
use keel_domain::profile::{Choice, demo};
use keel_print::{NotSent, Outcome, Unconfirmed};
use keel_types::{Id, Locale};

use crate::tests::{Scratch, TestRuntime, big_latte, id, register_at, signed_in, usd};
use crate::{Job, JobKind, JobState, Resolution, RuntimeError};

const PRINTER: PrinterProfile = PrinterProfile::EPSON_80MM;
const PRINTED: Outcome = Outcome::Printed { paper_low: false };

/// A sale of a large latte with its choices and two bagels with butter, paid with $20.
fn sale(runtime: &mut TestRuntime) -> Id<Order> {
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x403), &big_latte(), NonZeroU32::MIN).unwrap();
    let butter = [Choice::of(id(0x20b))];
    runtime.add_item(order, id(0x407), &butter, NonZeroU32::new(2).unwrap()).unwrap();
    runtime.pay_cash(order, usd(2000)).unwrap();
    order
}

/// The document of `job`, as the outbox holds it.
fn doc_of(runtime: &TestRuntime, job: Id<Job>) -> Doc {
    let queued = runtime.store().effect(&job.to_bytes()).unwrap().unwrap();
    Doc::decode(&queued.effect.payload).unwrap()
}

/// The jobs' kinds and states.
fn states(runtime: &TestRuntime) -> Vec<(JobKind, JobState)> {
    runtime.print_jobs().unwrap().into_iter().map(|job| (job.kind, job.state)).collect()
}

fn later(runtime: &mut TestRuntime, wait: Duration) {
    runtime.clock().advance(wait).unwrap();
}

const RECEIPT: &str = "       Keel Café
                12 Harbor Street
               Brooklyn, NY 11201
------------------------------------------------
Order XXXXXX                    2026-10-02 09:00
Cashier: Alex
------------------------------------------------
Latte (Large)                              $8.50
  Oat milk                                 $0.75
  2 × Extra Shot                           $1.90
  Vanilla
  Caramel                                  $0.60
2 × Bagel                                  $6.50
  Butter                                   $0.50
------------------------------------------------
Subtotal                                  $15.00
NYC sales tax                              $1.33
TOTAL                                     $16.33
Cash                                      $20.00
Change                                     $3.67
------------------------------------------------
                   Thank you!


";

#[test]
fn a_cash_sale_enqueues_the_drawer_then_its_receipt() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let order = sale(&mut runtime);
    assert_eq!(
        states(&runtime),
        [(JobKind::Drawer, JobState::Queued), (JobKind::Receipt, JobState::Queued)]
    );
    let jobs = runtime.print_jobs().unwrap();
    assert_eq!(doc_of(&runtime, jobs[0].job).blocks(), [Block::DrawerKick]);
    let text = order.to_string();
    let short = text[text.len() - 6..].to_uppercase();
    let receipt = doc_of(&runtime, jobs[1].job);
    assert_eq!(plain_text(&receipt, Columns::PAPER_80MM), RECEIPT.replace("XXXXXX", &short));
    assert_eq!(receipt.blocks().last(), Some(&Block::Cut));
    // And as bytes, for an Epson on 80 mm paper, built here from the text: the commands that
    // start every job, the name large and bold, the total tall and bold, each line in
    // Windows-1252 and a line feed, then the cut.
    let text = RECEIPT.replace("XXXXXX", &short);
    let mut expected = vec![0x1B, 0x40, 0x1B, 0x74, 16, 0x1B, 0x52, 0, 0x1C, 0x2E, 0x1B, 0x61, 0];
    for (at, line) in text.lines().enumerate() {
        match (at, line.starts_with("TOTAL")) {
            (0, _) => expected.extend([0x1B, 0x45, 1, 0x1D, 0x21, 0x11]),
            (1, _) => expected.extend([0x1B, 0x45, 0, 0x1D, 0x21, 0x00]),
            (_, true) => expected.extend([0x1B, 0x45, 1, 0x1D, 0x21, 0x01]),
            _ if text.lines().nth(at - 1).is_some_and(|before| before.starts_with("TOTAL")) => {
                expected.extend([0x1B, 0x45, 0, 0x1D, 0x21, 0x00]);
            }
            _ => {}
        }
        expected.extend(keel_doc::text::encode(line));
        expected.push(0x0A);
    }
    expected.extend([0x1D, 0x56, 66, 0]);
    assert_eq!(render(&receipt, &PRINTER), expected);
}

#[test]
fn a_sale_that_owes_nothing_prints_its_receipt_and_opens_no_drawer() {
    let scratch = Scratch::new();
    let profile = crate::tests::demo_with(|data| crate::tests::variant(data, 0x409).price = usd(0));
    let mut runtime = crate::tests::register(&scratch, profile);
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x409), &[], NonZeroU32::MIN).unwrap();
    runtime.pay_cash(order, usd(0)).unwrap();
    assert_eq!(states(&runtime), [(JobKind::Receipt, JobState::Queued)]);
}

#[test]
fn jobs_wait_for_a_printer_and_go_to_it_one_at_a_time_in_order() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    sale(&mut runtime);
    assert_eq!(runtime.next_print_job().unwrap(), None);
    runtime.set_printer(PRINTER);
    let drawer = runtime.next_print_job().unwrap().unwrap();
    assert_eq!(drawer.kind, JobKind::Drawer);
    assert_eq!(drawer.bytes, render(&doc_of(&runtime, drawer.job), &PRINTER));
    // Its process ID comes from the job and its attempt, so no two jobs share one.
    let process =
        |job, attempts| keel_doc::escpos::process_id(crate::print::process_number(job, attempts));
    assert_eq!(drawer.process_id, process(drawer.job, 1));
    // One at a time: the receipt waits while the drawer's job is with the driver.
    assert_eq!(runtime.next_print_job().unwrap(), None);
    assert_eq!(
        states(&runtime),
        [(JobKind::Drawer, JobState::Sending), (JobKind::Receipt, JobState::Queued)]
    );
    runtime.print_outcome(drawer.job, PRINTED).unwrap();
    let receipt = runtime.next_print_job().unwrap().unwrap();
    assert_eq!(receipt.kind, JobKind::Receipt);
    assert_ne!(receipt.process_id, drawer.process_id);
    runtime.print_outcome(receipt.job, Outcome::Printed { paper_low: true }).unwrap();
    assert_eq!(states(&runtime), []);
    assert_eq!(runtime.next_print_job().unwrap(), None);
    // Its outcome recorded, a job is no longer the driver's.
    assert!(matches!(
        runtime.print_outcome(receipt.job, PRINTED),
        Err(RuntimeError::UnknownJob(_))
    ));
}

#[test]
fn a_job_not_sent_waits_longer_each_time() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    sale(&mut runtime);
    runtime.set_printer(PRINTER);
    for (attempt, wait) in [(1_u32, 2_u64), (2, 4), (3, 8), (4, 16), (5, 32), (6, 60), (7, 60)] {
        let job = runtime.next_print_job().unwrap().unwrap();
        assert_eq!(job.kind, JobKind::Drawer, "attempt {attempt}");
        runtime.print_outcome(job.job, Outcome::NotSent(NotSent::PaperOut)).unwrap();
        assert_eq!(runtime.print_jobs().unwrap()[0].attempts, attempt);
        // Not due a millisecond early; and the receipt waits behind it, though due.
        later(&mut runtime, Duration::from_millis(wait * 1000 - 1));
        let next = runtime.next_print_job().unwrap().unwrap();
        assert_eq!(next.kind, JobKind::Receipt);
        runtime.print_outcome(next.job, Outcome::NotSent(NotSent::Offline)).unwrap();
        later(&mut runtime, Duration::from_millis(1));
    }
}

#[test]
fn an_unconfirmed_job_is_never_sent_again_by_itself() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    sale(&mut runtime);
    runtime.set_printer(PRINTER);
    let drawer = runtime.next_print_job().unwrap().unwrap();
    runtime.print_outcome(drawer.job, Outcome::Unconfirmed(Unconfirmed::NoWord)).unwrap();
    assert_eq!(states(&runtime)[0], (JobKind::Drawer, JobState::Unconfirmed));
    // The receipt goes on; the drawer's job never comes back, however long.
    let receipt = runtime.next_print_job().unwrap().unwrap();
    assert_eq!(receipt.kind, JobKind::Receipt);
    runtime.print_outcome(receipt.job, Outcome::Unconfirmed(Unconfirmed::Closed)).unwrap();
    later(&mut runtime, Duration::from_secs(3600));
    assert_eq!(runtime.next_print_job().unwrap(), None);
    assert_eq!(
        states(&runtime),
        [(JobKind::Drawer, JobState::Unconfirmed), (JobKind::Receipt, JobState::Unconfirmed)]
    );
    // Staff: the drawer opened; print the receipt again.
    assert_eq!(runtime.resolve(drawer.job, Resolution::Printed).unwrap(), None);
    let again = runtime.resolve(receipt.job, Resolution::Again).unwrap().unwrap();
    assert_eq!(states(&runtime), [(JobKind::Receipt, JobState::Queued)]);
    let banner = crate::receipt::reprint_banner(Locale::EN_US);
    let original = doc_of(&runtime, receipt.job);
    let reprint = doc_of(&runtime, again);
    assert_eq!(reprint.blocks().first(), Some(&banner));
    assert_eq!(reprint.blocks().len(), original.blocks().len() + 2);
    // A job that isn't unconfirmed can't be resolved.
    assert!(matches!(
        runtime.resolve(again, Resolution::Dismiss),
        Err(RuntimeError::NotUnconfirmed(_))
    ));
    assert!(matches!(
        runtime.resolve(receipt.job, Resolution::Printed),
        Err(RuntimeError::NotUnconfirmed(_))
    ));
    assert!(matches!(
        runtime.resolve(id(0x999), Resolution::Printed),
        Err(RuntimeError::UnknownJob(_))
    ));
    // The reprint, unconfirmed and printed again, is marked once.
    let job = runtime.next_print_job().unwrap().unwrap();
    runtime.print_outcome(job.job, Outcome::Unconfirmed(Unconfirmed::NoWord)).unwrap();
    let twice = runtime.resolve(job.job, Resolution::Again).unwrap().unwrap();
    assert_eq!(doc_of(&runtime, twice), reprint);
    let job = runtime.next_print_job().unwrap().unwrap();
    runtime.print_outcome(job.job, Outcome::Unconfirmed(Unconfirmed::NoWord)).unwrap();
    assert_eq!(runtime.resolve(job.job, Resolution::Dismiss).unwrap(), None);
    assert_eq!(states(&runtime), []);
}

#[test]
fn a_job_with_the_driver_when_the_runtime_stops_is_unconfirmed_when_it_opens() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    sale(&mut runtime);
    runtime.set_printer(PRINTER);
    let drawer = runtime.next_print_job().unwrap().unwrap();
    drop(runtime);
    let mut runtime = register_at(&scratch, demo().unwrap(), "2026-10-02T13:05:00Z");
    runtime.set_printer(PRINTER);
    assert_eq!(
        states(&runtime),
        [(JobKind::Drawer, JobState::Unconfirmed), (JobKind::Receipt, JobState::Queued)]
    );
    assert!(matches!(runtime.print_outcome(drawer.job, PRINTED), Err(RuntimeError::UnknownJob(_))));
    assert_eq!(runtime.next_print_job().unwrap().unwrap().kind, JobKind::Receipt);
}

#[test]
fn a_reprint_is_the_receipt_as_it_closed_marked_above_and_below() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let order = sale(&mut runtime);
    let original = runtime.print_jobs().unwrap()[1].job;
    // Reprinted later, by someone else, it still shows the cashier and time of the close.
    later(&mut runtime, Duration::from_secs(600));
    runtime.sign_in(id(0x502), "2468").unwrap();
    let job = runtime.reprint(order).unwrap();
    let banner = crate::receipt::reprint_banner(Locale::EN_US);
    let mut expected = vec![banner.clone()];
    let blocks = doc_of(&runtime, original).blocks().to_vec();
    expected.extend_from_slice(&blocks[..blocks.len() - 2]);
    expected.push(banner);
    expected.extend_from_slice(&blocks[blocks.len() - 2..]);
    assert_eq!(doc_of(&runtime, job).blocks(), expected);
    // Reprinting again reprints the original, not the reprint.
    let again = runtime.reprint(order).unwrap();
    assert_eq!(doc_of(&runtime, again), doc_of(&runtime, job));
    // An open order has no receipt.
    let open = runtime.start_order(Mode::Takeout).unwrap().order;
    assert!(matches!(runtime.reprint(open), Err(RuntimeError::NoReceipt(_))));
}

#[test]
fn a_spanish_receipt_says_so_in_spanish() {
    let scratch = Scratch::new();
    let mut runtime = crate::tests::register_with_locale(&scratch, Locale::ES_US);
    runtime.sign_in(id(0x501), "1234").unwrap();
    sale(&mut runtime);
    let job = runtime.print_jobs().unwrap()[1].job;
    let text = plain_text(&doc_of(&runtime, job), Columns::PAPER_80MM);
    for words in ["Orden ", "Cajero: Alex", "Efectivo", "Cambio", "¡Gracias!", "2 × Extra Shot"] {
        assert!(text.contains(words), "{words} isn't in\n{text}");
    }
    let order = runtime.store().orders(keel_store::OrderState::Closed).unwrap()[0].id;
    let reprint = runtime.reprint(order).unwrap();
    assert!(plain_text(&doc_of(&runtime, reprint), Columns::PAPER_80MM).contains("REIMPRESIÓN"));
}

/// Found in review: a receipt past a document's 1,000 blocks failed the closing write, so a large
/// order couldn't be paid; and one just under it couldn't be reprinted.
#[test]
fn a_large_order_is_paid_and_its_receipt_says_how_many_lines_it_leaves_out() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    for _ in 0..200 {
        runtime.add_item(order, id(0x403), &big_latte(), NonZeroU32::MIN).unwrap();
    }
    runtime.pay_cash(order, usd(1_000_000)).unwrap();
    let job = runtime.print_jobs().unwrap()[1].job;
    let doc = doc_of(&runtime, job);
    assert!(doc.blocks().len() <= keel_doc::doc::MAX_BLOCKS - 2);
    let text = plain_text(&doc, Columns::PAPER_80MM);
    // 800 blocks of lines, five to a latte: 160 shown, 40 left out.
    assert!(text.contains("… 40 more lines"), "{text}");
    assert!(text.contains("TOTAL"));
    let reprint = runtime.reprint(order).unwrap();
    assert_eq!(doc_of(&runtime, reprint).blocks().len(), doc.blocks().len() + 2);
}

/// Found in review: a receipt whose encoding passed what an effect holds failed the sale too. It
/// is printed short instead.
#[test]
fn a_receipt_too_big_for_the_outbox_is_printed_short() {
    let scratch = Scratch::new();
    let profile = crate::tests::demo_with(|data| {
        let template = data.rules.taxes[0].clone();
        data.rules.taxes = (0..64_u64)
            .map(|n| keel_pricing::Tax {
                id: id(0x7000 + n),
                name: "税".repeat(200),
                rate: keel_types::Rate::from_basis_points(10),
                ..template.clone()
            })
            .collect();
        // A latte whose name takes 570 bytes, so that its lines fill their budget.
        data.catalog.items[1].name = keel_domain::codec::Name::new(&"珈".repeat(190)).unwrap();
    });
    let mut runtime = crate::tests::register(&scratch, profile);
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    for _ in 0..150 {
        runtime.add_item(order, id(0x403), &big_latte(), NonZeroU32::MIN).unwrap();
    }
    runtime.pay_cash(order, usd(1_000_000)).unwrap();
    let job = runtime.print_jobs().unwrap()[1].job;
    let text = plain_text(&doc_of(&runtime, job), Columns::PAPER_80MM);
    assert!(text.contains("The register shows this order in full."), "{text}");
    assert!(text.contains("TOTAL") && text.contains("Change"), "{text}");
}

/// Found in review: jobs enqueued just before the clock was set back waited for it to catch up,
/// so the drawer didn't open.
#[test]
fn jobs_the_clock_left_behind_are_due_at_once() {
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    sale(&mut runtime);
    runtime.set_printer(PRINTER);
    let back: keel_types::Timestamp = "2026-10-02T12:00:00Z".parse().unwrap();
    runtime.clock().set(back);
    let drawer = runtime.next_print_job().unwrap().unwrap();
    assert_eq!(drawer.kind, JobKind::Drawer);
    // Not sent, it waits its 2 s from the clock as it reads now, not from where it was.
    runtime.print_outcome(drawer.job, Outcome::NotSent(NotSent::Offline)).unwrap();
    let receipt = runtime.next_print_job().unwrap().unwrap();
    assert_eq!(receipt.kind, JobKind::Receipt);
    runtime.print_outcome(receipt.job, PRINTED).unwrap();
    assert_eq!(runtime.next_print_job().unwrap(), None);
    later(&mut runtime, Duration::from_secs(2));
    assert_eq!(runtime.next_print_job().unwrap().unwrap().job, drawer.job);
}

/// Found in review: a reprint was recognised only in the runtime's language, so after the locale
/// changed a reprint could be taken for the original and marked again.
#[test]
fn reprints_are_recognised_in_any_language() {
    let scratch = Scratch::new();
    let mut runtime = crate::tests::register_with_locale(&scratch, Locale::ES_US);
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = sale(&mut runtime);
    runtime.reprint(order).unwrap();
    drop(runtime);
    let mut runtime = crate::tests::register_with_locale(&scratch, Locale::EN_US);
    // A reprint is someone's, as every action is (ADR-0029).
    assert!(matches!(runtime.reprint(order), Err(RuntimeError::NotSignedIn)));
    runtime.sign_in(id(0x502), "2468").unwrap();
    let job = runtime.reprint(order).unwrap();
    let banner = |text: &str| Block::text(text, keel_doc::Align::Center, keel_doc::Style::LARGE);
    let (english, spanish) = (banner("REPRINT"), banner("REIMPRESIÓN"));
    let blocks = doc_of(&runtime, job).blocks().to_vec();
    assert_eq!(blocks.iter().filter(|b| **b == english || **b == spanish).count(), 2);
    assert_eq!(doc_of(&runtime, job).blocks()[0], crate::receipt::reprint_banner(Locale::EN_US));
}

/// Found building discounts: the receipt listed each line before its discounts and the subtotal
/// after them, and nothing between. Each discount now shows, under its line or above the
/// subtotal, as less (ADR-0030).
#[test]
fn a_receipt_shows_each_discount_as_less() {
    use keel_domain::codec::ReasonCode;
    use keel_domain::order::{DiscountValue, Reason};
    let scratch = Scratch::new();
    let mut runtime = signed_in(&scratch);
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let ticket = runtime.add_item(order, id(0x401), &[], NonZeroU32::new(2).unwrap()).unwrap();
    let reason = || Reason { code: ReasonCode::new("loyalty").unwrap(), note: None };
    let line = Some(ticket.lines[0].line);
    runtime.apply_discount(order, line, DiscountValue::Percent(1_000), reason()).unwrap();
    runtime.apply_discount(order, line, DiscountValue::Amount(usd(25)), reason()).unwrap();
    runtime.apply_discount(order, None, DiscountValue::Percent(1_250), reason()).unwrap();
    runtime.pay_cash(order, usd(1_000)).unwrap();
    let jobs = runtime.print_jobs().unwrap();
    let text = plain_text(&doc_of(&runtime, jobs[1].job), Columns::PAPER_80MM);
    // $6.50, less 10%, $0.65, less $0.25, is $5.60; less 12.5%, $0.70, $4.90, with $0.43 of tax.
    let from = text.find("2 × Espresso").unwrap();
    let to = text.find("TOTAL").unwrap();
    assert_eq!(
        &text[from..to],
        "2 × Espresso                               $6.50
  Discount 10%                            -$0.65
  Discount                                -$0.25
------------------------------------------------
Discount 12.5%                            -$0.70
Subtotal                                   $4.90
NYC sales tax                              $0.43
"
    );
}
