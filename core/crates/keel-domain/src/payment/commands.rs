//! Payment commands: a payment's outcome, as a device records it, checked against its view.
//!
//! A payment is started by [`crate::checkout`], which needs the order. After that, the device
//! driving the terminal or the drawer records what happened, and these commands check it:
//! - an outcome needs an initiated payment, at the device's location, that is unresolved;
//! - only a card is authorized, once, for no more than was asked for;
//! - a capture is for no more than the authorization, or than was asked for;
//! - a cash capture records the amount tendered, and a card capture doesn't; cash never has a
//!   processor reference.

use keel_events::envelope::Location;
use keel_types::Id;

use super::events::{PaymentAuthorized, PaymentCaptured, PaymentEnded, PaymentEvent, Tender};
use super::state::{Payment, PaymentStatus};
use crate::codec::PayloadError;
use crate::schema::{SchemaError, Unrecordable, check_recordable};

/// What a device asks to record about a payment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaymentCommand {
    /// Record that a card was authorized.
    Authorize(PaymentAuthorized),
    /// Record that the money moved.
    Capture(PaymentCaptured),
    /// Record that the attempt failed.
    Fail(PaymentEnded),
    /// Void the payment before it is captured.
    Void(PaymentEnded),
}

/// Why a payment command was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PaymentError {
    /// The payment hasn't been initiated.
    #[error("the payment hasn't been initiated")]
    NotInitiated,
    /// The payment belongs to another location than the device's.
    #[error("the payment belongs to another location")]
    WrongLocation,
    /// The payment was already captured, failed or voided.
    #[error("the payment already has an outcome")]
    Resolved,
    /// The card was already authorized.
    #[error("the payment is already authorized")]
    AlreadyAuthorized,
    /// The details don't fit the tender: cash isn't authorized and has no processor reference, a
    /// cash capture records the amount tendered, and a card capture doesn't.
    #[error("the details don't fit the payment's tender")]
    WrongTender,
    /// An amount in another currency than the payment's.
    #[error("an amount is in another currency than the payment's")]
    WrongCurrency,
    /// More than the payment asked for, or than was authorized.
    #[error("the amount is more than the payment's")]
    TooMuch,
    /// The event wouldn't satisfy its schema, such as an amount of zero.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl From<Unrecordable> for PaymentError {
    fn from(error: Unrecordable) -> PaymentError {
        match error {
            Unrecordable::Schema(error) => PaymentError::Schema(error),
            Unrecordable::Payload(error) => PaymentError::Invalid(error),
        }
    }
}

impl Payment {
    /// Checks `command`, from a device at `location`, against the payment as this device sees
    /// it, and returns the event to record.
    ///
    /// # Errors
    /// [`PaymentError`] saying why the command isn't allowed.
    pub fn decide(
        &self,
        location: Id<Location>,
        command: PaymentCommand,
    ) -> Result<PaymentEvent, PaymentError> {
        let info = self.info().ok_or(PaymentError::NotInitiated)?;
        if info.location != location {
            return Err(PaymentError::WrongLocation);
        }
        if !self.status().is_unresolved() {
            return Err(PaymentError::Resolved);
        }
        let cash = info.tender == Tender::Cash;
        let in_currency = |amount: keel_types::Money| amount.currency() == info.amount.currency();
        let at_most = |amount: keel_types::Money, limit: keel_types::Money| {
            amount.compare(limit).is_ok_and(core::cmp::Ordering::is_le)
        };
        let event = match command {
            PaymentCommand::Authorize(authorized) => {
                if cash {
                    return Err(PaymentError::WrongTender);
                }
                if matches!(self.status(), PaymentStatus::Authorized(_)) {
                    return Err(PaymentError::AlreadyAuthorized);
                }
                if !in_currency(authorized.amount) {
                    return Err(PaymentError::WrongCurrency);
                }
                if !at_most(authorized.amount, info.amount) {
                    return Err(PaymentError::TooMuch);
                }
                PaymentEvent::Authorized(authorized)
            }
            PaymentCommand::Capture(captured) => {
                if cash != captured.cash.is_some() || cash && captured.reference.is_some() {
                    return Err(PaymentError::WrongTender);
                }
                if !in_currency(captured.amount) {
                    return Err(PaymentError::WrongCurrency);
                }
                let limit = match self.status() {
                    PaymentStatus::Authorized(authorized) => authorized.amount,
                    _ => info.amount,
                };
                if !at_most(captured.amount, limit) {
                    return Err(PaymentError::TooMuch);
                }
                PaymentEvent::Captured(captured)
            }
            PaymentCommand::Fail(ended) | PaymentCommand::Void(ended)
                if cash && ended.reference.is_some() =>
            {
                return Err(PaymentError::WrongTender);
            }
            PaymentCommand::Fail(ended) => PaymentEvent::Failed(ended),
            PaymentCommand::Void(ended) => PaymentEvent::Voided(ended),
        };
        check_recordable(&event)?;
        Ok(event)
    }
}
