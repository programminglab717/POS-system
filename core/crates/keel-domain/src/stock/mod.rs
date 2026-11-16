//! Stock (ADR-0036): an item's inventory at a location, as the sum of its movements.
//!
//! A variant's stock is its own aggregate, its identifier the variant's. Any device writes it,
//! with no owner: its events are movements and counts, which add up in any order, so devices that
//! record them offline agree once they hold the same events (offline and sync §6.1). Sales and
//! returns are not stock events: they are read from orders, and given to [`levels`] as flows.
//!
//! - [`StockEvent`] lists what can happen to an item's stock, and its payloads (schemas version 1).
//! - [`Stock`] is the state folded from those events: what moved and was counted, when.
//! - [`levels`] gives on hand, reserved and available, from the stock's entries and the flows
//!   orders made.
//! - [`StockCommand`] is what a device records; [`Stock::decide`] checks it.

mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;

pub use commands::{StockCommand, StockError};
pub use events::{Movement, StockCounted, StockEvent, StockMoved};
pub use state::{
    Conflict, ConflictKind, Count, Entry, EntryKind, Flow, Levels, Stock, levels, time_of,
};
