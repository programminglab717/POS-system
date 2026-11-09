//! ESC/POS (ADR-0024, decisions 4 and 6): a document rendered in the subset Epson, Star in its
//! ESC/POS emulation, Bixolon, Citizen and generic printers share, and the printer's answers to
//! the driver's questions read.

use crate::doc::{Doc, Style};
use crate::layout::{Columns, Laid, layout};

/// How a printer says a job is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Confirm {
    /// `GS ( H` function 48: a process ID response once what came before it has printed, as
    /// Epson's TM printers give.
    Printed,
    /// `GS r 1`: the paper sensor's status once what came before it is processed.
    Processed,
    /// Nothing: a clean close of the connection is all there is.
    Nothing,
}

/// What the renderer and the driver need to know of a printer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrinterProfile {
    /// The columns of a line in its normal font.
    pub columns: Columns,
    /// Whether it cuts the paper.
    pub cutter: bool,
    /// How it says a job is done.
    pub confirm: Confirm,
}

impl PrinterProfile {
    /// An Epson TM printer on 80 mm paper: 48 columns, a cutter, and process ID responses.
    pub const EPSON_80MM: PrinterProfile =
        PrinterProfile { columns: Columns::PAPER_80MM, cutter: true, confirm: Confirm::Printed };
}

/// `ESC @`: initialize.
pub const INITIALIZE: [u8; 2] = [0x1B, 0x40];
/// `ESC t 16`: the Windows-1252 code page.
pub const CODE_PAGE_1252: [u8; 3] = [0x1B, 0x74, 16];
/// `ESC R 0`: the international character set of the USA.
pub const INTERNATIONAL_USA: [u8; 3] = [0x1B, 0x52, 0];
/// `FS .`: Kanji mode cancelled.
pub const CANCEL_KANJI: [u8; 2] = [0x1C, 0x2E];
/// `ESC a 0`: left-aligned, as every line is, padded by the layout.
pub const ALIGN_LEFT: [u8; 3] = [0x1B, 0x61, 0];
/// `LF`: print the line and feed one.
pub const LINE_FEED: u8 = 0x0A;
/// `GS V 66 0`: feed to the cutter and cut partly.
pub const CUT: [u8; 4] = [0x1D, 0x56, 66, 0];
/// `ESC p 0 25 250`: a pulse on pin 2, 50 ms on and 500 ms off.
pub const DRAWER_KICK: [u8; 5] = [0x1B, 0x70, 0, 25, 250];
/// `GS r 1`: send the paper sensor's status once what came before is processed.
pub const PROCESSED: [u8; 3] = [0x1D, 0x72, 1];

/// `ESC E n`: bold or not.
pub const fn bold(on: bool) -> [u8; 3] {
    [0x1B, 0x45, if on { 1 } else { 0 }]
}

/// `GS ! n`: double width, double height, both or neither.
pub const fn size(style: Style) -> [u8; 3] {
    let width = if style.wide { 0x10 } else { 0 };
    let height = if style.tall { 0x01 } else { 0 };
    [0x1D, 0x21, width | height]
}

/// `doc`, rendered for `printer`.
pub fn render(doc: &Doc, printer: &PrinterProfile) -> Vec<u8> {
    let mut out = Vec::new();
    for command in
        [&INITIALIZE[..], &CODE_PAGE_1252, &INTERNATIONAL_USA, &CANCEL_KANJI, &ALIGN_LEFT]
    {
        out.extend_from_slice(command);
    }
    let mut current = Style::default();
    for laid in layout(doc, printer.columns) {
        match laid {
            Laid::Line(line) => {
                if line.style.bold != current.bold {
                    out.extend(bold(line.style.bold));
                }
                if (line.style.wide, line.style.tall) != (current.wide, current.tall) {
                    out.extend(size(line.style));
                }
                current = line.style;
                out.extend(line.text);
                out.push(LINE_FEED);
            }
            Laid::Cut if printer.cutter => out.extend(CUT),
            Laid::Cut => out.extend([LINE_FEED; 3]),
            Laid::DrawerKick => out.extend(DRAWER_KICK),
        }
    }
    out
}

