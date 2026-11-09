//! A document laid out in a number of columns (ADR-0024, decision 2): lines of Windows-1252
//! bytes, each no wider than its style allows, which every renderer prints.

use crate::doc::{Align, Block, Doc, Style};
use crate::text;

/// The fewest columns a line has.
pub const MIN_COLUMNS: u8 = 24;
/// The most columns a line has.
pub const MAX_COLUMNS: u8 = 64;

/// The columns of a line, from [`MIN_COLUMNS`] to [`MAX_COLUMNS`], as a printer's paper and font
/// give them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Columns(u8);

impl Columns {
    /// 48 columns: 80 mm paper at 203 dpi.
    pub const PAPER_80MM: Columns = Columns(48);
    /// 42 columns: 80 mm paper at 180 dpi.
    pub const PAPER_80MM_180DPI: Columns = Columns(42);
    /// 32 columns: 58 mm paper.
    pub const PAPER_58MM: Columns = Columns(32);

    /// `columns`, if it is within the bounds.
    pub const fn new(columns: u8) -> Option<Columns> {
        if columns >= MIN_COLUMNS && columns <= MAX_COLUMNS { Some(Columns(columns)) } else { None }
    }

    /// How many.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// How many characters of `style` a line holds: half as many when they are wide.
    pub fn for_style(self, style: Style) -> usize {
        let columns = usize::from(self.0);
        if style.wide { columns / 2 } else { columns }
    }
}

/// A line of a laid-out document.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Line {
    /// Its characters, a byte each in Windows-1252, padded on the left as its alignment needs,
    /// with no trailing spaces but those of a row.
    pub text: Vec<u8>,
    /// How it is printed.
    pub style: Style,
}

/// What a document lays out as.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Laid {
    /// A line.
    Line(Line),
    /// A cut.
    Cut,
    /// The cash drawer's pulse.
    DrawerKick,
}

/// `doc`, laid out in `columns`.
pub fn layout(doc: &Doc, columns: Columns) -> Vec<Laid> {
    let mut out = Vec::new();
    for block in doc.blocks() {
        match block {
            Block::Text { text, align, style } => {
                let width = columns.for_style(*style);
                for line in wrap(&text::encode(text), width) {
                    out.push(line_of(aligned(line, width, *align), *style));
                }
            }
            Block::Row { left, right, indent, style } => {
                let width = columns.for_style(*style);
                for line in
                    row(&text::encode(left), &text::encode(right), usize::from(*indent), width)
                {
                    out.push(line_of(line, *style));
                }
            }
            Block::Rule => {
                out.push(line_of(
                    vec![b'-'; columns.for_style(Style::default())],
                    Style::default(),
                ));
            }
            Block::Feed(lines) => {
                for _ in 0..*lines {
                    out.push(line_of(Vec::new(), Style::default()));
                }
            }
            Block::Cut => out.push(Laid::Cut),
            Block::DrawerKick => out.push(Laid::DrawerKick),
        }
    }
    out
}

/// A line, its no-break spaces printed as spaces.
fn line_of(mut text: Vec<u8>, style: Style) -> Laid {
    for byte in &mut text {
        if *byte == text::NO_BREAK_SPACE {
            *byte = b' ';
        }
    }
    Laid::Line(Line { text, style })
}

