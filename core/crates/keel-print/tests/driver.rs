//! The driver against a scripted fake printer on TCP (ADR-0024, decision 10): a server that
//! answers the driver's questions as each test needs, records the job it is sent, and refuses,
//! resets mid-job or goes silent on cue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::time::Duration;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use keel_doc::Confirm;
use keel_doc::escpos::{self, Question};
use keel_print::{NotSent, Outcome, Unconfirmed, Waits, tcp};

/// What the fake printer does.
#[derive(Clone, Debug)]
struct Script {
    /// Its answer to `DLE EOT 1`, 2 and 4, or `None` to say nothing.
    printer: Option<u8>,
    offline: u8,
    paper: u8,
    /// How it answers the job's word.
    word: Word,
    /// It closes the connection once it has this many of the job's bytes.
    reset_after: Option<usize>,
    /// Bytes it sends before its word, as a printer with something else to say might.
    chatter: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Word {
    /// It answers `GS ( H` and `GS r 1`.
    Both,
    /// It answers only `GS r 1`, as a printer without `GS ( H`.
    ProcessedOnly,
    /// It answers nothing.
    Silent,
}

/// A ready printer: online, with paper, answering everything.
fn ready() -> Script {
    Script {
        printer: Some(0x12),
        offline: 0x12,
        paper: 0x12,
        word: Word::Both,
        reset_after: None,
        chatter: vec![],
    }
}

struct Fake {
    address: SocketAddr,
    job: Arc<Mutex<Vec<u8>>>,
    server: JoinHandle<()>,
}

/// A fake printer that takes one connection and plays `script` on it.
fn fake(script: Script) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let job = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&job);
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0_u8; 4096];
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => {
                    recorded.lock().unwrap().extend(pending.drain(..));
                    return;
                }
                Ok(n) => n,
            };
            pending.extend_from_slice(&buf[..n]);
            // Real-time questions, the job's word, and the rest, which is the job.
            loop {
                if let Some(at) = pending.windows(2).position(|w| w == [0x10, 0x04]) {
                    if pending.len() < at + 3 {
                        break;
                    }
                    let n = pending[at + 2];
                    recorded.lock().unwrap().extend_from_slice(&pending[..at]);
                    pending.drain(..at + 3);
                    let answer = match n {
                        1 => script.printer,
                        2 => Some(script.offline),
                        4 => Some(script.paper),
                        _ => None,
                    };
                    if let (Some(answer), Some(_)) = (answer, script.printer) {
                        stream.write_all(&[answer]).unwrap();
                    }
                    continue;
                }
                break;
            }
            let ask = [0x1D, 0x28, 0x48, 6, 0, 48, 48];
            if let Some(at) = pending.windows(ask.len()).position(|w| w == ask)
                && pending.len() >= at + 11
            {
                let id: [u8; 4] = pending[at + 7..at + 11].try_into().unwrap();
                recorded.lock().unwrap().extend_from_slice(&pending[..at + 11]);
                pending.clear();
                if script.reset_after.is_some() {
                    // Reset mid-job: the connection ends before the word.
                    return;
                }
                stream.write_all(&script.chatter).unwrap();
                if script.word == Word::Both {
                    stream.write_all(&escpos::printed(id)).unwrap();
                }
                continue;
            }
            if let Some(at) = pending.windows(3).position(|w| w == escpos::PROCESSED) {
                recorded.lock().unwrap().extend_from_slice(&pending[..at + 3]);
                pending.drain(..at + 3);
                stream.write_all(&script.chatter).unwrap();
                if script.word != Word::Silent {
                    stream.write_all(&[0x00]).unwrap();
                }
                continue;
            }
            // Keep a possible start of a command back; the rest is the job.
            let keep = pending.len().min(10);
            let take = pending.len() - keep;
            recorded.lock().unwrap().extend(pending.drain(..take));
            if let Some(limit) = script.reset_after
                && recorded.lock().unwrap().len() >= limit
            {
                return;
            }
        }
    });
    Fake { address, job, server }
}

/// Short waits, so that tests of silence are quick.
const WAITS: Waits = Waits { status: Duration::from_millis(300), word: Duration::from_millis(500) };

const ID: [u8; 4] = *b"0042";

fn job() -> Vec<u8> {
    let mut job = escpos::INITIALIZE.to_vec();
    job.extend(b"Latte (Large)       $5.25\n");
    job.extend(escpos::CUT);
    job
}

