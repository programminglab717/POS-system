//! The kitchen (ADR-0034): tickets, each what one station is to make for one firing of an
//! order's lines, and what the station did with it.
//!
//! A ticket is its own aggregate, its identifier drawn by the device that fired the lines, and
//! written in the fire's own write. Any device writes it: the screen that shows and bumps it,
//! the register that fired it, and reroutes it when no screen showed it.
//!
//! - [`KitchenEvent`] lists what can happen to a ticket, and its payload (schema version 1).
//! - [`KitchenTicket`] is the state folded from those events.
//! - [`KitchenCommand`] is what a device records; [`KitchenTicket::decide`] checks it.

mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;

pub use commands::{KitchenCommand, KitchenError};
pub use events::{
    KitchenEvent, KitchenItem, KitchenModifier, MAX_DEPTH, MAX_ITEMS, MAX_MODIFIERS, RerouteTo,
    TicketSent,
};
pub use state::{Conflict, ConflictKind, KitchenTicket, Moment};
