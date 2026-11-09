//! Text for the printer (ADR-0024, decision 3): every text as Windows-1252, one byte to a
//! character, so that a column is a byte.
//!
//! - The characters the code page has map to their bytes.
//! - Line breaks, tabs and spaces of other widths become a space, and no-break spaces the code
//!   page's own, `0xA0`, which the layout never breaks a line at and prints as a space; the minus
//!   sign and the hyphens the code page lacks become `-`.
//! - Letters with marks the code page lacks become their base letters (`ł` to `l`, `ő` to `o`),
//!   for Latin Extended-A; combining marks are dropped, so a letter written with one becomes its
//!   base letter.
//! - Other control and format characters are dropped: C0, DEL, C1, the soft hyphen, the
//!   zero-width and direction-changing marks, variation selectors and tags; and so are combining
//!   marks.
//! - Anything else becomes `?`.
//!
//! So no text yields a byte below `0x20`, nor `0x7F`.

/// The characters Windows-1252 puts at `0x80` to `0x9F`, where Latin-1 has controls; `None`
/// where it has nothing.
const HIGH: [Option<char>; 32] = [
    Some('€'),
    None,
    Some('‚'),
    Some('ƒ'),
    Some('„'),
    Some('…'),
    Some('†'),
    Some('‡'),
    Some('ˆ'),
    Some('‰'),
    Some('Š'),
    Some('‹'),
    Some('Œ'),
    None,
    Some('Ž'),
    None,
    None,
    Some('‘'),
    Some('’'),
    Some('“'),
    Some('”'),
    Some('•'),
    Some('–'),
    Some('—'),
    Some('˜'),
    Some('™'),
    Some('š'),
    Some('›'),
    Some('œ'),
    None,
    Some('ž'),
    Some('Ÿ'),
];

/// The base letter of each character of Latin Extended-A, `U+0100` to `U+017F`. Those the code
/// page has (`Œ œ Š š Ÿ Ž ž`) never reach this table, and the digraphs `Ĳ ĳ` are written as two.
const LATIN_EXTENDED_A: &[u8; 128] = b"AaAaAaCcCcCcCcDd\
DdEeEeEeEeEeGgGg\
GgGgHhHhIiIiIiIi\
IiIiJjKkkLlLlLlL\
lLlNnNnNnnNnOoOo\
OoOoRrRrRrSsSsSs\
SsTtTtTtUuUuUuUu\
UuUuWwYyYZzZzZzs";

/// The byte of a no-break space: the layout breaks lines only at `0x20`, and prints this as one.
pub const NO_BREAK_SPACE: u8 = 0xA0;

/// What a character becomes.
enum Mapped {
    /// One byte.
    Byte(u8),
    /// Two bytes, for a digraph.
    Two(u8, u8),
    /// Nothing.
    Dropped,
}

fn map(c: char) -> Mapped {
    let code = u32::from(c);
    match code {
        0x20..=0x7E | 0xA1..=0xAC | 0xAE..=0xFF => {
            u8::try_from(code).map_or(Mapped::Byte(b'?'), Mapped::Byte)
        }
        // No-break spaces stay no-break: the layout never breaks a line there, and prints a space.
        0xA0 | 0x2007 | 0x202F => Mapped::Byte(NO_BREAK_SPACE),
        // Line breaks and tabs separate words, as other spaces do.
        0x09..=0x0D
        | 0x85
        | 0x1680
        | 0x2000..=0x2006
        | 0x2008..=0x200A
        | 0x2028
        | 0x2029
        | 0x205F
        | 0x3000 => Mapped::Byte(b' '),
        // Controls, format characters and combining marks.
        0xAD
        | 0x00..=0x08
        | 0x0E..=0x1F
        | 0x7F..=0x84
        | 0x86..=0x9F
        | 0x0300..=0x036F
        | 0x061C
        | 0x180E
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x200B..=0x200F
        | 0x202A..=0x202E
        | 0x20D0..=0x20FF
        | 0x2060..=0x206F
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F
        | 0xFEFF
        | 0xFFF9..=0xFFFB
        | 0xE0000..=0xE007F => Mapped::Dropped,
        0x0132 => Mapped::Two(b'I', b'J'),
        0x0133 => Mapped::Two(b'i', b'j'),
        // Romanian's letters with a comma below.
        0x0218 => Mapped::Byte(b'S'),
        0x0219 => Mapped::Byte(b's'),
        0x021A => Mapped::Byte(b'T'),
        0x021B => Mapped::Byte(b't'),
        0x2010..=0x2012 | 0x2015 | 0x2043 | 0x2212 | 0xFE63 | 0xFF0D => Mapped::Byte(b'-'),
        _ => {
            if let Some(at) = HIGH.iter().position(|high| *high == Some(c)) {
                // `at` is below 32.
                return u8::try_from(at).map_or(Mapped::Byte(b'?'), |at| Mapped::Byte(0x80 | at));
            }
            let base = code
                .checked_sub(0x100)
                .and_then(|at| usize::try_from(at).ok())
                .and_then(|at| LATIN_EXTENDED_A.get(at));
            Mapped::Byte(base.copied().unwrap_or(b'?'))
        }
    }
}

