"""Planted bugs for keel-print: see run.py.

The crate's tests run the driver against a scripted fake printer, known answers each: every bug
here is a unit test's to catch.
"""

KNOWN_ANSWERS = ["driver"]

BUGS = [
    (
        "print: an offline printer is sent the job",
        "src/lib.rs",
        "    if escpos::offline(printer) {",
        "    if escpos::offline(printer) && printer == 0 {",
        "unit",
    ),
    (
        "print: a printer out of paper is sent the job",
        "src/lib.rs",
        "    if paper.end { Err(NotSent::PaperOut) } else { Ok(paper) }",
        "    if paper.end && paper.near_end { Err(NotSent::PaperOut) } else { Ok(paper) }",
        "unit",
    ),
    (
        "print: a byte that isn't a status is taken for one",
        "src/lib.rs",
        "            Ok(1) if escpos::is_status(byte[0]) => return Ok(byte[0]),",
        "            Ok(1) if escpos::is_status(byte[0]) || byte[0] == 0xFF => return Ok(byte[0]),",
        "unit",
    ),
    (
        "print: a send that fails part way is not sent",
        "src/lib.rs",
        "        return if sent.accepted == 0 {",
        "        return if sent.accepted < usize::MAX {",
        "unit",
    ),
    (
        "print: silence after the job is taken for its word",
        "src/lib.rs",
        """            Err(error) if timed_out(&error) => return Err(Unconfirmed::NoWord),
            Err(_) => return Err(Unconfirmed::Closed),
        }
    }
    Err(Unconfirmed::NoWord)
}

/// Ends the job""",
        """            Err(error) if timed_out(&error) => return Ok(()),
            Err(_) => return Err(Unconfirmed::Closed),
        }
    }
    Err(Unconfirmed::NoWord)
}

/// Ends the job""",
        "unit",
    ),
    (
        "print: any process ID is taken for the job's",
        "src/lib.rs",
        "                answers.iter().any(|answer| *answer == printed)",
        "                answers.iter().any(|answer| answer.first() == printed.first())",
        "unit",
    ),
    (
        "print: the paper's state isn't asked",
        "src/lib.rs",
        "    let paper = Paper::of(ask(transport, Question::Paper, wait)?);",
        "    let paper = Paper::default();",
        "unit",
    ),
    # Found in review.
    (
        "print: a send that fails part way says none of it was accepted",
        "src/tcp.rs",
        "                Err(error) => return Err(Sent { accepted, error }),",
        "                Err(error) => return Err(Sent { accepted: 0, error }),",
        "unit",
    ),
    (
        "print: a printer that stops reading holds the send for ever",
        "src/tcp.rs",
        "        self.stream.set_write_timeout(Some(wait.max(Duration::from_millis(1))))",
        "        self.stream.set_write_timeout(None)",
        "unit",
    ),
    (
        "print: automatic status as the connection opens is taken for a status",
        "src/lib.rs",
        "            Ok(1) if escpos::is_automatic_status(byte[0]) => {",
        "            Ok(1) if escpos::is_automatic_status(byte[0]) && byte[0] == 0 => {",
        "unit",
    ),
    (
        "print: an automatic status block is read as single bytes",
        "src/lib.rs",
        "            self.current.len() == 4",
        "            self.current.len() == 1",
        "unit",
    ),
    (
        "print: a printer that confirms nothing is printed once the job is queued",
        "src/lib.rs",
        """    transport.finish().map_err(|_| Unconfirmed::SendFailed)?;
    let mut buf""",
        """    transport.finish().map_err(|_| Unconfirmed::SendFailed)?;
    if wait > Duration::ZERO {
        return Ok(());
    }
    let mut buf""",
        "unit",
    ),
]