/// `doc`, as plain text: its lines, each ended by a newline, whatever their style. Cuts and
/// drawer kicks leave nothing.
pub fn plain_text(doc: &Doc, columns: Columns) -> String {
    let mut out = String::new();
    for laid in layout(doc, columns) {
        if let Laid::Line(line) = laid {
            out.push_str(&crate::text::decode(&line.text));
            out.push('\n');
        }
    }
    out
}

/// `DLE EOT n`, a real-time question: the printer answers at once, wherever it finds it.
pub const fn status(question: Question) -> [u8; 3] {
    let n = match question {
        Question::Printer => 1,
        Question::Offline => 2,
        Question::Error => 3,
        Question::Paper => 4,
    };
    [0x10, 0x04, n]
}

/// What `DLE EOT` asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Question {
    /// Whether the printer is online.
    Printer,
    /// Why it is offline.
    Offline,
    /// What its error is.
    Error,
    /// Its roll paper.
    Paper,
}

/// Whether `byte` is a `DLE EOT` answer: bits 1 and 4 set, bits 0 and 7 clear.
pub const fn is_status(byte: u8) -> bool {
    byte & 0b1001_0011 == 0b0001_0010
}

/// The answer to `DLE EOT 1`: whether the printer is offline (bit 3).
pub const fn offline(byte: u8) -> bool {
    byte & 0x08 != 0
}

/// Why a printer is offline: the answer to `DLE EOT 2`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[expect(clippy::struct_excessive_bools, reason = "the bits of a status byte")]
pub struct OfflineCause {
    /// The cover is open (bit 2).
    pub cover_open: bool,
    /// Paper is being fed by the feed button (bit 3).
    pub feeding: bool,
    /// Printing stopped at the paper's end (bit 5).
    pub paper_end: bool,
    /// An error occurred (bit 6).
    pub error: bool,
}

impl OfflineCause {
    /// What `byte` says.
    pub const fn of(byte: u8) -> OfflineCause {
        OfflineCause {
            cover_open: byte & 0x04 != 0,
            feeding: byte & 0x08 != 0,
            paper_end: byte & 0x20 != 0,
            error: byte & 0x40 != 0,
        }
    }
}

/// The roll paper's state: the answer to `DLE EOT 4`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Paper {
    /// Near its end (bits 2 and 3).
    pub near_end: bool,
    /// At its end (bits 5 and 6).
    pub end: bool,
}

impl Paper {
    /// What `byte` says.
    pub const fn of(byte: u8) -> Paper {
        Paper { near_end: byte & 0x0C != 0, end: byte & 0x60 != 0 }
    }
}

/// `GS ( H` function 48 with process ID `id`: the printer answers [`printed`] once what came
/// before it has printed.
pub const fn ask_printed(id: [u8; 4]) -> [u8; 11] {
    let [a, b, c, d] = id;
    [0x1D, 0x28, 0x48, 6, 0, 48, 48, a, b, c, d]
}

/// The answer to [`ask_printed`] for `id`: `37 22 d1 d2 d3 d4 00`.
pub const fn printed(id: [u8; 4]) -> [u8; 7] {
    let [a, b, c, d] = id;
    [0x37, 0x22, a, b, c, d, 0]
}

/// A process ID, four characters from `0x20` to `0x7E`, from `n`: its last four decimal digits.
pub fn process_id(n: u64) -> [u8; 4] {
    let digit = |place: u64| {
        let value = n.checked_div(place).and_then(|value| value.checked_rem(10)).unwrap_or(0);
        u8::try_from(value).map_or(b'0', |value| b'0'.saturating_add(value))
    };
    [digit(1000), digit(100), digit(10), digit(1)]
}

