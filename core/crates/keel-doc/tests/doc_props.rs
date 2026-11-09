//! Property tests of `KeelDoc` v0 (ADR-0024, decisions 2 to 4): documents round-trip their
//! encoding; and the ESC/POS bytes of any document, read back by an interpreter written here,
//! give the lines a model of the layout gives, none wider than its columns, with no control
//! byte outside the renderer's commands and no real-time command at all, whatever the text.
//!
//! The model fills lines by taking, each time, the longest run of words that fits, where the
//! layout streams words into a line until one doesn't fit.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use keel_doc::escpos::{PrinterProfile, plain_text, render};
use keel_doc::{Align, Block, Columns, Confirm, Doc, Style, text};
use proptest::prelude::*;

/// A text, and the Windows-1252 bytes ADR-0024's decision 3 says it becomes, worked out here
/// piece by piece rather than by the crate's own table: `0xA0` for a no-break space, which the
/// layout never breaks at and prints as a space.
type Text = (String, Vec<u8>);

/// Text aimed at the layout's edges and the code page's: words of every length, runs of
/// spaces, letters the code page has and lacks, controls, line breaks, and the bytes of
/// commands.
fn any_text() -> impl Strategy<Value = Text> {
    let ascii = |text: String| {
        let bytes = text.as_bytes().to_vec();
        (text, bytes)
    };
    let special: Vec<(&str, &[u8])> = vec![
        ("é", &[0xE9]),
        ("€", &[0x80]),
        ("—", &[0x97]),
        ("×", &[0xD7]),
        ("ł", b"l"),
        ("ș", b"s"),
        ("Ĳ", b"IJ"),
        ("e\u{301}", b"e"),
        ("東", b"?"),
        ("🙂", b"?"),
        ("\u{a0}", &[0xA0]),
        ("\u{202f}", &[0xA0]),
        ("\u{2212}", b"-"),
        ("\n", b" "),
        ("\t", b" "),
        ("\u{2028}", b" "),
        ("\u{10}\u{4}\u{1}", b""),
        ("\u{10}\u{14}", b""),
        ("\u{1b}p", b"p"),
        ("\u{9b}", b""),
        ("\u{200b}", b""),
        ("\u{e0067}", b""),
    ];
    let piece = prop_oneof![
        4 => "[a-z]{1,8}".prop_map(ascii),
        2 => Just(ascii(" ".to_owned())),
        1 => Just(ascii("  ".to_owned())),
        1 => "[a-z]{20,70}".prop_map(ascii),
        // Words as wide as lines are, wide and not: 12 to 32 columns wide, and 24 to 64.
        1 => prop::sample::select(vec![12_usize, 16, 21, 24, 32, 42, 48, 64]).prop_map(move |n| ascii("w".repeat(n))),
        2 => prop::sample::select(special).prop_map(|(text, bytes)| (text.to_owned(), bytes.to_vec())),
    ];
    prop::collection::vec(piece, 0..12).prop_map(|pieces| {
        let text = pieces.iter().map(|(text, _)| text.as_str()).collect();
        let bytes = pieces.iter().flat_map(|(_, bytes)| bytes.iter().copied()).collect();
        (text, bytes)
    })
}

fn any_style() -> impl Strategy<Value = Style> {
    (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(bold, wide, tall)| Style {
        bold,
        wide,
        tall,
    })
}

/// A block, with the bytes of each of its texts.
fn any_block() -> impl Strategy<Value = (Block, Vec<Vec<u8>>)> {
    let align = prop::sample::select(vec![Align::Left, Align::Center, Align::Right]);
    let amount = |text: String| {
        let bytes = text.as_bytes().to_vec();
        (text, bytes)
    };
    let right = prop_oneof![
        3 => "\\$[0-9]{1,3}\\.[0-9]{2}".prop_map(amount),
        1 => Just(("-EUR\u{a0}12.34".to_owned(), b"-EUR\xa012.34".to_vec())),
        1 => any_text(),
        1 => Just((String::new(), Vec::new())),
    ];
    prop_oneof![
        4 => (any_text(), align, any_style())
            .prop_map(|((text, bytes), align, style)| (Block::Text { text, align, style }, vec![bytes])),
        4 => (any_text(), right, 0_u8..=8, any_style()).prop_map(|((left, l), (right, r), indent, style)| {
            (Block::Row { left, right, indent, style }, vec![l, r])
        }),
        1 => Just((Block::Rule, vec![])),
        1 => (1_u8..=9).prop_map(|lines| (Block::Feed(lines), vec![])),
        1 => Just((Block::Cut, vec![])),
        1 => Just((Block::DrawerKick, vec![])),
    ]
}

