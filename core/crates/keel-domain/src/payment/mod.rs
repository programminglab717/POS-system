//! Payments: money moving against a check (domain model §7, ADR-0015).
//!
//! A payment is its own aggregate, so a terminal capturing a payment never conflicts with a
//! server adding a line. Its identifier is the idempotency key the terminal or processor sees,
//! so a crash, a replay or a retry can never charge twice. Version 1 has cash and card.
//!
//! - [`PaymentEvent`] lists what can happen to a payment, and its payloads (schemas version 1).
//! - [`Payment`] is the state folded from those events: a state machine that keeps money that
//!   may have moved, even when events arrive out of turn.
//! - [`PaymentCommand`] is what a device records once a payment has started;
//!   [`Payment::decide`] checks it. Starting a payment needs the order and its other payments:
//!   see [`crate::checkout`].

mod commands;
mod events;
mod state;
#[cfg(test)]
mod tests;

pub use commands::{PaymentCommand, PaymentError};
pub use events::{
    CashTendered, PaymentAuthorized, PaymentCaptured, PaymentEnded, PaymentEvent, PaymentInitiated,
    Tender,
};
pub use state::{Conflict, ConflictKind, Outcome, Payment, PaymentInfo, PaymentStatus};
