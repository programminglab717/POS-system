//! End to end (ADR-0024): a sale rung and paid in cash, its drawer kick and receipt handed to the
//! driver and sent over TCP to a fake printer, which answers as a ready Epson does; the bytes it
//! receives are the jobs', and the jobs are done.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::NonZeroU32;
use core::time::Duration;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

use keel_doc::PrinterProfile;
use keel_doc::escpos;
use keel_domain::order::Mode;
use keel_domain::profile::{Choice, demo};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_print::{Outcome, Waits, tcp};
use keel_runtime::{Identity, JobKind, Runtime};
use keel_store::StoreKey;
use keel_types::{Currency, Id, Locale, ManualClock, Money, SeededEntropy};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

/// A printer that takes one connection, answers every status as a ready printer and every
/// `GS ( H` with its process ID, and gives back everything else it was sent.
fn printer() -> (std::net::SocketAddr, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (mut all, mut job, mut buf) = (Vec::new(), Vec::new(), [0_u8; 4096]);
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => return job,
                Ok(n) => n,
            };
            all.extend_from_slice(&buf[..n]);
            while let Some(at) = all.windows(2).position(|w| w == [0x10, 0x04]) {
                if all.len() < at + 3 {
                    break;
                }
                job.extend(all.drain(..at + 3).take(at));
                stream.write_all(&[0x12]).unwrap();
            }
            let ask = [0x1D, 0x28, 0x48, 6, 0, 48, 48];
            if let Some(at) = all.windows(ask.len()).position(|w| w == ask)
                && all.len() >= at + 11
            {
                let process: [u8; 4] = all[at + 7..at + 11].try_into().unwrap();
                job.extend(all.drain(..at + 11));
                stream.write_all(&escpos::printed(process)).unwrap();
                return job;
            }
        }
    });
    (address, server)
}

#[test]
fn a_cash_sales_drawer_and_receipt_reach_the_printer() {
    let dir: PathBuf = std::env::temp_dir().join(format!("keel-end-to-end-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let identity = Identity {
        device: id::<Device>(0xd1),
        signer: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[7; 32]).unwrap(),
    };
    let mut runtime = Runtime::open(
        dir.join("store.db"),
        StoreKey::new(&mut [0x4b; 32]),
        identity,
        demo().unwrap(),
        ManualClock::new("2026-10-02T13:00:00Z".parse().unwrap()),
        SeededEntropy::new(1),
        SeededEntropy::new(2),
        Locale::EN_US,
    )
    .unwrap();
    runtime.sign_in(id(0x501), "1234").unwrap();
    let order = runtime.start_order(Mode::Takeout).unwrap().order;
    let oat = [Choice::of(id(0x202))];
    runtime.add_item(order, id(0x403), &oat, NonZeroU32::MIN).unwrap();
    runtime.pay_cash(order, Money::from_minor(1_000, Currency::USD)).unwrap();
    runtime.set_printer(PrinterProfile::EPSON_80MM);
    let waits = Waits { status: Duration::from_secs(2), word: Duration::from_secs(5) };
    let mut printed = Vec::new();
    while let Some(job) = runtime.next_print_job().unwrap() {
        let (address, server) = printer();
        let outcome = tcp::print(address, &job.bytes, job.confirm, job.process_id, waits);
        assert_eq!(outcome, Outcome::Printed { paper_low: false });
        let mut expected = job.bytes.clone();
        expected.extend(escpos::ask_printed(job.process_id));
        assert_eq!(server.join().unwrap(), expected);
        runtime.print_outcome(job.job, outcome).unwrap();
        printed.push((job.kind, job.bytes));
    }
    assert_eq!(
        printed.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        [JobKind::Drawer, JobKind::Receipt]
    );
    assert!(printed[0].1.ends_with(&escpos::DRAWER_KICK));
    let receipt = keel_doc::text::decode(&printed[1].1);
    assert!(receipt.contains("Latte (Large)                              $6.00"), "{receipt}");
    assert!(receipt.contains("Change                                     $3.47"), "{receipt}");
    assert!(runtime.print_jobs().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