/// A document, with the bytes of each of its blocks' texts.
fn any_doc() -> impl Strategy<Value = (Doc, Vec<Vec<Vec<u8>>>)> {
    prop::collection::vec(any_block(), 0..12).prop_map(|blocks| {
        let (blocks, bytes): (Vec<Block>, Vec<Vec<Vec<u8>>>) = blocks.into_iter().unzip();
        (Doc::new(blocks).unwrap(), bytes)
    })
}

fn any_columns() -> impl Strategy<Value = Columns> {
    prop_oneof![Just(24_u8), Just(25), Just(32), Just(42), Just(48), Just(64), 24_u8..=64]
        .prop_map(|columns| Columns::new(columns).unwrap())
}

/// What the interpreter reads.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Read {
    Line(Vec<u8>, Style),
    Cut,
    Kick,
}

/// Reads ESC/POS bytes back: the commands the renderer writes, and text bytes, which must be
/// printable. Panics on anything else.
fn interpret(bytes: &[u8], cutter: bool) -> Vec<Read> {
    let init = [0x1B, 0x40, 0x1B, 0x74, 16, 0x1B, 0x52, 0, 0x1C, 0x2E, 0x1B, 0x61, 0];
    assert!(bytes.starts_with(&init), "no initialization");
    let mut out = Vec::new();
    let (mut style, mut text) = (Style::default(), Vec::new());
    let mut at = init.len();
    while at < bytes.len() {
        match &bytes[at..] {
            [0x1B, 0x45, n, ..] if *n <= 1 => {
                style.bold = *n == 1;
                at += 3;
            }
            [0x1D, 0x21, n, ..] if *n & !0x11 == 0 => {
                style.wide = n & 0x10 != 0;
                style.tall = n & 0x01 != 0;
                at += 3;
            }
            [0x0A, ..] => {
                out.push(Read::Line(core::mem::take(&mut text), style));
                at += 1;
            }
            [0x1D, 0x56, 66, 0, ..] if cutter => {
                out.push(Read::Cut);
                at += 4;
            }
            [0x1B, 0x70, 0, 25, 250, ..] => {
                out.push(Read::Kick);
                at += 5;
            }
            [byte, ..] if *byte >= 0x20 && *byte != 0x7F => {
                text.push(*byte);
                at += 1;
            }
            rest => panic!("unexpected bytes {:02x?}", &rest[..rest.len().min(8)]),
        }
    }
    assert!(text.is_empty(), "text after the last line feed");
    out
}

/// The words of `text`, the longest run of them that fits taken each time; a word too long for
/// a line gives the line as much of it as fits.
fn fill(words: &[Vec<u8>], width: usize) -> Vec<Vec<u8>> {
    if words.is_empty() {
        return Vec::new();
    }
    if words[0].len() > width {
        let (head, tail) = words[0].split_at(width);
        let mut rest = vec![tail.to_vec()];
        rest.extend_from_slice(&words[1..]);
        let mut lines = vec![head.to_vec()];
        lines.extend(fill(&rest, width));
        return lines;
    }
    let mut take = 1;
    while take < words.len() && words[..=take].iter().map(Vec::len).sum::<usize>() + take <= width {
        take += 1;
    }
    let mut lines = vec![words[..take].join(&b' ')];
    lines.extend(fill(&words[take..], width));
    lines
}

fn words(text: &[u8]) -> Vec<Vec<u8>> {
    text.split(|b| *b == b' ').filter(|w| !w.is_empty()).map(<[u8]>::to_vec).collect()
}

fn paragraph(text: &[u8], width: usize) -> Vec<Vec<u8>> {
    let lines = fill(&words(text), width);
    if lines.is_empty() { vec![Vec::new()] } else { lines }
}

fn pad(line: Vec<u8>, left: usize) -> Vec<u8> {
    let mut out = vec![b' '; left];
    out.extend(line);
    out
}

/// How many characters of `style` a line holds: half the columns when they are wide.
fn width_of(columns: Columns, style: Style) -> usize {
    let columns = usize::from(columns.get());
    if style.wide { columns / 2 } else { columns }
}

