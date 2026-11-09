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
        "        Ok(1) if escpos::is_status(byte[0]) => Ok(byte[0]),",
        "        Ok(1) if escpos::is_status(byte[0]) || byte[0] == 0xFF => Ok(byte[0]),",
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
        """                return Err(Unconfirmed::NoWord);
            }
            Err(_) => return Err(Unconfirmed::Closed),""",
        """                return Ok(());
            }
            Err(_) => return Err(Unconfirmed::Closed),""",
        "unit",
    ),
    (
        "print: any process ID is taken for the job's",
        "src/lib.rs",
        "wait_for(transport, waits.word, |seen| seen.ends_with(&escpos::printed(id)))",
        "wait_for(transport, waits.word, |seen| seen.starts_with(&[0x37]))",
        "unit",
    ),
    (
        "print: the paper's state isn't asked",
        "src/lib.rs",
        "    let paper = Paper::of(ask(transport, Question::Paper, wait)?);",
        "    let paper = Paper::default();",
        "unit",
    ),
]
