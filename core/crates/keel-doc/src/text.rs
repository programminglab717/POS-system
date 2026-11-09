//! Text for the printer (ADR-0024, decision 3): every text as Windows-1252, one byte to a
//! character, so that a column is a byte.
//!
//! - The characters the code page has map to their bytes.
//! - Spaces of other widths, no-break ones included, become a space; the minus sign and the
//!   hyphens the code page lacks become `-`.
//! - Letters with marks the code page lacks become their base letters (`ł` to `l`, `ő` to `o`),
//!   for Latin Extended-A; combining marks are dropped, so a letter written with one becomes its
//!   base letter.
//! - Control and format characters are dropped: C0, DEL, C1, the soft hyphen, and the zero-width
//!   and direction-changing marks.
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
        0xA0 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000 => Mapped::Byte(b' '),
        0xAD
        | 0x00..=0x1F
        | 0x7F..=0x9F
        | 0x0300..=0x036F
        | 0x200B..=0x200F
        | 0x2028..=0x202E
        | 0x2060..=0x206F
        | 0xFE00..=0xFE0F
        | 0xFEFF => Mapped::Dropped,
        0x0132 => Mapped::Two(b'I', b'J'),
        0x0133 => Mapped::Two(b'i', b'j'),
        0x2010..=0x2012 | 0x2015 | 0x2212 => Mapped::Byte(b'-'),
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
        assert_eq!(encode("EUR\u{a0}12.34"), b"EUR 12.34");
        assert_eq!(encode("12\u{202f}345\u{2009}6\u{3000}"), b"12 345 6 ");
        assert_eq!(encode("\u{2212}5 a\u{2010}b\u{2011}c\u{2012}d\u{2015}"), b"-5 a-b-c-d-");
    }

    #[test]
    fn letters_the_code_page_lacks_become_their_base_letters() {
        assert_eq!(encode("Łódź Kraków Gdańsk"), b"L\xf3dz Krak\xf3w Gdansk");
        assert_eq!(encode("Ő ű ř ğ ı İ Ĳsselmeer ĳs ſ"), b"O u r g i I IJsselmeer ijs s");
        // Combining marks are dropped: an e written with a combining acute is an e.
        assert_eq!(encode("e\u{301}"), b"e");
        for code in 0x100..=0x17F_u32 {
            let c = char::from_u32(code).unwrap();
            let bytes = encode(&c.to_string());
            assert!(bytes.iter().all(u8::is_ascii_alphabetic) || bytes.len() == 1, "{c}");
        }
    }

    #[test]
    fn controls_and_format_characters_are_dropped() {
        let text = "a\u{0}\u{4}\u{10}\u{1b}\n\t\u{7f}\u{85}\u{9b}\u{ad}\u{200b}\u{200e}\u{202e}\u{2066}\u{feff}\u{fe0f}b";
        assert_eq!(encode(text), b"ab");
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
