//! # keel-store
//!
//! The device's store: every event it holds, what it derives from them, and the effects they
//! cause, in one SQLite database, encrypted (ADR-0016, ADR-0017, ADR-0018).
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
//! - **Projections** (ADR-0017): a row for each order and each payment, folded from its stream's
//!   events in canonical order ([`OrderSummary`], [`PaymentSummary`]). Each write recomputes the
//!   rows of the streams it touched, before it commits; a projection whose version changed is
//!   rebuilt from the log when the store opens. [`Store::load`] folds any aggregate.
//! - **The outbox** (ADR-0017): effects waiting to happen, such as printing or charging a card,
//!   enqueued in the write that records their cause, and started, finished, retried or failed in
//!   writes too ([`Effect`], [`EffectState`]). An effect found running after a restart is in
//!   doubt: its executor finds out what happened before anything else.
//! - **Encryption at rest** (ADR-0018): SQLCipher encrypts every page with AES-256 and
//!   authenticates it with HMAC-SHA512, under a [`StoreKey`] that the platform keeps safe and
//!   hands the store when it opens. A store opens only with its key ([`StoreError::KeyRejected`]),
//!   and [`Store::rekey`] changes it, proving the change took before it returns.
//! - **Damage** (ADR-0018): a store that meets a page that fails its authentication, or a file
//!   SQLite finds malformed, fails closed. It never returns what it read there, and refuses
//!   everything after it ([`StoreError::Damaged`]) until it is reopened.
//! - **Checks** (ADR-0018): opening checks what costs the same whatever the store holds, and says
//!   whether the store was closed cleanly last time ([`Store::recovered`]). [`Store::check`]
//!   checks everything, for the shell to run when the device is idle, and reports each
//!   [`Problem`] it finds.
//!
//! The database runs in WAL mode, and every commit is on disk before it returns. Crash-safety
//! tests interrupt writes and rekeys at each [`Point`], through [`Faults`].
//!
//! Unlike the kernel's portable crates, this one does I/O and links C code, SQLCipher's and
//! OpenSSL's, so it isn't built for `wasm32`: browsers are clients, never replicas.

mod check;
mod error;
mod faults;
mod key;
mod outbox;
mod projection;
mod rows;
mod schema;
mod store;
mod write;

#[cfg(test)]
mod tests;

pub use check::Problem;
pub use error::StoreError;
pub use faults::{Faults, NoFaults, Point};
pub use key::StoreKey;
pub use outbox::{
    Effect, EffectError, EffectKind, EffectState, Enqueued, MAX_KEY, MAX_PAYLOAD, Queued,
};
pub use projection::{OrderState, OrderSummary, PaymentState, PaymentSummary};
pub use schema::init_sqlite;
pub use store::{Store, StoreConfig};
pub use write::{Quarantined, Reason, Received, Writing};
