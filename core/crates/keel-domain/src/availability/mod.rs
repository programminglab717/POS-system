//! Availability (ADR-0033): whether an item, a variant or a modifier, is on sale at a location,
//! or 86'd: sold out until a time, or until it is put back.
//!
//! An item's availability is its own aggregate, its identifier the item's. Any device writes it:
//! the latest change by canonical order, which follows the events' HLCs, wins, so every device
//! agrees on which stands (offline and sync §6.5).
//!
//! - [`AvailabilityEvent`] lists what can happen to it, and its payload (schema version 1).
//! - [`Availability`] is the state folded from those events.
//! - [`AvailabilityCommand`] is what a device records; [`Availability::decide`] checks it.

mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;

pub use commands::{AvailabilityCommand, AvailabilityError};
pub use events::{AvailabilityChanged, AvailabilityEvent, ItemKind};
pub use state::{Availability, Change, Conflict, ConflictKind};
