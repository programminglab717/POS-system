//! # keel-domain
//!
//! Keel's business aggregates: what can happen to an order (and later to checks, payments and the
//! rest of the domain model), how each event is written, and how state is folded from events.
//!
//! Every aggregate follows the same pattern:
//!
//! - **Events** are one enum per aggregate, implementing [`schema::DomainEvent`]. Each event has
//!   a named, versioned schema, and a payload that is a canonical CBOR map with small integer
//!   keys ([`codec`]). Decoding is strict, so every payload has exactly one encoding.
//! - **State** is a fold over the aggregate's events in canonical order ([`aggregate`]). Folds are
//!   total: an event that doesn't fit the state, because its device acted concurrently on a
//!   different view, still has a defined outcome, and leaves a conflict for a person to review.
//!   Events a kernel can't decode are kept in the log but skipped, and the aggregate says so.
//! - **Commands** are what a device asks to do. They are checked against the device's current
//!   view and become events, so a device acting alone never creates a conflict.
//!
//! The aggregates so far:
//! - [`order`]: an order's creation and attributes, its lines, its checks, and closing it;
//! - [`payment`]: money moving against a check, in cash or by card.
//!
//! [`checkout`] holds the rules that span an order and its payments: what each check still owes,
//! starting a payment, closing a check, and money that doesn't fit the order.
//!
//! Like the rest of the kernel, this crate is deterministic, never panics, and builds for
//! `wasm32`.

pub mod aggregate;
pub mod checkout;
pub mod codec;
pub mod order;
pub mod payment;
pub mod refs;
pub mod schema;
