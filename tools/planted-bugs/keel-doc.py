"""Planted bugs for keel-doc: see run.py."""

BUGS = [
    # The layout.
    (
        "layout: a word that fills the line goes to the next",
        "src/layout.rs",
        "            && current.len().saturating_add(1).saturating_add(word.len()) <= width",
        "            && current.len().saturating_add(1).saturating_add(word.len()) < width",
    ),
    (
        "layout: a word longer than the line isn't broken",
        "src/layout.rs",
        "            rest.split_at_checked(width).filter(|(_, tail)| !tail.is_empty())",
        "            rest.split_at_checked(width).filter(|(_, tail)| tail.is_empty() && !tail.is_empty())",
    ),
    (
        "layout: centred text sits against the right",
        "src/layout.rs",
        "        Align::Center => room / 2,",
        "        Align::Center => room,",
    ),
    (
        "layout: wide text has every column",
        "src/layout.rs",
        "if style.wide { columns / 2 } else { columns }",
        "if style.wide { columns } else { columns }",
    ),
    (
        "layout: a right text of exactly half the line goes on its own",
        "src/layout.rs",
        "let alone = right.len().saturating_mul(2) > width",
        "let alone = right.len().saturating_mul(2) >= width",
    ),
    (
        "layout: a row's left text runs up against its right",
        "src/layout.rs",
        "let room = width.saturating_sub(indent).saturating_sub(right.len()).saturating_sub(1);",
        "let room = width.saturating_sub(indent).saturating_sub(right.len());",
    ),
    (
        "layout: only a row's first line is indented",
        "src/layout.rs",
        """    let mut lines: Vec<Vec<u8>> = wrap(left, room).into_iter().map(indented).collect();""",
        """    let mut lines: Vec<Vec<u8>> = wrap(left, room).into_iter().enumerate().map(|(at, line)| if at == 0 { indented(line) } else { line }).collect();""",
    ),
    # Text for the printer.
    (
        "text: control characters reach the printer",
        "src/text.rs",
        "        0x20..=0x7E | 0xA1..=0xAC | 0xAE..=0xFF => {",
        "        0x00..=0x7E | 0xA1..=0xAC | 0xAE..=0xFF => {",
    ),
    (
        "text: C1 controls are printed as the code page's characters",
        "src/text.rs",
        "        | 0x86..=0x9F\n",
        "        | 0x86..=0x86\n",
    ),
    (
        "text: no-break spaces stay no-break",
        "src/text.rs",
        "        0xA0 | 0x2007 | 0x202F => Mapped::Byte(NO_BREAK_SPACE),",
        "        0x2007 | 0x202F => Mapped::Byte(NO_BREAK_SPACE),",
    ),
    (
        "text: letters with marks become a question mark",
        "src/text.rs",
        "            Mapped::Byte(base.copied().unwrap_or(b'?'))",
        "            Mapped::Byte(base.map_or(b'?', |_| b'?'))",
    ),
    # The ESC/POS renderer.
    (
        "escpos: bold is never turned off",
        "src/escpos.rs",
        "        if style.bold != current.bold {",
        "        if style.bold && !current.bold {",
    ),
    (
        "escpos: a printer without a cutter is sent the cut",
        "src/escpos.rs",
        "Laid::Cut if printer.cutter => out.extend(CUT),",
        "Laid::Cut if printer.cutter || !printer.cutter => out.extend(CUT),",
    ),
    (
        "escpos: double height is double width",
        "src/escpos.rs",
        "    let height = if style.tall { 0x01 } else { 0 };",
        "    let height = if style.tall { 0x10 } else { 0 };",
    ),
    (
        "escpos: the code page isn't chosen",
        "src/escpos.rs",
        "        [&INITIALIZE[..], &CODE_PAGE_1252, &INTERNATIONAL_USA, &CANCEL_KANJI, &ALIGN_LEFT]",
        "        [&INITIALIZE[..], &INTERNATIONAL_USA, &CANCEL_KANJI, &ALIGN_LEFT]",
    ),
    (
        "escpos: a status byte's fixed bits aren't checked",
        "src/escpos.rs",
        "    byte & 0b1001_0011 == 0b0001_0010",
        "    byte & 0b0000_0010 == 0b0000_0010",
        # Only the driver reads status bytes: keel-print's tests catch it too.
        "unit",
    ),
    (
        "escpos: paper near its end reads as at its end",
        "src/escpos.rs",
        "        Paper { near_end: byte & 0x0C != 0, end: byte & 0x60 != 0 }",
        "        Paper { near_end: byte & 0x0C != 0, end: byte & 0x6C != 0 }",
        # As above.
        "unit",
    ),
    # The encoding.
    (
        "doc: wide and tall are swapped in the encoding",
        "src/doc.rs",
        "Some(Style { bold: code & 1 != 0, wide: code & 2 != 0, tall: code & 4 != 0 })",
        "Some(Style { bold: code & 1 != 0, wide: code & 4 != 0, tall: code & 2 != 0 })",
    ),
    (
        "doc: a feed of no lines is accepted",
        "src/doc.rs",
        "Block::Feed(lines) if *lines == 0 || *lines > MAX_FEED => {",
        "Block::Feed(lines) if *lines > MAX_FEED => {",
        # The random documents' feeds are valid.
        "unit",
    ),
    # Found in review.
    (
        "layout: no-break spaces print as the code page's",
        "src/layout.rs",
        "        if *byte == text::NO_BREAK_SPACE {",
        "        if *byte == text::NO_BREAK_SPACE && byte.is_ascii() {",
    ),
    (
        "text: line breaks and tabs glue words together",
        "src/text.rs",
        "        0x09..=0x0D\n",
        "        0x0B..=0x0C\n",
    ),
    (
        "text: tags print as question marks",
        "src/text.rs",
        "        | 0xE0000..=0xE007F => Mapped::Dropped,",
        "        | 0xE0000..=0xE0000 => Mapped::Dropped,",
    ),
    (
        "escpos: a cut without a cutter feeds in the style before it",
        "src/escpos.rs",
        "                    line(&mut out, &[], Style::default());",
        "                    out.push(LINE_FEED);",
    ),
    (
        "escpos: any byte that starts with bit 4 is automatic status",
        "src/escpos.rs",
        "    byte & 0b1001_0011 == 0b0001_0000",
        "    byte & 0b0001_0000 == 0b0001_0000",
        "unit",
    ),
]
