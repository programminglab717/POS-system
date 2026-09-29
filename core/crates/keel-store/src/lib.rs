//! # keel-store
//!
//! The device's store: every event it holds, in one SQLite database (ADR-0016).
//!
//! - **The device's own log.** [`Store`] owns the device's log writer. [`Writing::append`] makes,
//!   signs and stores the next event, which leaves the store only once its write commits.
//! - **Received events.** [`Writing::receive`] verifies an event from another replica with the
//!   device registry, and stores it if it is the next in its device's log. A duplicate changes
//!   nothing; after a gap, the caller asks for the rest in order; forks, broken chains and events
//!   that fail verification go to the quarantine, for a person to look at ([`Reason`]).
//! - **One transaction per write.** [`Store::write`] commits everything its closure stored, or
//!   nothing. If nothing, the device's log writer goes back to the log and clock as stored, so it
//!   never runs ahead of them.
//! - **Reads**: a device's log from any position, the version vector (how far into each device's
//!   log the store holds), a stream's events in canonical order, an event by identifier, and the
//!   quarantine.
//!
//! The database runs in WAL mode, and every commit is on disk before it returns. Crash-safety
//! tests interrupt writes at each [`Point`], through [`Faults`].
//!
//! Unlike the kernel's portable crates, this one does I/O and links SQLite's C code, so it isn't
//! built for `wasm32`: browsers are clients, never replicas.

mod error;
mod faults;
mod rows;
mod schema;
mod store;
mod write;

#[cfg(test)]
mod tests;

pub use error::StoreError;
pub use faults::{Faults, NoFaults, Point};
pub use store::{Store, StoreConfig};
pub use write::{Quarantined, Reason, Received, Writing};
