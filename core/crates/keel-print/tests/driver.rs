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
    /// Bytes it sends as the connection opens, as a printer with automatic status back on does.
    greeting: Vec<u8>,
    /// Once it has answered every question, it waits for the job and closes the connection
    /// without reading it, which resets it.
    close_unread: bool,
    /// Once it has answered every question, it reads nothing more for three seconds.
    stall: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Word {
    /// It answers `GS ( H` and `GS r 1`.
    Both,
    /// It answers only `GS r 1`, as a printer without `GS ( H`.
    ProcessedOnly,
    /// It answers nothing.
    Silent,
    /// It answers `GS ( H` with another job's process ID.
    Stale,
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
        greeting: vec![],
        close_unread: false,
        stall: false,
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
        stream.write_all(&script.greeting).unwrap();
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
                    if n == 4 && script.stall {
                        thread::sleep(Duration::from_secs(10));
                        return;
                    }
                    if n == 4 && script.close_unread {
                        thread::sleep(Duration::from_millis(200));
                        return;
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
                if script.word == Word::Stale {
                    stream.write_all(&escpos::printed(*b"0041")).unwrap();
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
const WAITS: Waits = Waits { status: Duration::from_secs(1), word: Duration::from_secs(1) };

const ID: [u8; 4] = *b"0042";

fn job() -> Vec<u8> {
    let mut job = escpos::INITIALIZE.to_vec();
    job.extend(b"Latte (Large)       $5.25\n");
    job.extend(escpos::CUT);
    job
}

/// Runs the job against `script`, and what the printer recorded of it.
fn run(script: Script, confirm: Confirm) -> (Outcome, Vec<u8>) {
    run_job(script, confirm, &job())
}

/// Runs `job` against `script`, and what the printer recorded of it.
fn run_job(script: Script, confirm: Confirm, job: &[u8]) -> (Outcome, Vec<u8>) {
    let fake = fake(script);
    let outcome = tcp::print(fake.address, job, confirm, ID, WAITS);
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
    // A stray status byte, a block of automatic status back, and another job's process ID.
    let chatter = vec![0x12, 0x38, 0, 0, 0, 0x37, 0x22, b'0', b'0', b'4', b'1', 0];
    let chatty = Script { chatter: chatter.clone(), ..ready() };
    let (outcome, _) = run(chatty, Confirm::Printed);
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
    let chatty = Script { chatter: chatter.clone(), ..ready() };
    assert_eq!(run(chatty, Confirm::Processed).0, Outcome::Printed { paper_low: false });
}

/// Another job's process ID isn't this job's word.
#[test]
fn another_jobs_process_id_isnt_the_word() {
    let (outcome, _) = run(Script { word: Word::Stale, ..ready() }, Confirm::Printed);
    assert_eq!(outcome, Outcome::Unconfirmed(Unconfirmed::NoWord));
}

/// Found in review: with `GS r 1`, any byte with bits 4 and 7 clear was taken for its answer,
/// and the last three bytes of an automatic status block are such bytes.
#[test]
fn an_automatic_status_block_isnt_taken_for_the_printers_word() {
    let chatty = Script { chatter: vec![0x38, 0, 0, 0], word: Word::Silent, ..ready() };
    assert_eq!(run(chatty, Confirm::Processed).0, Outcome::Unconfirmed(Unconfirmed::NoWord));
    let chatty = Script {
        chatter: vec![0x37, 0x22, b'0', b'0', b'4', b'1', 0],
        word: Word::Silent,
        ..ready()
    };
    assert_eq!(run(chatty, Confirm::Processed).0, Outcome::Unconfirmed(Unconfirmed::NoWord));
    assert_eq!(
        run(
            Script { chatter: vec![0x37, 0x22, b'0', b'0', b'4', b'1', 0], ..ready() },
            Confirm::Printed
        )
        .0,
        Outcome::Printed { paper_low: false }
    );
}

/// Found in review: a printer sending automatic status as the connection opens could never
/// print, its first byte taken for a status that isn't one.
#[test]
fn automatic_status_before_the_first_answer_is_skipped() {
    let greeting = Script { greeting: vec![0x18, 0x00, 0x00, 0x0F, 0x10, 0, 0, 0], ..ready() };
    assert_eq!(run(greeting, Confirm::Printed).0, Outcome::Printed { paper_low: false });
}

/// Found in review: for a printer that confirms nothing, the driver said it printed as soon as
/// it had queued its end of the connection, before the printer read anything.
#[test]
fn a_printer_that_confirms_nothing_must_close_the_connection_itself() {
    let (outcome, recorded) = run(Script { word: Word::Silent, ..ready() }, Confirm::Nothing);
    assert_eq!(outcome, Outcome::Printed { paper_low: false });
    assert_eq!(recorded, job());
    let unread = Script { close_unread: true, ..ready() };
    let (outcome, _) = run(unread, Confirm::Nothing);
    assert!(matches!(outcome, Outcome::Unconfirmed(_)), "{outcome:?}");
}

/// Found in review: the test of a job cut off mid-way ended it after the whole job had arrived.
/// This printer reads 100 KB of an 8 MB job and resets the connection.
#[test]
fn a_job_reset_part_way_through_its_send_is_unconfirmed() {
    let big = vec![b'x'; 8 << 20];
    let (outcome, recorded) =
        run_job(Script { reset_after: Some(100_000), ..ready() }, Confirm::Printed, &big);
    assert_eq!(outcome, Outcome::Unconfirmed(Unconfirmed::SendFailed));
    assert!(recorded.len() >= 100_000 && recorded.len() < big.len());
}

#[test]
fn the_status_questions_are_the_ones_the_printer_answers() {
    assert_eq!(escpos::status(Question::Printer), [0x10, 0x04, 0x01]);
}

/// Found in review: a printer that stopped reading held the send for ever. It waits as long as
/// the printer's word would, and some of the job may have been sent.
#[test]
fn a_printer_that_stops_reading_fails_the_send() {
    let big = vec![b'x'; 64 << 20];
    let started = std::time::Instant::now();
    let fake = fake(Script { stall: true, ..ready() });
    let outcome = tcp::print(fake.address, &big, Confirm::Printed, ID, WAITS);
    // Long before the printer gives up, after 10 s.
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(outcome, Outcome::Unconfirmed(Unconfirmed::SendFailed));
    fake.server.join().unwrap();
}