/// Runs the job against `script`, and what the printer recorded of it.
fn run(script: Script, confirm: Confirm) -> (Outcome, Vec<u8>) {
    let fake = fake(script);
    let outcome = tcp::print(fake.address, &job(), confirm, ID, WAITS);
    // The driver is done with the connection: dropping it ends the server's loop.
    let job = Arc::clone(&fake.job);
    fake.server.join().unwrap();
    let recorded = job.lock().unwrap().clone();
    (outcome, recorded)
}

#[test]
fn a_ready_printer_prints_the_job_and_says_so() {
    let (outcome, recorded) = run(ready(), Confirm::Printed);
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
    let mut expected = job();
    expected.extend(escpos::ask_printed(ID));
    assert_eq!(recorded, expected);
}

#[test]
fn paper_near_its_end_prints_and_says_so() {
    let (outcome, _) = run(Script { paper: 0x1E, ..ready() }, Confirm::Printed);
    assert_eq!(outcome, Outcome::Printed { paper_low: true });
}

#[test]
fn a_printer_that_cant_print_gets_none_of_the_job() {
    let offline = |cause: u8| Script { printer: Some(0x1A), offline: cause, ..ready() };
    let cases = [
        (offline(0x16), NotSent::CoverOpen),
        (offline(0x32), NotSent::PaperOut),
        (offline(0x52), NotSent::Error),
        (offline(0x1A), NotSent::Offline),
        (Script { paper: 0x72, ..ready() }, NotSent::PaperOut),
        (Script { printer: Some(0xFF), ..ready() }, NotSent::NotAStatus(0xFF)),
        (Script { printer: None, ..ready() }, NotSent::NoAnswer),
    ];
    for (script, why) in cases {
        let (outcome, recorded) = run(script.clone(), Confirm::Printed);
        assert_eq!(outcome, Outcome::NotSent(why), "{script:?}");
        assert!(recorded.is_empty(), "{script:?} was sent {recorded:02x?}");
    }
}

#[test]
fn a_printer_that_isnt_there_is_unreachable() {
    let address = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    assert_eq!(
        tcp::print(address, &job(), Confirm::Printed, ID, WAITS),
        Outcome::NotSent(NotSent::Unreachable)
    );
}

#[test]
fn a_job_cut_off_mid_way_is_unconfirmed() {
    let (outcome, _) = run(Script { reset_after: Some(5), ..ready() }, Confirm::Printed);
    assert!(
        matches!(outcome, Outcome::Unconfirmed(Unconfirmed::Closed | Unconfirmed::SendFailed)),
        "{outcome:?}"
    );
}

#[test]
fn silence_after_the_job_is_unconfirmed() {
    let (outcome, recorded) = run(Script { word: Word::Silent, ..ready() }, Confirm::Printed);
    assert_eq!(outcome, Outcome::Unconfirmed(Unconfirmed::NoWord));
    assert!(recorded.starts_with(&job()));
    let (outcome, _) = run(Script { word: Word::Silent, ..ready() }, Confirm::Processed);
    assert_eq!(outcome, Outcome::Unconfirmed(Unconfirmed::NoWord));
}

#[test]
fn a_printer_without_gs_paren_h_is_unconfirmed_unless_its_profile_says_so() {
    let without = Script { word: Word::ProcessedOnly, ..ready() };
    let (outcome, _) = run(without.clone(), Confirm::Printed);
    assert_eq!(outcome, Outcome::Unconfirmed(Unconfirmed::NoWord));
    let (outcome, recorded) = run(without, Confirm::Processed);
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
    let mut expected = job();
    expected.extend(escpos::PROCESSED);
    assert_eq!(recorded, expected);
}

#[test]
fn a_printer_that_confirms_nothing_has_the_job_and_nothing_more() {
    let (outcome, recorded) = run(Script { word: Word::Silent, ..ready() }, Confirm::Nothing);
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
    assert_eq!(recorded, job());
}

#[test]
fn what_a_printer_says_before_its_word_is_skipped() {
    let chatty = Script { chatter: vec![0x12, 0x37, 0x22, b'9', b'9'], ..ready() };
    let (outcome, _) = run(chatty, Confirm::Printed);
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
}

#[test]
fn the_status_questions_are_the_ones_the_printer_answers() {
    assert_eq!(escpos::status(Question::Printer), [0x10, 0x04, 0x01]);
}
