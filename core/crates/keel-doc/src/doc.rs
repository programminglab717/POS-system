//! `KeelDoc` v0 (ADR-0024, decision 2): a document is a list of blocks.
//!
//! It is canonical CBOR, `{1: format, 2: [block, ...]}`, decoded strictly, each block an array
//! whose first element is its kind:
//!
//! | Block | Encoding |
//! |---|---|
//! | text | `[0, text, align, style]`: align 0 left, 1 centred, 2 right |
//! | row | `[1, left, right, indent, style]` |
//! | rule | `[2]` |
//! | feed | `[3, lines]`, 1 to [`MAX_FEED`] |
//! | cut | `[4]` |
//! | drawer kick | `[5]` |
//!
//! A style is a number: 1 bold, 2 wide, 4 tall, added up.

use keel_events::cbor::{self, CborError, Map, Value};

/// The document format this crate reads and writes.
pub const FORMAT: u64 = 1;
/// The most blocks a document holds.
pub const MAX_BLOCKS: usize = 1_000;
/// The most bytes of text a block holds, in each of its texts.
pub const MAX_TEXT: usize = 1_000;
/// The most lines a feed adds.
pub const MAX_FEED: u8 = 9;
/// The most columns a row is indented by.
pub const MAX_INDENT: u8 = 8;

/// How a text sits on its lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    /// Against the left edge.
    Left,
    /// In the middle.
    Center,
    /// Against the right edge.
    Right,
}

/// How a block's text is printed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    /// Emphasized.
    pub bold: bool,
    /// Double width: each character takes two columns.
    pub wide: bool,
    /// Double height.
    pub tall: bool,
}

impl Style {
    /// Bold, and double in width and height: a heading or a total.
    pub const LARGE: Style = Style { bold: true, wide: true, tall: true };
    /// Bold only.
    pub const BOLD: Style = Style { bold: true, wide: false, tall: false };

    const fn code(self) -> u64 {
        let bold = if self.bold { 1 } else { 0 };
        let wide = if self.wide { 2 } else { 0 };
        let tall = if self.tall { 4 } else { 0 };
        bold | wide | tall
    }

    const fn from_code(code: u64) -> Option<Style> {
        if code > 7 {
            return None;
        }
        Some(Style { bold: code & 1 != 0, wide: code & 2 != 0, tall: code & 4 != 0 })
    }
}

/// A block of a document.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Block {
    /// A paragraph, wrapped at spaces, a word longer than a line broken where it must be.
    Text {
        /// The text.
        text: String,
        /// How it sits on its lines.
        align: Align,
        /// How it is printed.
        style: Style,
    },
    /// A left text, wrapped in the columns the right leaves it, and a right text at the end of
    /// its first line, such as an item and its price.
    Row {
        /// The left text.
        left: String,
        /// The right text.
        right: String,
        /// The columns every line of the left text is indented by.
        indent: u8,
        /// How both are printed.
        style: Style,
    },
    /// A line of `-` across the paper.
    Rule,
    /// Empty lines.
    Feed(u8),
    /// A cut, or feeding past the tear bar where the printer has no cutter.
    Cut,
    /// The cash drawer's pulse.
    DrawerKick,
}

impl Block {
    /// A text, aligned `align`, in `style`.
    pub fn text(text: impl Into<String>, align: Align, style: Style) -> Block {
        Block::Text { text: text.into(), align, style }
    }

    /// A row of `left` and `right`, unindented and plain.
    pub fn row(left: impl Into<String>, right: impl Into<String>) -> Block {
        Block::Row { left: left.into(), right: right.into(), indent: 0, style: Style::default() }
    }
}

/// A document: blocks, within their limits.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Doc {
    blocks: Vec<Block>,
}

