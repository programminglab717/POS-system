//! What a device records of an item's stock, checked against it as the device sees it
//! (ADR-0036). Each refuses what the fold would report: an event at another location, or in
//! another unit than the stock's; and a release beyond what is reserved, or a count as of a time
//! after it is made.

use keel_events::envelope::Location;
use keel_types::{Id, Timestamp};

use super::events::{Movement, StockCounted, StockEvent, StockMoved};
use super::state::Stock;
use crate::codec::PayloadError;
use crate::schema::{SchemaError, Unrecordable, check_recordable};

/// What a device asks to record of an item's stock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StockCommand {
    /// Move it.
    Move(StockMoved),
    /// Count it.
    Count(StockCounted),
}

/// Why a stock command was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StockError {
    /// The item's stock belongs to another location.
    #[error("the item's stock belongs to another location")]
    WrongLocation,
    /// The quantity is in another unit than the item's stock.
    #[error("the item's stock is kept in another unit")]
    WrongUnit,
    /// A release of more than is reserved.
    #[error("more than is set aside")]
    BeyondReserved,
    /// A count as of a time after it is made.
    #[error("a count is as of a time no later than now")]
    CountedLater,
    /// The event wouldn't satisfy its schema, such as a quantity of zero.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl From<Unrecordable> for StockError {
    fn from(unrecordable: Unrecordable) -> StockError {
        match unrecordable {
            Unrecordable::Payload(invalid) => StockError::Invalid(invalid),
            Unrecordable::Schema(schema) => StockError::Schema(schema),
        }
    }
}

impl Stock {
    /// Checks `command`, from a device at `location` at `now`, against the item's stock as the
    /// device sees it, and returns the event to record.
    ///
    /// # Errors
    /// [`StockError`] saying why the command isn't allowed.
    pub fn decide(
        &self,
        location: Id<Location>,
        now: Timestamp,
        command: StockCommand,
    ) -> Result<StockEvent, StockError> {
        let event = match command {
            StockCommand::Move(moved) => StockEvent::Moved(moved),
            StockCommand::Count(counted) => StockEvent::Counted(counted),
        };
        event.validate().map_err(StockError::Invalid)?;
        if self.location().is_some_and(|known| known != location) {
            return Err(StockError::WrongLocation);
        }
        if self.unit().is_some_and(|unit| unit != event.quantity().unit()) {
            return Err(StockError::WrongUnit);
        }
        match &event {
            StockEvent::Moved(StockMoved { movement: Movement::Released, quantity, .. }) => {
                let reserved = self.reserved().map_err(|_| StockError::BeyondReserved)?;
                if quantity.compare(reserved).map_or(true, core::cmp::Ordering::is_gt) {
                    return Err(StockError::BeyondReserved);
                }
            }
            StockEvent::Counted(counted) if counted.as_of > now => {
                return Err(StockError::CountedLater);
            }
            _ => {}
        }
        check_recordable(&event)?;
        Ok(event)
    }
}
