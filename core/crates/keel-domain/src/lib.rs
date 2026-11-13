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
//! - [`order`]: an order's creation and attributes, its lines, its checks, closing it, and its
//!   lines returned;
//! - [`payment`]: money moving against a check, in cash or by card;
//! - [`drawer`]: a drawer session's cash, from its float to its blind close (ADR-0031);
//! - [`refund`]: money paid back for lines returned (ADR-0032);
//! - [`availability`]: whether an item is on sale, or 86'd (ADR-0033).
//!
//! [`checkout`] holds the rules that span an order and its payments: what each check still owes,
//! starting a payment, closing a check, and money that doesn't fit the order. [`sequence`] holds
//! the Store Hub's sequencing records, which number the store's events, and [`hub`] the claims by
//! which a replica takes the hub's role, and the rules that say which hub's records count.
//! [`staff`] holds roles, permissions, PINs and approvals (ADR-0029). [`profile`] holds a
//! location's profile: its settings, pricing rules, catalog, menu and team,
//! and the rules by which a cashier's choices ring as a line; [`publication`], how the cloud
//! publishes a new one, as events.
//!
//! Like the rest of the kernel, this crate is deterministic, never panics, and builds for
//! `wasm32`.

pub mod aggregate;
pub mod availability;
pub mod checkout;
pub mod codec;
pub mod drawer;
pub mod hub;
pub mod order;
pub mod payment;
pub mod profile;
pub mod publication;
pub mod refs;
pub mod refund;
pub mod roster;
pub mod schema;
pub mod sequence;
pub mod staff;