/// Whether `byte` is an answer to `GS r 1`: bits 4 and 7 clear.
pub const fn is_processed(byte: u8) -> bool {
    byte & 0x90 == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Align, Block};

    #[test]
    fn commands_are_their_bytes() {
        assert_eq!(bold(true), [0x1B, 0x45, 0x01]);
        assert_eq!(bold(false), [0x1B, 0x45, 0x00]);
        assert_eq!(size(Style::default()), [0x1D, 0x21, 0x00]);
        assert_eq!(size(Style { wide: true, ..Style::default() }), [0x1D, 0x21, 0x10]);
        assert_eq!(size(Style { tall: true, ..Style::default() }), [0x1D, 0x21, 0x01]);
        assert_eq!(size(Style::LARGE), [0x1D, 0x21, 0x11]);
        assert_eq!(status(Question::Printer), [0x10, 0x04, 0x01]);
        assert_eq!(status(Question::Offline), [0x10, 0x04, 0x02]);
        assert_eq!(status(Question::Error), [0x10, 0x04, 0x03]);
        assert_eq!(status(Question::Paper), [0x10, 0x04, 0x04]);
        assert_eq!(
            ask_printed(*b"0042"),
            [0x1D, 0x28, 0x48, 0x06, 0x00, 0x30, 0x30, 0x30, 0x30, 0x34, 0x32]
        );
        assert_eq!(printed(*b"0042"), [0x37, 0x22, 0x30, 0x30, 0x34, 0x32, 0x00]);
        assert_eq!(process_id(0), *b"0000");
        assert_eq!(process_id(123_456), *b"3456");
        assert_eq!(process_id(u64::MAX), *b"1615");
    }

    #[test]
    fn a_document_renders_as_its_commands_and_lines() {
        let doc = Doc::new(vec![
            Block::DrawerKick,
            Block::text("Keel", Align::Center, Style::LARGE),
            Block::row("Tea", "$2"),
            Block::Cut,
        ])
        .unwrap();
        let printer =
            PrinterProfile { columns: Columns::new(24).unwrap(), ..PrinterProfile::EPSON_80MM };
        let mut expected =
            vec![0x1B, 0x40, 0x1B, 0x74, 0x10, 0x1B, 0x52, 0x00, 0x1C, 0x2E, 0x1B, 0x61, 0x00];
        expected.extend([0x1B, 0x70, 0x00, 0x19, 0xFA]);
        expected.extend([0x1B, 0x45, 0x01, 0x1D, 0x21, 0x11]);
        expected.extend(b"    Keel\n");
        expected.extend([0x1B, 0x45, 0x00, 0x1D, 0x21, 0x00]);
        expected.extend(b"Tea                   $2\n");
        expected.extend([0x1D, 0x56, 0x42, 0x00]);
        assert_eq!(render(&doc, &printer), expected);
        // Without a cutter, three feeds past the tear bar.
        let tear = PrinterProfile { cutter: false, ..printer };
        assert!(render(&doc, &tear).ends_with(b"$2\n\n\n\n"));
        assert_eq!(plain_text(&doc, printer.columns), "    Keel\nTea                   $2\n");
    }

    #[test]
    fn status_bytes_read_as_the_printer_means() {
        assert!(is_status(0x12));
        assert!(!is_status(0x13) && !is_status(0x10) && !is_status(0x92) && !is_status(0x02));
        assert!(!offline(0x12));
        assert!(offline(0x1A));
        assert_eq!(OfflineCause::of(0x12), OfflineCause::default());
        assert_eq!(
            OfflineCause::of(0x16),
            OfflineCause { cover_open: true, ..OfflineCause::default() }
        );
        assert_eq!(
            OfflineCause::of(0x32),
            OfflineCause { paper_end: true, ..OfflineCause::default() }
        );
        assert_eq!(OfflineCause::of(0x52), OfflineCause { error: true, ..OfflineCause::default() });
        assert_eq!(
            OfflineCause::of(0x1A),
            OfflineCause { feeding: true, ..OfflineCause::default() }
        );
        assert_eq!(Paper::of(0x12), Paper::default());
        assert_eq!(Paper::of(0x1E), Paper { near_end: true, end: false });
        assert_eq!(Paper::of(0x72), Paper { near_end: false, end: true });
        assert_eq!(Paper::of(0x7E), Paper { near_end: true, end: true });
        assert!(
            is_processed(0x00) && is_processed(0x0F) && !is_processed(0x37) && !is_processed(0x80)
        );
    }
}
