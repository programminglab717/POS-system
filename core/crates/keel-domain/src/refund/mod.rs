//! Refunds (ADR-0032): money paid back for lines returned, to the tender that paid.
//!
//! A refund is its own aggregate, single-writer: the device that made the return alone appends
//! to it. Its identifier is the one the return names (`order.lines_returned`).
//!
//! - [`RefundEvent`] lists what can happen to a refund, and its payloads (schemas version 1).
//! - [`Refund`] is the state folded from those events.
//! - [`RefundCommand`] is what the device records; [`Refund::decide`] checks it.

mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;

pub use commands::{RefundCommand, RefundError};
pub use events::{RefundCompleted, RefundEvent, RefundInitiated};
pub use state::{Conflict, ConflictKind, Refund, RefundInfo};
