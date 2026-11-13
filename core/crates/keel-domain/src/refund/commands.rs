//! What a device records of a refund, checked against the refund as it sees it (ADR-0032). Each
//! refuses what the fold would report: a refund written by another device or at another
//! location, started twice, completed twice, before it is started, in another currency, or for
//! more than it was started for.

use core::cmp::Ordering;

use keel_events::envelope::{Device, Location};
use keel_types::Id;

use super::events::{RefundCompleted, RefundEvent, RefundInitiated};
use super::state::Refund;
use crate::codec::PayloadError;
use crate::schema::{SchemaError, Unrecordable, check_recordable};

/// What a device asks to record of a refund.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundCommand {
    /// Start it.
    Initiate(RefundInitiated),
    /// Record that the money was paid back.
    Complete(RefundCompleted),
}

/// Why a refund command was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RefundError {
    /// The refund was started already.
    #[error("the refund was started already")]
    AlreadyInitiated,
    /// The refund hasn't been started.
    #[error("the refund hasn't been started")]
    NotInitiated,
    /// The refund was started at another location.
    #[error("the refund was started at another location")]
    WrongLocation,
    /// Another device started the refund, and alone writes it.
    #[error("another device started the refund")]
    NotInitiator,
    /// An amount in another currency than the refund's.
    #[error("an amount is in another currency than the refund's")]
    WrongCurrency,
    /// The refund was completed already.
    #[error("the refund was completed already")]
    AlreadyCompleted,
    /// More than the refund was started for.
    #[error("more than the refund is for")]
    BeyondInitiated,
    /// The event wouldn't satisfy its schema, such as an amount of zero.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl From<Unrecordable> for RefundError {
    fn from(unrecordable: Unrecordable) -> RefundError {
        match unrecordable {
            Unrecordable::Payload(invalid) => RefundError::Invalid(invalid),
            Unrecordable::Schema(schema) => RefundError::Schema(schema),
        }
    }
}

impl Refund {
    /// Checks `command`, from `device` at `location`, against the refund as this device sees it,
    /// and returns the event to record.
    ///
    /// # Errors
    /// [`RefundError`] saying why the command isn't allowed.
    pub fn decide(
        &self,
        location: Id<Location>,
        device: Id<Device>,
        command: RefundCommand,
    ) -> Result<RefundEvent, RefundError> {
        let event = match command {
            RefundCommand::Initiate(initiated) => {
                if self.info().is_some() {
                    return Err(RefundError::AlreadyInitiated);
                }
                RefundEvent::Initiated(initiated)
            }
            RefundCommand::Complete(completed) => {
                let info = self.info().ok_or(RefundError::NotInitiated)?;
                if info.location != location {
                    return Err(RefundError::WrongLocation);
                }
                if info.device != device {
                    return Err(RefundError::NotInitiator);
                }
                if self.completed().is_some() {
                    return Err(RefundError::AlreadyCompleted);
                }
                match completed.amount.compare(info.amount) {
                    Err(_) => return Err(RefundError::WrongCurrency),
                    Ok(Ordering::Greater) => return Err(RefundError::BeyondInitiated),
                    Ok(_) => {}
                }
                RefundEvent::Completed(completed)
            }
        };
        event.validate().map_err(RefundError::Invalid)?;
        check_recordable(&event)?;
        Ok(event)
    }
}