/// Why a document can't be made or read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DocError {
    /// The bytes aren't canonical CBOR.
    #[error("the document isn't canonical CBOR: {0}")]
    Cbor(#[from] CborError),
    /// A format this crate doesn't read.
    #[error("document format {0} isn't one this crate reads")]
    Format(u64),
    /// The bytes aren't a document's encoding.
    #[error("the document's encoding is malformed: {0}")]
    Malformed(&'static str),
    /// A limit the document goes past.
    #[error("{0}")]
    Limit(&'static str),
}

impl Doc {
    /// A document of `blocks`.
    ///
    /// # Errors
    /// [`DocError::Limit`] for more than [`MAX_BLOCKS`] blocks, a text of more than
    /// [`MAX_TEXT`] bytes, a feed of none or more than [`MAX_FEED`] lines, or an indent of more
    /// than [`MAX_INDENT`].
    pub fn new(blocks: Vec<Block>) -> Result<Doc, DocError> {
        if blocks.len() > MAX_BLOCKS {
            return Err(DocError::Limit("more blocks than a document holds"));
        }
        for block in &blocks {
            match block {
                Block::Text { text, .. } if text.len() > MAX_TEXT => {
                    return Err(DocError::Limit("a text longer than a block holds"));
                }
                Block::Row { left, right, indent, .. } => {
                    if left.len() > MAX_TEXT || right.len() > MAX_TEXT {
                        return Err(DocError::Limit("a text longer than a block holds"));
                    }
                    if *indent > MAX_INDENT {
                        return Err(DocError::Limit("a row indented more than allowed"));
                    }
                }
                Block::Feed(lines) if *lines == 0 || *lines > MAX_FEED => {
                    return Err(DocError::Limit("a feed of no lines, or more than allowed"));
                }
                _ => {}
            }
        }
        Ok(Doc { blocks })
    }

    /// The blocks.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The document with `before` ahead of its blocks and `after` behind them.
    ///
    /// # Errors
    /// [`DocError::Limit`] if the blocks together go past a limit.
    pub fn around(&self, before: Vec<Block>, after: Vec<Block>) -> Result<Doc, DocError> {
        let mut blocks = before;
        blocks.extend(self.blocks.iter().cloned());
        blocks.extend(after);
        Doc::new(blocks)
    }

    /// The canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        let blocks = self.blocks.iter().map(block_value).collect();
        let entries = vec![
            (Value::Unsigned(1), Value::Unsigned(FORMAT)),
            (Value::Unsigned(2), Value::Array(blocks)),
        ];
        // Two distinct keys: building the map can't fail.
        Map::from_entries(entries).map(Value::Map).map(|value| value.encode()).unwrap_or_default()
    }

    /// The document `bytes` encode.
    ///
    /// # Errors
    /// [`DocError`] if `bytes` aren't a document of format [`FORMAT`] in canonical CBOR, or it
    /// goes past a limit.
    pub fn decode(bytes: &[u8]) -> Result<Doc, DocError> {
        let value = cbor::decode(bytes)?;
        let map = value.as_map().ok_or(DocError::Malformed("a document is a map"))?;
        if map.len() != 2 {
            return Err(DocError::Malformed("a document has a format and blocks, and no more"));
        }
        let format = map
            .get(&Value::Unsigned(1))
            .and_then(Value::as_u64)
            .ok_or(DocError::Malformed("format"))?;
        if format != FORMAT {
            return Err(DocError::Format(format));
        }
        let blocks = map
            .get(&Value::Unsigned(2))
            .and_then(Value::as_array)
            .ok_or(DocError::Malformed("blocks"))?;
        if blocks.len() > MAX_BLOCKS {
            return Err(DocError::Limit("more blocks than a document holds"));
        }
        let blocks = blocks
            .iter()
            .map(|block| block_from(block).ok_or(DocError::Malformed("a block")))
            .collect::<Result<Vec<_>, _>>()?;
        Doc::new(blocks)
    }
}

fn style_value(style: Style) -> Value {
    Value::Unsigned(style.code())
}

fn block_value(block: &Block) -> Value {
    let fields = match block {
        Block::Text { text, align, style } => {
            let align = match align {
                Align::Left => 0,
                Align::Center => 1,
                Align::Right => 2,
            };
            vec![
                Value::Unsigned(0),
                Value::from(text.as_str()),
                Value::Unsigned(align),
                style_value(*style),
            ]
        }
        Block::Row { left, right, indent, style } => vec![
            Value::Unsigned(1),
            Value::from(left.as_str()),
            Value::from(right.as_str()),
            Value::Unsigned(u64::from(*indent)),
            style_value(*style),
        ],
        Block::Rule => vec![Value::Unsigned(2)],
        Block::Feed(lines) => vec![Value::Unsigned(3), Value::Unsigned(u64::from(*lines))],
        Block::Cut => vec![Value::Unsigned(4)],
        Block::DrawerKick => vec![Value::Unsigned(5)],
    };
    Value::Array(fields)
}

fn block_from(value: &Value) -> Option<Block> {
    let fields = value.as_array()?;
    let (kind, rest) = fields.split_first()?;
    let style = |value: &Value| Style::from_code(value.as_u64()?);
    let text = |value: &Value| value.as_text().map(str::to_owned);
    Some(match (kind.as_u64()?, rest) {
        (0, [body, align, style_code]) => Block::Text {
            text: text(body)?,
            align: match align.as_u64()? {
                0 => Align::Left,
                1 => Align::Center,
                2 => Align::Right,
                _ => return None,
            },
            style: style(style_code)?,
        },
        (1, [left, right, indent, style_code]) => Block::Row {
            left: text(left)?,
            right: text(right)?,
            indent: u8::try_from(indent.as_u64()?).ok()?,
            style: style(style_code)?,
        },
        (2, []) => Block::Rule,
        (3, [lines]) => Block::Feed(u8::try_from(lines.as_u64()?).ok()?),
        (4, []) => Block::Cut,
        (5, []) => Block::DrawerKick,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_have_limits() {
        let limit = |blocks: Vec<Block>| Doc::new(blocks).map(|_| ()).unwrap_err();
        let long = "x".repeat(MAX_TEXT + 1);
        let row = |left: &str, right: &str, indent| Block::Row {
            left: left.to_owned(),
            right: right.to_owned(),
            indent,
            style: Style::default(),
        };
        let cases = [
            (vec![Block::Rule; MAX_BLOCKS + 1], "more blocks than a document holds"),
            (
                vec![Block::text(long.clone(), Align::Left, Style::default())],
                "a text longer than a block holds",
            ),
            (vec![row(&long, "", 0)], "a text longer than a block holds"),
            (vec![row("", &long, 0)], "a text longer than a block holds"),
            (vec![row("a", "b", MAX_INDENT + 1)], "a row indented more than allowed"),
            (vec![Block::Feed(0)], "a feed of no lines, or more than allowed"),
            (vec![Block::Feed(MAX_FEED + 1)], "a feed of no lines, or more than allowed"),
        ];
        for (blocks, why) in cases {
            assert_eq!(limit(blocks), DocError::Limit(why), "{why}");
        }
        let at = vec![
            Block::text("x".repeat(MAX_TEXT), Align::Left, Style::default()),
            row("a", "b", MAX_INDENT),
            Block::Feed(1),
            Block::Feed(MAX_FEED),
        ];
        assert!(Doc::new(at).is_ok());
        assert!(Doc::new(vec![Block::Rule; MAX_BLOCKS]).is_ok());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut text, byte| {
            let _ = core::fmt::Write::write_fmt(&mut text, format_args!("{byte:02x}"));
            text
        })
    }

    /// Found in review: only one block's kind and one style were pinned, so a change made to the
    /// encoder and decoder together, such as swapping wide and tall, passed every test, and the
    /// documents queued in a store would read differently after an upgrade. Every kind of block,
    /// every field of a row and every style bit, pinned against Python's `cbor2`:
    /// `cbor2.dumps({1: 1, 2: [[0, "Hi", 0, 1], [0, "Hi", 1, 2], [0, "Hi", 2, 4],
    /// [1, "Tea", "$2", 3, 0], [2], [3, 9], [4], [5]]}, canonical=True)`.
    #[test]
    fn every_kind_of_block_is_pinned() {
        let plain = Style::default();
        let doc = Doc::new(vec![
            Block::text("Hi", Align::Left, Style { bold: true, ..plain }),
            Block::text("Hi", Align::Center, Style { wide: true, ..plain }),
            Block::text("Hi", Align::Right, Style { tall: true, ..plain }),
            Block::Row { left: "Tea".to_owned(), right: "$2".to_owned(), indent: 3, style: plain },
            Block::Rule,
            Block::Feed(9),
            Block::Cut,
            Block::DrawerKick,
        ])
        .unwrap();
        let pinned = "a2010102888400624869000184006248690102840062486902048501635465616224320300810282030981048105";
        assert_eq!(hex(&doc.encode()), pinned);
        assert_eq!(Doc::decode(&doc.encode()).unwrap(), doc);
    }

    /// A document's encoding, with its blocks replaced by `blocks`.
    fn with_blocks(blocks: Vec<Value>) -> Vec<u8> {
        let entries = vec![
            (Value::Unsigned(1), Value::Unsigned(1)),
            (Value::Unsigned(2), Value::Array(blocks)),
        ];
        Value::Map(Map::from_entries(entries).unwrap()).encode()
    }

    #[test]
    fn encodings_other_than_a_documents_own_are_refused() {
        let doc =
            Doc::new(vec![Block::text("Hi", Align::Center, Style::LARGE), Block::Cut]).unwrap();
        // `{1: 1, 2: [[0, "Hi", 1, 7], [4]]}`.
        assert_eq!(
            doc.encode(),
            [0xA2, 0x01, 0x01, 0x02, 0x82, 0x84, 0x00, 0x62, b'H', b'i', 0x01, 0x07, 0x81, 0x04]
        );
        let text = |align: u64, style: u64| {
            Value::Array(vec![
                Value::Unsigned(0),
                Value::from("Hi"),
                Value::Unsigned(align),
                Value::Unsigned(style),
            ])
        };
        assert!(Doc::decode(&with_blocks(vec![text(2, 7)])).is_ok());
        for bad in [
            text(3, 0),
            text(0, 8),
            Value::Array(vec![Value::Unsigned(6)]),
            Value::Array(vec![Value::Unsigned(4), Value::Null]),
            Value::Array(vec![Value::Unsigned(3), Value::Unsigned(256)]),
            Value::Array(vec![]),
            Value::Unsigned(4),
        ] {
            assert_eq!(
                Doc::decode(&with_blocks(vec![bad.clone()])),
                Err(DocError::Malformed("a block")),
                "{bad:?}"
            );
        }
        let format_2 = Value::Map(
            Map::from_entries(vec![
                (Value::Unsigned(1), Value::Unsigned(2)),
                (Value::Unsigned(2), Value::Array(vec![])),
            ])
            .unwrap(),
        );
        assert_eq!(Doc::decode(&format_2.encode()), Err(DocError::Format(2)));
        let extra = Value::Map(
            Map::from_entries(vec![
                (Value::Unsigned(1), Value::Unsigned(1)),
                (Value::Unsigned(2), Value::Array(vec![])),
                (Value::Unsigned(3), Value::Null),
            ])
            .unwrap(),
        );
        assert!(matches!(Doc::decode(&extra.encode()), Err(DocError::Malformed(_))));
        let mut trailing = doc.encode();
        trailing.push(0);
        assert!(matches!(Doc::decode(&trailing), Err(DocError::Cbor(_))));
        assert_eq!(
            Doc::decode(&with_blocks(vec![Value::Array(vec![
                Value::Unsigned(3),
                Value::Unsigned(0)
            ])])),
            Err(DocError::Limit("a feed of no lines, or more than allowed"))
        );
    }
}
