//! Orders: the universal transaction (domain model §6).
//!
//! A café sale, a table's dinner and an online pickup order are all one `Order` aggregate. This
//! version covers creating an order and its attributes, and its lines: adding, changing,
//! removing, firing, voiding and comping them, and voiding or abandoning the whole order. An
//! order's lines are split among its [`Check`]s, in whole shares. A check closes once its
//! payments cover it, with a snapshot of what it was charged, and the order closes once its
//! checks have. Adjustments come later.
//!
//! - [`OrderEvent`] lists what can happen to an order, and its payloads (schemas version 1).
//! - [`Order`] is the state folded from those events. The fold is total, and resolves concurrent
//!   edits from different devices as the offline and sync design describes.
//! - [`OrderCommand`] is what a device asks to do; [`Order::decide`] checks it against the
//!   device's view and returns the event to record.
//! - [`Order::check_basket`] is what `keel-pricing` prices for a check: the lines allocated to
//!   it, with its share of the lines it shares with other checks.
//! - [`CheckClosed`] is a closed check's snapshot. Closing a check needs its payments, so
//!   [`crate::checkout`] decides it.

mod basket;
mod checks;
mod closing;
mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;
mod types;

pub use checks::{Allocation, Check, CheckShare, LinesAllocated};
pub use closing::{CheckClosed, LineCharge, TaxCharge};
pub use commands::{CommandError, OrderCommand};
pub use events::{AttributesChanged, LineAdded, LineChanged, OrderCreated, OrderEvent};
pub use state::{Conflict, ConflictKind, Line, LineStatus, Order, OrderInfo, OrderStatus, Stage};

pub use crate::aggregate::Skipped;
pub use types::{Channel, ChosenModifier, ItemSnapshot, Mode, Placement, Prefix, Reason};
