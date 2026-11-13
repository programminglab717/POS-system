//! Drawer sessions: the cash in a drawer, from its float to its blind close (domain model §13,
//! ADR-0031).
//!
//! A drawer session is its own aggregate, single-writer: the device the drawer is attached to,
//! which opened it, alone appends to it, so it never conflicts (offline and sync §6.2).
//!
//! - [`DrawerEvent`] lists what can happen to a session, and its payloads (schemas version 1).
//! - [`Drawer`] is the state folded from those events, with what it should hold.
//! - [`DrawerCommand`] is what the device records; [`Drawer::decide`] checks it.

mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;

pub use commands::{DrawerCommand, DrawerError};
pub use events::{CashMoved, CashRefunded, CashTaken, DrawerClosed, DrawerEvent, Movement};
pub use state::{Conflict, ConflictKind, Drawer, DrawerInfo, Sums};