/// The model: the lines ADR-0024's decision 2 describes.
fn model(doc: &Doc, texts: &[Vec<Vec<u8>>], columns: Columns, cutter: bool) -> Vec<Read> {
    let mut out = Vec::new();
    for (block, texts) in doc.blocks().iter().zip(texts) {
        match block {
            Block::Text { text: body, align, style } => {
                let width = width_of(columns, *style);
                let _ = body;
                for line in paragraph(&texts[0], width) {
                    let room = width - line.len();
                    let left = match align {
                        Align::Left => 0,
                        Align::Center => room / 2,
                        Align::Right => room,
                    };
                    out.push(Read::Line(pad(line, left), *style));
                }
            }
            Block::Row { left, right, indent, style } => {
                let width = width_of(columns, *style);
                let indent = usize::from(*indent);
                let _ = (left, right);
                let (left, right) = (texts[0].clone(), texts[1].clone());
                let right_words = words(&right);
                let right_line = right_words.join(&b' ');
                let mut lines = Vec::new();
                if right_words.is_empty() {
                    lines = paragraph(&left, width - indent)
                        .into_iter()
                        .map(|l| pad(l, indent))
                        .collect();
                } else if right_line.len() * 2 > width || width - indent < right_line.len() + 2 {
                    if !words(&left).is_empty() {
                        lines = paragraph(&left, width - indent)
                            .into_iter()
                            .map(|l| pad(l, indent))
                            .collect();
                    }
                    for line in paragraph(&right, width) {
                        let room = width - line.len();
                        lines.push(pad(line, room));
                    }
                } else {
                    let room = width - indent - right_line.len() - 1;
                    lines = paragraph(&left, room).into_iter().map(|l| pad(l, indent)).collect();
                    let first = &mut lines[0];
                    let gap = width - first.len() - right_line.len();
                    first.extend(std::iter::repeat_n(b' ', gap));
                    first.extend(&right_line);
                }
                out.extend(lines.into_iter().map(|line| Read::Line(line, *style)));
            }
            Block::Rule => {
                out.push(Read::Line(vec![b'-'; usize::from(columns.get())], Style::default()));
            }
            Block::Feed(lines) => {
                out.extend((0..*lines).map(|_| Read::Line(Vec::new(), Style::default())));
            }
            Block::Cut if cutter => out.push(Read::Cut),
            Block::Cut => out.extend((0..3).map(|_| Read::Line(Vec::new(), Style::default()))),
            Block::DrawerKick => out.push(Read::Kick),
        }
    }
    // No-break spaces print as spaces.
    for item in &mut out {
        if let Read::Line(text, _) = item {
            for byte in text.iter_mut().filter(|byte| **byte == 0xA0) {
                *byte = b' ';
            }
        }
    }
    out
}

proptest! {
    #[test]
    fn documents_round_trip_their_encoding((doc, _) in any_doc()) {
        let bytes = doc.encode();
        let decoded = Doc::decode(&bytes).unwrap();
        prop_assert_eq!(&decoded, &doc);
        prop_assert_eq!(decoded.encode(), bytes);
    }

    #[test]
    fn the_bytes_print_the_models_lines((doc, texts) in any_doc(), columns in any_columns(), cutter in any::<bool>()) {
        let printer = PrinterProfile { columns, cutter, confirm: Confirm::Printed };
        let bytes = render(&doc, &printer);
        // No real-time command: `DLE EOT`, `DLE ENQ` or `DLE DC4`.
        for pair in bytes.windows(2) {
            prop_assert!(!matches!(pair, [0x10, 0x04 | 0x05 | 0x14]), "a real-time command in {bytes:02x?}");
        }
        let read = interpret(&bytes, cutter);
        for item in &read {
            if let Read::Line(text, style) = item {
                prop_assert!(text.len() <= width_of(columns, *style), "{} columns", text.len());
            }
        }
        prop_assert_eq!(&read, &model(&doc, &texts, columns, cutter));
        // The plain text is the paper's lines, but for the feeds past the tear bar that stand for
        // a cut on a printer without a cutter.
        let with_cutter = interpret(&render(&doc, &PrinterProfile { cutter: true, ..printer }), true);
        let lines: String = with_cutter
            .iter()
            .filter_map(|item| match item {
                Read::Line(text, _) => Some(text::decode(text) + "\n"),
                _ => None,
            })
            .collect();
        prop_assert_eq!(plain_text(&doc, columns), lines);
    }
}