/// `text`'s words, filled into lines of at most `width`, a word longer than a line broken where
/// it must be. Spaces between words become one. Always at least one line, empty for no words.
fn wrap(text: &[u8], width: usize) -> Vec<Vec<u8>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    for word in text.split(|byte| *byte == b' ').filter(|word| !word.is_empty()) {
        if !current.is_empty()
            && current.len().saturating_add(1).saturating_add(word.len()) <= width
        {
            current.push(b' ');
            current.extend_from_slice(word);
            continue;
        }
        if !current.is_empty() {
            lines.push(core::mem::take(&mut current));
        }
        let mut rest = word;
        while let Some((head, tail)) =
            rest.split_at_checked(width).filter(|(_, tail)| !tail.is_empty())
        {
            lines.push(head.to_vec());
            rest = tail;
        }
        current = rest.to_vec();
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// `line`, padded on the left to sit as `align` says in `width`.
fn aligned(line: Vec<u8>, width: usize, align: Align) -> Vec<u8> {
    let room = width.saturating_sub(line.len());
    let pad = match align {
        Align::Left => 0,
        Align::Center => room / 2,
        Align::Right => room,
    };
    let mut padded = vec![b' '; pad];
    padded.extend(line);
    padded
}

/// A row's lines: `left` wrapped in what `right` leaves it, indented by `indent`, and `right` at
/// the end of the first line; or, when `right` takes more than half the line, or leaves `left`
/// less than a column and a space, `left` on lines of its own and `right` after it, aligned
/// right. Spaces between `right`'s words become one, as a paragraph's do.
fn row(left: &[u8], right: &[u8], indent: usize, width: usize) -> Vec<Vec<u8>> {
    let indent = indent.min(width.saturating_sub(1));
    let indented = |line: Vec<u8>| {
        let mut out = vec![b' '; indent];
        out.extend(line);
        out
    };
    let has_words = |text: &[u8]| text.iter().any(|byte| *byte != b' ');
    let right = wrap(right, usize::MAX).into_iter().next().unwrap_or_default();
    if right.is_empty() {
        return wrap(left, width.saturating_sub(indent)).into_iter().map(indented).collect();
    }
    let alone = right.len().saturating_mul(2) > width
        || width.saturating_sub(indent).saturating_sub(right.len()) < 2;
    if alone {
        let mut lines: Vec<Vec<u8>> = if has_words(left) {
            wrap(left, width.saturating_sub(indent)).into_iter().map(indented).collect()
        } else {
            Vec::new()
        };
        lines
            .extend(wrap(&right, width).into_iter().map(|line| aligned(line, width, Align::Right)));
        return lines;
    }
    let room = width.saturating_sub(indent).saturating_sub(right.len()).saturating_sub(1);
    let mut lines: Vec<Vec<u8>> = wrap(left, room).into_iter().map(indented).collect();
    if let Some(first) = lines.first_mut() {
        let pad = width.saturating_sub(first.len()).saturating_sub(right.len());
        first.extend(core::iter::repeat_n(b' ', pad));
        first.extend(right);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;

    fn lines(blocks: Vec<Block>, columns: u8) -> Vec<String> {
        let doc = Doc::new(blocks).unwrap();
        layout(&doc, Columns::new(columns).unwrap())
            .into_iter()
            .map(|laid| match laid {
                Laid::Line(line) => text::decode(&line.text),
                Laid::Cut => "<cut>".to_owned(),
                Laid::DrawerKick => "<kick>".to_owned(),
            })
            .collect()
    }

    fn plain(text: &str) -> Block {
        Block::text(text, Align::Left, Style::default())
    }

    #[test]
    fn text_wraps_at_spaces_and_breaks_words_too_long_for_a_line() {
        let text = "The quick brown fox jumps over the lazy dog";
        assert_eq!(
            lines(vec![plain(text)], 24),
            ["The quick brown fox", "jumps over the lazy dog"]
        );
        // A word exactly as wide as the line fills it; one wider is broken.
        let word = "a".repeat(24);
        assert_eq!(lines(vec![plain(&format!("x {word} y"))], 24), ["x", &word, "y"]);
        let long = "b".repeat(50);
        assert_eq!(lines(vec![plain(&long)], 24), ["b".repeat(24), "b".repeat(24), "b".repeat(2)]);
        // Spaces between words become one, and an empty text is an empty line.
        assert_eq!(lines(vec![plain("  a    b  ")], 24), ["a b"]);
        assert_eq!(lines(vec![plain("")], 24), [""]);
    }

    #[test]
    fn text_is_aligned_by_padding_on_the_left() {
        let centred = Block::text("Keel", Align::Center, Style::default());
        let right = Block::text("Keel", Align::Right, Style::default());
        assert_eq!(
            lines(vec![centred, right], 24),
            [format!("{}Keel", " ".repeat(10)), format!("{}Keel", " ".repeat(20))]
        );
        // Wide text has half the columns: 12, so "Keel" is centred after 4.
        let wide = Block::text("Keel", Align::Center, Style::LARGE);
        assert_eq!(lines(vec![wide], 24), ["    Keel"]);
    }

    #[test]
    fn a_row_puts_its_right_text_at_the_end_of_its_first_line() {
        let row = Block::row("Latte (Large)", "$5.25");
        assert_eq!(lines(vec![row], 24), ["Latte (Large)      $5.25"]);
        // The left wraps in what the right leaves it: 24 - 6 - 1 = 17 columns.
        let row = Block::row("Breakfast sandwich with egg", "$12.95");
        assert_eq!(lines(vec![row], 24), ["Breakfast         $12.95", "sandwich with egg"]);
        // Indented, every line.
        let row = Block::Row {
            left: "Oat milk and more".to_owned(),
            right: "$0.75".to_owned(),
            indent: 2,
            style: Style::default(),
        };
        assert_eq!(lines(vec![row], 24), ["  Oat milk and     $0.75", "  more"]);
    }

    #[test]
    fn a_right_text_wider_than_half_the_line_goes_on_its_own() {
        let row = Block::row("Total", "$1,234,567,890.12");
        assert_eq!(lines(vec![row], 24), ["Total", "       $1,234,567,890.12"]);
        // At exactly half, it shares the line.
        let row = Block::row("Total", "123456789012");
        assert_eq!(lines(vec![row], 24), ["Total       123456789012"]);
        // A row with nothing on its right is its left, wrapped.
        assert_eq!(lines(vec![Block::row("Cash", "")], 24), ["Cash"]);
        // Nothing on its left is its right, aligned right.
        assert_eq!(lines(vec![Block::row("", "$1.00")], 24), [format!("{}$1.00", " ".repeat(19))]);
    }

    /// Found in review: no-break spaces became spaces before the layout, so an amount could break
    /// between its symbol and its digits.
    #[test]
    fn lines_never_break_at_a_no_break_space() {
        // With spaces it would be "Amount due today: EUR" and "12.34".
        let due = Block::text("Amount due today: EUR\u{a0}12.34", Align::Left, Style::default());
        assert_eq!(lines(vec![due], 24), ["Amount due today:", "EUR 12.34"]);
        let total = Block::text("Total: 1\u{202f}234\u{202f}567,89", Align::Left, Style::default());
        assert_eq!(lines(vec![total], 24), ["Total: 1 234 567,89"]);
        let amount = Block::text("Due EUR\u{a0}12.34 now", Align::Left, Style::default());
        assert_eq!(lines(vec![amount], 24), ["Due EUR 12.34 now"]);
        assert_eq!(
            lines(vec![Block::row("Cash", "-EUR\u{a0}12.34")], 24),
            ["Cash          -EUR 12.34"]
        );
    }

    #[test]
    fn rules_feeds_cuts_and_kicks() {
        let blocks = vec![Block::Rule, Block::Feed(2), Block::Cut, Block::DrawerKick];
        assert_eq!(
            lines(blocks, 32),
            ["-".repeat(32), String::new(), String::new(), "<cut>".to_owned(), "<kick>".to_owned()]
        );
    }

    #[test]
    fn columns_have_bounds() {
        assert_eq!(Columns::new(23), None);
        assert_eq!(Columns::new(65), None);
        assert_eq!(Columns::new(24).map(Columns::get), Some(24));
        assert_eq!(Columns::new(64).map(Columns::get), Some(64));
        assert_eq!(Columns::PAPER_80MM.for_style(Style::LARGE), 24);
    }
}
