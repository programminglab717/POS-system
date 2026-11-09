//! The print driver ([ADR-0024](../../../../docs/adr/0024-receipts-and-printing.md), decision 6):
//! one job, one connection, against an ESC/POS printer, and what happened.
//!
//! 1. The printer's status, `DLE EOT 1`, and when it is offline its cause, `DLE EOT 2`; then its
//!    paper, `DLE EOT 4`. A printer that is offline, open, out of paper or in error, that doesn't
//!    answer, or whose answer isn't a status, gets none of the job: [`Outcome::NotSent`].
//! 2. The job. A send that fails before any of its bytes is accepted is [`Outcome::NotSent`];
//!    one that fails after is [`Outcome::Unconfirmed`].
//! 3. The printer's word, as its profile says: `GS ( H`, answered once the job has printed;
//!    `GS r 1`, answered once it is processed; or nothing, where a clean close is all there is.
//!    No word in time is [`Outcome::Unconfirmed`].
//!
//! The driver reads no clock: a transport waits at most a given time for each answer, and the
//! driver reads a bounded number of bytes for each.
//!
//! A platform crate: [`tcp`] is the transport over `std::net`, to port 9100.

pub mod tcp;

use core::time::Duration;
use std::io;

use keel_doc::Confirm;
use keel_doc::escpos::{self, OfflineCause, Paper, Question};

/// How a driver reaches a printer.
pub trait Transport {
    /// Sends `bytes`, all of them.
    ///
    /// # Errors
    /// [`Sent`], with how many bytes were accepted before the failure.
    fn send(&mut self, bytes: &[u8]) -> Result<(), Sent>;

    /// Waits at most `wait` for bytes, and puts what came, at least one byte, at the start of
    /// `buf`: `Ok(0)` when the printer closed the connection.
    ///
    /// # Errors
    /// [`io::ErrorKind::TimedOut`] or [`io::ErrorKind::WouldBlock`] if nothing came in time, and
    /// any other error if the connection failed.
    fn receive(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize>;

    /// Ends what the driver sends, so that the printer sees the job is whole.
    ///
    /// # Errors
    /// Any error closing it.
    fn finish(&mut self) -> io::Result<()>;
}

/// A send that failed.
#[derive(Debug, thiserror::Error)]
#[error("sending failed after {accepted} bytes: {error}")]
pub struct Sent {
    /// How many bytes were accepted before it failed.
    pub accepted: usize,
    /// Why it failed.
    pub error: io::Error,
}

/// How long the driver waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waits {
    /// For each answer to a status question.
    pub status: Duration,
    /// For the printer's word that a job printed or was processed: as long as the longest job
    /// takes to print.
    pub word: Duration,
}

impl Waits {
    /// Two seconds for a status, sixty for a job's word.
    pub const DEFAULT: Waits =
        Waits { status: Duration::from_secs(2), word: Duration::from_secs(60) };
}

/// Why none of a job was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotSent {
    /// The printer couldn't be reached.
    Unreachable,
    /// It didn't answer a status question in time, or closed the connection.
    NoAnswer,
    /// It answered with a byte that isn't a status: not an ESC/POS printer, or not listening.
    NotAStatus(u8),
    /// Its cover is open.
    CoverOpen,
    /// It is out of paper.
    PaperOut,
    /// It has an error, such as a cutter jammed.
    Error,
    /// It is offline for another reason, such as paper being fed.
    Offline,
    /// The connection failed before any of the job was accepted.
    SendFailed,
}

/// Why a job, some of which may have been sent, isn't known to have printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unconfirmed {
    /// The connection failed after some of the job was accepted.
    SendFailed,
    /// The printer closed the connection before its word.
    Closed,
    /// No word came in time.
    NoWord,
}

/// What happened to a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It printed, or the printer said it processed it, or, for a printer that confirms
    /// nothing, was sent whole.
    Printed {
        /// The paper is near its end.
        paper_low: bool,
    },
    /// None of it was sent: it can be sent again.
    NotSent(NotSent),
    /// Some of it may have been sent: it must never be sent again by itself.
    Unconfirmed(Unconfirmed),
}

/// The most bytes read while waiting for the printer's word: anything else it sends is skipped.
const MAX_SKIPPED: usize = 256;