/// `text` in Windows-1252, as decision 3 of ADR-0024 says.
pub fn encode(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        match map(c) {
            Mapped::Byte(byte) => bytes.push(byte),
            Mapped::Two(first, second) => bytes.extend([first, second]),
            Mapped::Dropped => {}
        }
    }
    bytes
}

/// The text Windows-1252 `bytes` stand for: what a printer prints for them. A byte the code page
/// leaves undefined reads as `U+FFFD`, and a control byte as itself.
pub fn decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| match byte {
            0x80..=0x9F => HIGH
                .get(usize::from(byte & 0x1F))
                .copied()
                .flatten()
                .unwrap_or(char::REPLACEMENT_CHARACTER),
            _ => char::from(byte),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_pages_own_characters_are_its_bytes() {
        assert_eq!(encode("Latte $4.50"), b"Latte $4.50");
        assert_eq!(encode("Café crème ñ ü ß ×"), b"Caf\xe9 cr\xe8me \xf1 \xfc \xdf \xd7");
        assert_eq!(
            encode("€5 – “quoted” … Œuvre ™"),
            b"\x805 \x96 \x93quoted\x94 \x85 \x8cuvre \x99"
        );
        // Every byte the code page defines reads back as the character it came from.
        for byte in (0x20..=0x7E).chain(0xA1..=0xFF).chain(0x80..=0x9F) {
            let text = decode(&[byte]);
            if text != "\u{FFFD}" && byte != 0xAD {
                assert_eq!(encode(&text), vec![byte], "{byte:#x} {text:?}");
            }
        }
    }

    #[test]
    fn other_spaces_and_dashes_become_the_code_pages() {
        assert_eq!(encode("EUR\u{a0}12.34"), b"EUR\xa012.34");
        assert_eq!(
            encode("12\u{202f}345\u{2007}6\u{2009}7\u{3000}\u{1680}"),
            b"12\xa0345\xa06 7  "
        );
        // Found in review: line breaks and tabs were dropped, gluing words together.
        assert_eq!(
            encode("123 Main St\nSpringfield\tNY\r\n\u{2028}x\u{2029}y\u{85}z"),
            b"123 Main St Springfield NY   x y z"
        );
        assert_eq!(encode("\u{2212}5 a\u{2010}b\u{2011}c\u{2012}d\u{2015}"), b"-5 a-b-c-d-");
        assert_eq!(encode("\u{2043}\u{fe63}\u{ff0d}"), b"---");
    }

    #[test]
    fn letters_the_code_page_lacks_become_their_base_letters() {
        assert_eq!(encode("Łódź Kraków Gdańsk"), b"L\xf3dz Krak\xf3w Gdansk");
        assert_eq!(encode("Ő ű ř ğ ı İ Ĳsselmeer ĳs ſ"), b"O u r g i I IJsselmeer ijs s");
        // Combining marks are dropped: an e written with a combining acute is an e.
        assert_eq!(encode("e\u{301}"), b"e");
        // Every letter of Latin Extended-A. Those Unicode decomposes become the first character of
        // their decomposition, as Python's `unicodedata.normalize("NFD", c)[0]` gives it; the 20
        // it doesn't decompose are named after the letter they look like; the code page's own
        // (`Œ œ Š š Ÿ Ž ž`) are themselves, and the digraphs two letters.
        let expected = "AaAaAaCcCcCcCcDdDdEeEeEeEeEeGgGgGgGgHhHhIiIiIiIiIiIiJjKkkLlLlLlLlLlNnNnNnnNnOoOoOoŒœRrRrRrSsSsSsŠšTtTtTtUuUuUuUuUuUuWwYyŸZzZzŽžs";
        for (code, expected) in (0x100..=0x17F_u32).zip(expected.chars()) {
            let c = char::from_u32(code).unwrap();
            let want = match c {
                'Ĳ' => b"IJ".to_vec(),
                'ĳ' => b"ij".to_vec(),
                _ => encode(&expected.to_string()),
            };
            assert_eq!(encode(&c.to_string()), want, "{c}");
        }
    }

    #[test]
    fn controls_and_format_characters_are_dropped() {
        let text = "a\u{0}\u{4}\u{10}\u{1b}\u{7f}\u{9b}\u{ad}\u{200b}\u{200e}\u{202e}\u{2066}\u{feff}\u{fe0f}b";
        assert_eq!(encode(text), b"ab");
        // Found in review: these printed as `?`. A direction mark, a vowel separator, annotation
        // marks, tags (England's flag is a black flag and six of them), and combining marks
        // outside the first block.
        let more = "a\u{61c}\u{180e}\u{fff9}\u{fffb}\u{e0067}\u{e007f}e\u{20dd}1\u{fe0f}\u{20e3}\u{1ab0}\u{1dc0}\u{fe20}b";
        assert_eq!(encode(more), b"ae1b");
        assert_eq!(encode("ș ț Ș Ț s\u{326}"), b"s t S T s");
    }

    #[test]
    fn anything_else_becomes_a_question_mark() {
        assert_eq!(encode("東京 ☕ Ω 🙂"), b"?? ? ? ?");
    }

    #[test]
    fn no_text_yields_a_control_byte() {
        // Every character in the first planes, one at a time.
        for code in (0..0x3_0000_u32).filter_map(char::from_u32) {
            for byte in encode(&code.to_string()) {
                assert!(byte >= 0x20 && byte != 0x7F, "{code:?} gives {byte:#x}");
            }
        }
    }
}
