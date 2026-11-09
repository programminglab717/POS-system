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

/// Text aimed at the layout's edges and the code page's: words of every length, runs of
/// spaces, letters the code page has and lacks, controls, and the bytes of commands.
fn any_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        4 => "[a-z]{1,8}",
        2 => Just(" ".to_owned()),
        1 => Just("  ".to_owned()),
        1 => "[a-z]{20,70}",
        // Words as wide as lines are, wide and not: 12 to 32 columns wide, and 24 to 64.
        1 => prop::sample::select(vec![12_usize, 16, 21, 24, 32, 42, 48, 64]).prop_map(|n| "w".repeat(n)),
        1 => prop::sample::select(vec![
            "é", "€", "ł", "東", "\u{a0}", "\u{2212}", "\u{10}\u{4}\u{1}", "\u{10}\u{14}", "\u{1b}p",
            "\n", "\u{9b}", "\u{200b}", "e\u{301}", "Ĳ", "—", "×", "🙂",
        ])
        .prop_map(str::to_owned),
    ];
    prop::collection::vec(piece, 0..12).prop_map(|pieces| pieces.concat())
}

fn any_style() -> impl Strategy<Value = Style> {
    (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(bold, wide, tall)| Style {
        bold,
        wide,
        tall,
    })
}

fn any_block() -> impl Strategy<Value = Block> {
    let align = prop::sample::select(vec![Align::Left, Align::Center, Align::Right]);
    let right = prop_oneof![
        3 => "\\$[0-9]{1,3}\\.[0-9]{2}",
        1 => any_text(),
        1 => Just(String::new()),
    ];
    prop_oneof![
        4 => (any_text(), align, any_style()).prop_map(|(text, align, style)| Block::Text { text, align, style }),
        4 => (any_text(), right, 0_u8..=8, any_style())
            .prop_map(|(left, right, indent, style)| Block::Row { left, right, indent, style }),
        1 => Just(Block::Rule),
        1 => (1_u8..=9).prop_map(Block::Feed),
        1 => Just(Block::Cut),
        1 => Just(Block::DrawerKick),
    ]
}

fn any_doc() -> impl Strategy<Value = Doc> {
    prop::collection::vec(any_block(), 0..12).prop_map(|blocks| Doc::new(blocks).unwrap())
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
fn model(doc: &Doc, columns: Columns, cutter: bool) -> Vec<Read> {
    let mut out = Vec::new();
    for block in doc.blocks() {
        match block {
            Block::Text { text: body, align, style } => {
                let width = width_of(columns, *style);
                for line in paragraph(&text::encode(body), width) {
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
                let (left, right) = (text::encode(left), text::encode(right));
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
    out
}

/// Lines' styles only matter to the bytes when text is there to print: an empty line's style
/// is whatever came before.
fn comparable(read: Vec<Read>) -> Vec<Read> {
    let mut last = Style::default();
    read.into_iter()
        .map(|item| match item {
            Read::Line(text, _) if text.is_empty() => Read::Line(text, last),
            Read::Line(text, style) => {
                last = style;
                Read::Line(text, style)
            }
            other => other,
        })
        .collect()
}

proptest! {
    #[test]
    fn documents_round_trip_their_encoding(doc in any_doc()) {
        let bytes = doc.encode();
        let decoded = Doc::decode(&bytes).unwrap();
        prop_assert_eq!(&decoded, &doc);
        prop_assert_eq!(decoded.encode(), bytes);
    }

    #[test]
    fn the_bytes_print_the_models_lines(doc in any_doc(), columns in any_columns(), cutter in any::<bool>()) {
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
        prop_assert_eq!(comparable(read.clone()), comparable(model(&doc, columns, cutter)));
        // The plain text is the same lines.
        let lines: Vec<String> = read
            .iter()
            .filter_map(|item| match item {
                Read::Line(text, _) => Some(text::decode(text) + "\n"),
                _ => None,
            })
            .collect();
        let without_cutter_feeds = if cutter { lines.concat() } else { String::new() };
        if cutter {
            prop_assert_eq!(plain_text(&doc, columns), without_cutter_feeds);
        }
    }
}
