"""Planted bugs for keel-net: see run.py.

The crate's tests run a hub and devices over real TLS on the host: known answers, so every bug
here is theirs to catch.
"""

KNOWN_ANSWERS = ["net"]

BUGS = [
    (
        "net: a device the roster doesn't have is taken",
        "src/lib.rs",
        "    roster.device(device)?;\n    let subject",
        "    let subject",
        "unit",
    ),
    (
        "net: a certificate naming two devices is taken for the first",
        "src/lib.rs",
        "    if names.next().is_some() || name.contains('*') {",
        "    if name.contains('*') {",
        "unit",
    ),
    (
        "net: a wildcard beside a device's name is ignored",
        "src/lib.rs",
        "        .filter(|name| name.contains('*') || name.to_ascii_lowercase().ends_with(DEVICES));",
        "        .filter(|name| !name.contains('*') && name.to_ascii_lowercase().ends_with(DEVICES));",
        "unit",
    ),
    (
        "net: a device takes any device of the roster for its hub",
        "src/lib.rs",
        "        Peer::Hub => roster.device(device).is_some_and(|device| device.hub.is_some()),",
        "        Peer::Hub => roster.device(device).is_some(),",
        "unit",
    ),
    (
        "net: a frame larger than a frame may be is taken",
        "src/lib.rs",
        "Ok(Some(Ok(Message::Binary(frame)))) if frame.len() <= MAX_FRAME => {",
        "Ok(Some(Ok(Message::Binary(frame)))) if frame.len() <= MAX_FRAME.saturating_add(64) => {",
        "unit",
    ),
    (
        "net: a device gives up once its hub has gone",
        "src/lib.rs",
        "            backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);",
        "            backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);\n            if backoff > Duration::from_secs(1) { return; }",
        "unit",
    ),
    (
        "net: no side pings, so a quiet connection falls silent and closes",
        "src/lib.rs",
        "                    _ = ping.tick() => Message::Ping(Vec::new().into()),",
        "                    _ = ping.tick() => continue,",
        "unit",
    ),
    (
        "net: a reader waits for its writer, so both sides writing at once stall",
        "src/lib.rs",
        "        tokio::select! {\n            () = reader => {}\n            () = writer => {}\n        }",
        "        writer.await;\n        reader.await;",
        "unit",
    ),
    (
        "net: any device of the roster passes for the cloud",
        "src/lib.rs",
        "        Peer::Device(expected) => device == expected,",
        "        Peer::Device(_) => true,",
        "unit",
    ),
    (
        "net: the cell admits any device of the roster, not those that may host the hub alone",
        "src/lib.rs",
        "        Net::listen(listener, roster, acceptor, Peer::Hub, None)",
        "        Net::listen(listener, roster, acceptor, Peer::Any, None)",
        "unit",
    ),
    (
        "net: the hub never dials its cloud",
        "src/lib.rs",
        "            if let Some(dialer) = dialer {",
        "            if let Some(dialer) = dialer.filter(|_| false) {",
        "unit",
    ),
]
