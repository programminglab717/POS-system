"""Planted bugs for keel-net: see run.py.

The crate's tests run a hub and devices over real TLS on the host: known answers, so every bug
here is theirs to catch.
"""

KNOWN_ANSWERS = ["net"]

BUGS = [
    (
        "net: any certificate from the CA is taken for a roster's device",
        "src/lib.rs",
        "            .is_ok_and(|name| parsed.verify_is_valid_for_subject_name(&name).is_ok())",
        "            .is_ok_and(|_| true)",
        "unit",
    ),
    (
        "net: a frame larger than a frame may be is taken",
        "src/lib.rs",
        "                    Some(Ok(Message::Binary(frame))) if frame.len() <= MAX_FRAME => {",
        "                    Some(Ok(Message::Binary(frame))) if frame.len() <= MAX_FRAME.saturating_add(64) => {",
        "unit",
    ),
    (
        "net: a device gives up once its hub has gone",
        "src/lib.rs",
        "                backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);",
        "                backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);\n                if backoff > Duration::from_secs(1) { return; }",
        "unit",
    ),
]
