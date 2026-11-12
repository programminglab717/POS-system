//! Drawer commands: what the device the drawer is attached to records, checked against its view
//! (ADR-0031). Each refuses what the fold would report: a session written by another device
//! than its opener, or after its close; cash taken twice; an amount in another currency.

use keel_events::envelope::{Device, Location};
use keel_types::{Currency, Id, Money};

use super::events::{CashMoved, CashTaken, DrawerClosed, DrawerEvent, Movement};
use super::state::{Drawer, Sums, held};
use crate::codec::PayloadError;
use crate::order::Reason;
use crate::schema::{SchemaError, Unrecordable, check_recordable};

/// What a device asks to record about its drawer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawerCommand {
    /// Open the session with a float, in the location's currency.
    Open {
        /// The cash put in to begin.
        float: Money,
        /// The location's currency.
        currency: Currency,
    },
    /// Count a cash payment's cash into the drawer.
    TakeCash(CashTaken),
    /// Pay cash in or out, or drop it to the safe.
    MoveCash(CashMoved),
    /// Open the drawer with no sale.
    NoSale(Reason),
    /// Close the session with what was counted: the expected amount is the drawer's own.
    Close {
        /// What was counted.
        counted: Money,
    },
}

/// Why a drawer command was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DrawerError {
    /// The session was opened already.
    #[error("the drawer is open already")]
    AlreadyOpened,
    /// The session hasn't been opened.
    #[error("the drawer hasn't been opened")]
    NotOpened,
    /// The session was closed.
    #[error("the drawer is closed")]
    Closed,
    /// The session belongs to another location than the device's.
    #[error("the drawer belongs to another location")]
    WrongLocation,
    /// Another device opened the session, and it alone writes it.
    #[error("another device's drawer")]
    NotOpener,
    /// An amount in another currency than the drawer's.
    #[error("an amount is in another currency than the drawer's")]
    WrongCurrency,
    /// The payment's cash was counted already.
    #[error("that payment's cash is in the drawer already")]
    AlreadyTaken,
    /// The amounts overflow.
    #[error("the amounts overflow")]
    Overflow,
    /// The event wouldn't satisfy its schema, such as an amount of zero.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl From<Unrecordable> for DrawerError {
    fn from(error: Unrecordable) -> DrawerError {
        match error {
            Unrecordable::Schema(error) => DrawerError::Schema(error),
            Unrecordable::Payload(error) => DrawerError::Invalid(error),
        }
    }
}

impl Drawer {
    /// Checks `command`, from `device` at `location`, against the session as this device sees it,
    /// and returns the event to record.
    ///
    /// # Errors
    /// [`DrawerError`] saying why the command isn't allowed.
    pub fn decide(
        &self,
        location: Id<Location>,
        device: Id<Device>,
        command: DrawerCommand,
    ) -> Result<DrawerEvent, DrawerError> {
        let event = match command {
            DrawerCommand::Open { float, currency } => {
                if self.info().is_some() {
                    return Err(DrawerError::AlreadyOpened);
                }
                if float.currency() != currency {
                    return Err(DrawerError::WrongCurrency);
                }
                DrawerEvent::Opened { float }
            }
            command => self.decide_open(location, device, command)?,
        };
        event.validate().map_err(DrawerError::Invalid)?;
        check_recordable(&event)?;
        Ok(event)
    }

    /// A command on an open session, from its opener at its location.
    fn decide_open(
        &self,
        location: Id<Location>,
        device: Id<Device>,
        command: DrawerCommand,
    ) -> Result<DrawerEvent, DrawerError> {
        let info = self.info().ok_or(DrawerError::NotOpened)?;
        if info.location != location {
            return Err(DrawerError::WrongLocation);
        }
        if info.device != device {
            return Err(DrawerError::NotOpener);
        }
        if self.closed().is_some() {
            return Err(DrawerError::Closed);
        }
        let currency = info.float.currency();
        let in_currency = |amount: Money| {
            if amount.currency() == currency { Ok(()) } else { Err(DrawerError::WrongCurrency) }
        };
        Ok(match command {
            DrawerCommand::Open { .. } => return Err(DrawerError::AlreadyOpened),
            DrawerCommand::TakeCash(taken) => {
                in_currency(taken.amount)?;
                if self.payments().contains(&taken.payment) {
                    return Err(DrawerError::AlreadyTaken);
                }
                self.fits(taken.amount, |sums| &mut sums.taken)?;
                DrawerEvent::CashTaken(taken)
            }
            DrawerCommand::MoveCash(moved) => {
                in_currency(moved.amount)?;
                let to: fn(&mut Sums) -> &mut Money = match moved.kind {
                    Movement::PaidIn => |sums| &mut sums.paid_in,
                    Movement::PaidOut => |sums| &mut sums.paid_out,
                    Movement::Drop => |sums| &mut sums.dropped,
                };
                self.fits(moved.amount, to)?;
                DrawerEvent::CashMoved(moved)
            }
            DrawerCommand::NoSale(reason) => DrawerEvent::NoSale { reason },
            DrawerCommand::Close { counted } => {
                in_currency(counted)?;
                let expected = self
                    .expected()
                    .map_err(|_| DrawerError::Overflow)?
                    .ok_or(DrawerError::NotOpened)?;
                let closed = DrawerClosed { counted, expected };
                closed.variance().ok_or(DrawerError::Overflow)?;
                DrawerEvent::Closed(closed)
            }
        })
    }

    /// Refuses adding `amount` to the sum `to` picks if that sum, or what the session should
    /// hold, would overflow: the fold wouldn't apply it, or the session couldn't close. Found in
    /// review.
    fn fits(&self, amount: Money, to: fn(&mut Sums) -> &mut Money) -> Result<(), DrawerError> {
        let (Some(info), Some(sums)) = (self.info(), self.sums()) else {
            return Err(DrawerError::NotOpened);
        };
        let mut after = *sums;
        let sum = to(&mut after);
        *sum = sum.checked_add(amount).map_err(|_| DrawerError::Overflow)?;
        held(info.float, &after).map_err(|_| DrawerError::Overflow)?;
        Ok(())
    }
}
