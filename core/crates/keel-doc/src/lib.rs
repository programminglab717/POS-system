//! `KeelDoc` v0 ([ADR-0024](../../../../docs/adr/0024-receipts-and-printing.md)): Keel's
//! device-independent documents, such as receipts; their layout in a printer's columns; and
//! renderers to ESC/POS and to plain text, which print the same lines.
//!
//! - [`doc`]: the document, its blocks and its canonical encoding.
//! - [`text`]: text as Windows-1252, a byte to a column, with no control byte in it.
//! - [`layout`]: a document's lines in a number of columns.
//! - [`escpos`]: the ESC/POS renderer, the plain text one, and the printer's answers read.
//!
//! The crate is portable, like the kernel's, and does no I/O.

pub mod doc;
pub mod escpos;
pub mod layout;
pub mod text;

pub use doc::{Align, Block, Doc, DocError, Style};
pub use escpos::{Confirm, PrinterProfile};
pub use layout::Columns;
