//! # keel-runtime
//!
//! The device runtime (ADR-0023): what a register's shell drives. It holds the device's store and
//! its location's profile, and turns what a cashier does into the kernel's commands, and the
//! store into what the register shows.
//!
//! - **Intents**, each one write of the store, so each happens entirely or not at all: start an
//!   order, add an item with its modifiers, rung from the catalog; change a line's quantity, take
//!   a line off, drop the order; and pay cash, which takes the payment, closes the order's check
//!   and the order, and answers with the change.
//! - **Views**, read whole: an order's ticket, the open orders, the menu, and an item's modifier
//!   groups. Every amount comes with the text the locale shows ([`Amount`]).
//! - **A team member signs in** from the profile's team, and every event records them; until one
//!   does, intents are refused.
//! - It reads the time only from its [`Clock`](keel_types::Clock) and draws randomness only from
//!   its [`Entropy`](keel_types::Entropy), and does no I/O but the store's, so tests and the
//!   simulator drive it as a device does.
//!
//! Like `keel-store`, which it holds, this crate isn't built for `wasm32`.

mod error;
mod runtime;
mod ticket;
mod views;

#[cfg(test)]
mod tests;

pub use error::RuntimeError;
pub use runtime::{Identity, Runtime};
pub use views::{
    Amount, CashPaid, GroupView, ItemView, MenuButton, MenuPage, MenuView, ModifierView, Ticket,
    TicketLine, TicketModifier, TicketState, TicketTax,
};
