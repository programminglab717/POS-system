//! What a device records of a kitchen ticket, checked against it as the device sees it
//! (ADR-0034). Each refuses what the fold would report, an event at another location or before
//! the ticket was sent, a second sending, and a reroute of a ticket a screen showed; and each
//! refuses what changes nothing: a showing, bump, recall or reroute already so.

use keel_events::envelope::Location;
use keel_types::Id;

use super::events::{KitchenEvent, RerouteTo, TicketSent};
use super::state::KitchenTicket;
use crate::codec::PayloadError;
use crate::schema::{SchemaError, Unrecordable, check_recordable};

/// What a device asks to record of a kitchen ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KitchenCommand {
    /// Send it to its station.
    Send(TicketSent),
    /// Say a screen showed it.
    Show,
    /// Bump it: its station is done with it.
    Bump,
    /// Bring it back on its station's screen.
    Recall,
    /// Say no screen showed it in time, and where it was sent instead.
    Reroute(RerouteTo),
}

/// Why a kitchen command was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KitchenError {
    /// The ticket belongs to another location.
    #[error("the ticket belongs to another location")]
    WrongLocation,
    /// The ticket was sent already.
    #[error("the ticket was sent already")]
    AlreadySent,
    /// The ticket hasn't been sent.
    #[error("the ticket hasn't been sent")]
    NotSent,
    /// A screen showed the ticket already.
    #[error("the ticket was shown already")]
    AlreadyShown,
    /// The ticket is bumped already.
    #[error("the ticket is bumped already")]
    AlreadyBumped,
    /// The ticket isn't bumped: there is nothing to recall.
    #[error("the ticket isn't bumped")]
    NotBumped,
    /// The ticket was rerouted already.
    #[error("the ticket was rerouted already")]
    AlreadyRerouted,
    /// The event wouldn't satisfy its schema, such as a ticket with no items.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl From<Unrecordable> for KitchenError {
    fn from(unrecordable: Unrecordable) -> KitchenError {
        match unrecordable {
            Unrecordable::Payload(invalid) => KitchenError::Invalid(invalid),
            Unrecordable::Schema(schema) => KitchenError::Schema(schema),
        }
    }
}

impl KitchenTicket {
    /// Checks `command`, from a device at `location`, against the ticket as the device sees it,
    /// and returns the event to record.
    ///
    /// # Errors
    /// [`KitchenError`] saying why the command isn't allowed.
    pub fn decide(
        &self,
        location: Id<Location>,
        command: KitchenCommand,
    ) -> Result<KitchenEvent, KitchenError> {
        if self.location().is_some_and(|known| known != location) {
            return Err(KitchenError::WrongLocation);
        }
        let event = match command {
            KitchenCommand::Send(sent) => {
                if self.sent().is_some() {
                    return Err(KitchenError::AlreadySent);
                }
                KitchenEvent::Sent(sent)
            }
            other => {
                if self.sent().is_none() {
                    return Err(KitchenError::NotSent);
                }
                match other {
                    KitchenCommand::Send(_) => return Err(KitchenError::AlreadySent),
                    // A ticket a screen showed is neither shown again nor rerouted.
                    KitchenCommand::Show | KitchenCommand::Reroute(_) if self.shown().is_some() => {
                        return Err(KitchenError::AlreadyShown);
                    }
                    // A showing only keeps a ticket from being rerouted: once it is, the screen
                    // shows it, and records nothing the fold would report; nor is it rerouted
                    // again.
                    KitchenCommand::Show | KitchenCommand::Reroute(_)
                        if self.rerouted().is_some() =>
                    {
                        return Err(KitchenError::AlreadyRerouted);
                    }
                    KitchenCommand::Show => KitchenEvent::Shown,
                    KitchenCommand::Bump if self.is_bumped() => {
                        return Err(KitchenError::AlreadyBumped);
                    }
                    KitchenCommand::Bump => KitchenEvent::Bumped,
                    KitchenCommand::Recall if !self.is_bumped() => {
                        return Err(KitchenError::NotBumped);
                    }
                    KitchenCommand::Recall => KitchenEvent::Recalled,
                    KitchenCommand::Reroute(to) => KitchenEvent::Rerouted(to),
                }
            }
        };
        event.validate().map_err(KitchenError::Invalid)?;
        check_recordable(&event)?;
        Ok(event)
    }
}