/// Runs `job`, rendered ESC/POS bytes, against the printer at the other end of `transport`, which
/// confirms jobs as `confirm` says, under the process ID `id`.
pub fn print(
    transport: &mut impl Transport,
    job: &[u8],
    confirm: Confirm,
    id: [u8; 4],
    waits: Waits,
) -> Outcome {
    let paper_low = match ready(transport, waits.status) {
        Ok(paper) => paper.near_end,
        Err(not_sent) => return Outcome::NotSent(not_sent),
    };
    let mut bytes = job.to_vec();
    match confirm {
        Confirm::Printed => bytes.extend(escpos::ask_printed(id)),
        Confirm::Processed => bytes.extend(escpos::PROCESSED),
        Confirm::Nothing => {}
    }
    if let Err(sent) = transport.send(&bytes) {
        return if sent.accepted == 0 {
            Outcome::NotSent(NotSent::SendFailed)
        } else {
            Outcome::Unconfirmed(Unconfirmed::SendFailed)
        };
    }
    let word = match confirm {
        Confirm::Printed => {
            wait_for(transport, waits.word, |seen| seen.ends_with(&escpos::printed(id)))
        }
        Confirm::Processed => wait_for(transport, waits.word, |seen| {
            seen.last().is_some_and(|byte| escpos::is_processed(*byte))
        }),
        Confirm::Nothing => transport.finish().map_err(|_| Unconfirmed::SendFailed),
    };
    match word {
        Ok(()) => Outcome::Printed { paper_low },
        Err(why) => Outcome::Unconfirmed(why),
    }
}

/// Asks the printer whether it can print: its paper, if it can, or why none of a job should be
/// sent.
fn ready(transport: &mut impl Transport, wait: Duration) -> Result<Paper, NotSent> {
    let printer = ask(transport, Question::Printer, wait)?;
    if escpos::offline(printer) {
        let cause = OfflineCause::of(ask(transport, Question::Offline, wait)?);
        return Err(match cause {
            OfflineCause { cover_open: true, .. } => NotSent::CoverOpen,
            OfflineCause { paper_end: true, .. } => NotSent::PaperOut,
            OfflineCause { error: true, .. } => NotSent::Error,
            _ => NotSent::Offline,
        });
    }
    let paper = Paper::of(ask(transport, Question::Paper, wait)?);
    if paper.end { Err(NotSent::PaperOut) } else { Ok(paper) }
}

/// Asks `question`, and reads the status byte that answers it.
fn ask(transport: &mut impl Transport, question: Question, wait: Duration) -> Result<u8, NotSent> {
    transport.send(&escpos::status(question)).map_err(|_| NotSent::NoAnswer)?;
    let mut byte = [0_u8; 1];
    match transport.receive(&mut byte, wait) {
        Ok(1) if escpos::is_status(byte[0]) => Ok(byte[0]),
        Ok(1) => Err(NotSent::NotAStatus(byte[0])),
        _ => Err(NotSent::NoAnswer),
    }
}

/// Reads from the printer until what it sent satisfies `done`, each read waiting at most `wait`.
fn wait_for(
    transport: &mut impl Transport,
    wait: Duration,
    done: impl Fn(&[u8]) -> bool,
) -> Result<(), Unconfirmed> {
    let mut seen = Vec::new();
    let mut buf = [0_u8; 64];
    while seen.len() < MAX_SKIPPED {
        match transport.receive(&mut buf, wait) {
            Ok(0) => return Err(Unconfirmed::Closed),
            Ok(n) => {
                for byte in buf.get(..n).unwrap_or_default() {
                    seen.push(*byte);
                    if done(&seen) {
                        return Ok(());
                    }
                }
            }
            Err(error)
                if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) =>
            {
                return Err(Unconfirmed::NoWord);
            }
            Err(_) => return Err(Unconfirmed::Closed),
        }
    }
    Err(Unconfirmed::NoWord)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A transport that answers every status question as a ready printer, and fails the job's
    /// send after `accept` bytes.
    struct Failing {
        accept: usize,
        answers: Vec<u8>,
    }

    impl Transport for Failing {
        fn send(&mut self, bytes: &[u8]) -> Result<(), Sent> {
            if bytes.starts_with(&[0x10, 0x04]) {
                self.answers.push(0x12);
                return Ok(());
            }
            Err(Sent { accepted: self.accept, error: io::ErrorKind::ConnectionReset.into() })
        }

        fn receive(&mut self, buf: &mut [u8], _wait: Duration) -> io::Result<usize> {
            match (self.answers.pop(), buf.first_mut()) {
                (Some(answer), Some(first)) => {
                    *first = answer;
                    Ok(1)
                }
                _ => Err(io::ErrorKind::TimedOut.into()),
            }
        }

        fn finish(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_send_that_fails_is_not_sent_only_if_none_of_it_was_accepted() {
        let run = |accept| {
            print(
                &mut Failing { accept, answers: vec![] },
                b"job",
                Confirm::Printed,
                *b"0001",
                Waits::DEFAULT,
            )
        };
        assert_eq!(run(0), Outcome::NotSent(NotSent::SendFailed));
        assert_eq!(run(1), Outcome::Unconfirmed(Unconfirmed::SendFailed));
        assert_eq!(run(2), Outcome::Unconfirmed(Unconfirmed::SendFailed));
    }
}
