//! Orders: the universal transaction (domain model §6).
//!
//! A café sale, a table's dinner and an online pickup order are all one `Order` aggregate. This
//! version covers creating an order and its attributes, and its lines: adding, changing,
//! removing, firing, voiding and comping them, and voiding or abandoning the whole order.
//! Adjustments, checks and payments come later.
//!
//! - [`OrderEvent`] lists what can happen to an order, and its payloads (schemas version 1).
//! - [`Order`] is the state folded from those events. The fold is total, and resolves concurrent
//!   edits from different devices as the offline and sync design describes.
//! - [`OrderCommand`] is what a device asks to do; [`Order::decide`] checks it against the
//!   device's view and returns the event to record.

mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;
mod types;

pub use commands::{CommandError, OrderCommand};
pub use events::{AttributesChanged, LineAdded, LineChanged, OrderCreated, OrderEvent};
pub use state::{
    Conflict, ConflictKind, Line, LineStatus, Order, OrderInfo, OrderStatus, Skipped, Stage,
};
pub use types::{Channel, ChosenModifier, ItemSnapshot, Mode, Placement, Prefix, Reason};
