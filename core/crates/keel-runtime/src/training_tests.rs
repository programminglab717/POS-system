//! Training and languages (ADR-0038): a training register never syncs, prints its banner on
//! everything and kicks no drawer; the member signed in has their language; a station's
//! tickets print in its.

use core::num::NonZeroU32;

use keel_doc::Columns;
use keel_doc::escpos::{DRAWER_KICK, plain_text};
use keel_domain::order::Mode;
use keel_domain::profile::{ProfileData, demo};
use keel_types::Locale;

use crate::kitchen_tests::{bagel, kitchen_jobs, printed_profile};
use crate::tests::{Scratch, demo_with, id, register, usd};
use crate::{JobKind, RuntimeError};

const PRINTER: keel_doc::PrinterProfile = keel_doc::PrinterProfile::EPSON_80MM;

#[test]
fn a_training_register_prints_its_banner_kicks_no_drawer_and_never_syncs() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.start_training().unwrap();
    assert!(runtime.is_training());
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
    runtime.pay_cash(order, usd(500)).unwrap();
    runtime.set_printer(PRINTER);
    let mut printed = Vec::new();
    while let Some(job) = runtime.next_print_job().unwrap() {
        runtime.print_outcome(job.job, keel_print::Outcome::Printed { paper_low: false }).unwrap();
        printed.push(job);
    }
    assert!(printed.iter().any(|job| job.kind == JobKind::Receipt), "the receipt prints");
    for job in &printed {
        let text = String::from_utf8_lossy(&job.bytes);
        assert!(text.matches("TRAINING - NOT A RECEIPT").count() >= 2, "{text}");
        assert!(!job.bytes.windows(DRAWER_KICK.len()).any(|bytes| bytes == DRAWER_KICK), "no kick");
    }
    let roster = crate::publication_tests::roster();
    assert!(matches!(runtime.start_sync(roster), Err(RuntimeError::Training)));
}

#[test]
fn a_register_that_syncs_doesnt_train() {
    let scratch = Scratch::new();
    let mut runtime = register(&scratch, demo().unwrap());
    runtime.start_sync(crate::publication_tests::roster()).unwrap();
    assert!(matches!(runtime.start_training(), Err(RuntimeError::Training)));
    assert!(!runtime.is_training());
}

#[test]
fn the_member_signed_in_has_their_language_or_the_registers() {
    let scratch = Scratch::new();
    let profile = demo_with(|data: &mut ProfileData| {
        data.team[1].language = Some(Locale::ES_US);
    });
    let mut runtime = register(&scratch, profile);
    assert_eq!(runtime.language(), None, "no one signed in");
    runtime.sign_in(id(0x502), "2468").unwrap();
    assert_eq!(runtime.language(), Some(Locale::ES_US));
    runtime.sign_out();
    runtime.sign_in(id(0x501), "1234").unwrap();
    assert_eq!(runtime.language(), Some(Locale::EN_US), "the register's");
}

#[test]
fn a_stations_tickets_print_in_its_language() {
    let scratch = Scratch::new();
    let mut profile = printed_profile();
    let mut data = profile.data().clone();
    data.kitchen.as_mut().unwrap().stations[1].language = Some(Locale::ES_US);
    profile = keel_domain::profile::Profile::new(data).unwrap();
    let mut runtime = register(&scratch, profile);
    runtime.sign_in(id(0x502), "2468").unwrap();
    let order = runtime.start_order(Mode::DineIn).unwrap().order;
    runtime.add_item(order, id(0x407), &bagel(), NonZeroU32::MIN).unwrap();
    runtime.send(order).unwrap();
    let jobs = kitchen_jobs(&runtime);
    assert_eq!(jobs.len(), 1);
    let text = &jobs[0].1;
    for words in ["Orden", "PARA COMER AQU", "AL\u{c9}RGENOS: gluten, leche"] {
        assert!(text.contains(words), "{words:?} in\n{text}");
    }
    assert!(!text.contains("DINE IN") && !text.contains("ALLERGENS"), "{text}");
    let line = runtime.ticket(order).unwrap().lines[0].line;
    let reason = keel_domain::order::Reason {
        code: keel_domain::codec::ReasonCode::new("customer").unwrap(),
        note: None,
    };
    runtime.void_line(order, line, reason).unwrap();
    let jobs = kitchen_jobs(&runtime);
    assert!(jobs[1].1.contains("ANULADO"), "{}", jobs[1].1);
    let _ = (plain_text, Columns::PAPER_80MM);
}
