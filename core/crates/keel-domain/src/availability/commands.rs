//! What a device records of an item's availability, checked against it as the device sees it
//! (ADR-0033). Each refuses what the fold would report, a change at another location or of the
//! other kind of item, and a change that changes nothing or comes back on sale in the past.

use keel_events::envelope::Location;
use keel_types::{Id, Timestamp};

use super::events::{AvailabilityChanged, AvailabilityEvent};
use super::state::Availability;
use crate::codec::PayloadError;
use crate::schema::{SchemaError, Unrecordable, check_recordable};

/// What a device asks to record of an item's availability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AvailabilityCommand {
    /// Change it.
    Change(AvailabilityChanged),
}

/// Why an availability command was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AvailabilityError {
    /// The item's availability belongs to another location.
    #[error("the item's availability belongs to another location")]
    WrongLocation,
    /// The item is the other kind than its availability says.
    #[error("the item is another kind than its availability")]
    WrongKind,
    /// The item is already so: on sale, or sold out until the same time.
    #[error("the item is already sold out until then, or already on sale")]
    NoChange,
    /// An 86 until a time that isn't after now.
    #[error("an 86 ends after it is made")]
    UntilPast,
    /// The event wouldn't satisfy its schema, such as an item put back until a time.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl From<Unrecordable> for AvailabilityError {
    fn from(unrecordable: Unrecordable) -> AvailabilityError {
        match unrecordable {
            Unrecordable::Payload(invalid) => AvailabilityError::Invalid(invalid),
            Unrecordable::Schema(schema) => AvailabilityError::Schema(schema),
        }
    }
}

impl Availability {
    /// Checks `command`, from a device at `location` at `now`, against the item's availability as
    /// the device sees it, and returns the event to record.
    ///
    /// # Errors
    /// [`AvailabilityError`] saying why the command isn't allowed.
    pub fn decide(
        &self,
        location: Id<Location>,
        now: Timestamp,
        command: AvailabilityCommand,
    ) -> Result<AvailabilityEvent, AvailabilityError> {
        let AvailabilityCommand::Change(changed) = command;
        let event = AvailabilityEvent::Changed(changed.clone());
        event.validate().map_err(AvailabilityError::Invalid)?;
        if self.location().is_some_and(|known| known != location) {
            return Err(AvailabilityError::WrongLocation);
        }
        if let Some(latest) = self.latest()
            && latest.changed.kind != changed.kind
        {
            return Err(AvailabilityError::WrongKind);
        }
        if changed.until.is_some_and(|until| until <= now) {
            return Err(AvailabilityError::UntilPast);
        }
        let sold_out = self.sold_out_at(now);
        let same_until = self.latest().map(|latest| latest.changed.until) == Some(changed.until);
        if (changed.available && !sold_out) || (!changed.available && sold_out && same_until) {
            return Err(AvailabilityError::NoChange);
        }
        check_recordable(&event)?;
        Ok(event)
    }
}
