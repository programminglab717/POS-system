//! Order commands: what a device asks to do, checked against its current view of the order.
//!
//! A command that passes becomes one event. Checking happens before the event exists, on the
//! device, against its latest view, so an event from a valid command never conflicts with the
//! state it was checked against. Conflicts only arise when devices act concurrently on
//! different views, and the fold resolves those (see [`super::state`]).
//!
//! Permissions, approvals and the owning device's lease are checked by other parts of the kernel.

use keel_events::envelope::Location;
use keel_types::Id;

use super::checks::{Check, LinesAllocated};
use super::events::{AttributesChanged, LineAdded, LineChanged, OrderCreated, OrderEvent};
use super::state::{Line, LineStatus, Order, OrderInfo, OrderStatus};
use super::types::Reason;
use crate::codec::{Change, IdSet, PayloadError};
use crate::schema::{DecodeError, DomainEvent, SchemaError};

/// What a device asks to do to an order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderCommand {
    /// Open the order.
    Create(OrderCreated),
    /// Change some of its attributes. Every field given must change something.
    ChangeAttributes(AttributesChanged),
    /// Add a line.
    AddLine(LineAdded),
    /// Change some of a line's details, before it is fired. Every field given must change
    /// something.
    ChangeLine(LineChanged),
    /// Take off a line that hasn't been fired.
    RemoveLine(Id<Line>),
    /// Send lines to be prepared.
    FireLines(IdSet<Line>),
    /// Void a fired line.
    VoidLine {
        /// The line.
        line: Id<Line>,
        /// Why.
        reason: Reason,
    },
    /// Give a line away.
    CompLine {
        /// The line.
        line: Id<Line>,
        /// Why.
        reason: Reason,
    },
    /// Void the whole order.
    Void(Reason),
    /// Drop an order before anything in it was fired: it has no lines, or every line was removed
    /// before it was fired.
    Abandon,
    /// Open a check.
    OpenCheck(Id<Check>),
    /// Allocate live lines to existing checks. Every line listed must get a new allocation.
    AllocateLines(LinesAllocated),
}

/// Why a command was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CommandError {
    /// The order hasn't been created.
    #[error("the order hasn't been created")]
    NotCreated,
    /// The order already exists.
    #[error("the order already exists")]
    AlreadyCreated,
    /// The order belongs to another location than the device's.
    #[error("the order belongs to another location")]
    WrongLocation,
    /// The order was voided or abandoned.
    #[error("the order is closed")]
    OrderClosed,
    /// A field given in a change already has that value, or nothing was given.
    #[error("the change doesn't change anything")]
    NoChange,
    /// A line with that identifier already exists.
    #[error("line {0} already exists")]
    LineExists(Id<Line>),
    /// No line has that identifier.
    #[error("no line {0}")]
    UnknownLine(Id<Line>),
    /// The line has already been fired, removed or voided.
    #[error("line {0} isn't waiting to be fired")]
    LineNotPending(Id<Line>),
    /// The line hasn't been fired, or has been removed or voided.
    #[error("line {0} isn't fired")]
    LineNotFired(Id<Line>),
    /// The line has been removed or voided.
    #[error("line {0} has been removed or voided")]
    LineNotLive(Id<Line>),
    /// The line is already comped.
    #[error("line {0} is already comped")]
    AlreadyComped(Id<Line>),
    /// A price in another currency than the order's.
    #[error("a price is in another currency than the order's")]
    WrongCurrency,
    /// A quantity in another unit than the line's.
    #[error("the quantity is in another unit than line {0}'s")]
    WrongUnit(Id<Line>),
    /// The order still has live lines.
    #[error("the order has live lines")]
    HasLiveLines,
    /// A line was sent to be prepared, so the order can't be abandoned: void it instead.
    #[error("line {0} was fired: void the order instead")]
    LineWasFired(Id<Line>),
    /// A check with that identifier already exists.
    #[error("check {0} already exists")]
    CheckExists(Id<Check>),
    /// No check has that identifier.
    #[error("no check {0}")]
    UnknownCheck(Id<Check>),
    /// The event wouldn't satisfy its schema, such as a negative price or quantity.
    #[error("invalid event: {0}")]
    Invalid(PayloadError),
    /// The event couldn't be encoded, such as modifiers nested too deeply.
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl Order {
    /// Checks `command`, from a device at `location`, against the order as this device sees
    /// it, and returns the event to record.
    ///
    /// # Errors
    /// [`CommandError`] saying why the command isn't allowed.
    pub fn decide(
        &self,
        location: Id<Location>,
        command: OrderCommand,
    ) -> Result<OrderEvent, CommandError> {
        let event = match command {
            OrderCommand::Create(created) => {
                if self.info().is_some() {
                    return Err(CommandError::AlreadyCreated);
                }
                OrderEvent::Created(created)
            }
            command => {
                let info = self.info().ok_or(CommandError::NotCreated)?;
                if info.location != location {
                    return Err(CommandError::WrongLocation);
                }
                if *self.status() != OrderStatus::Active {
                    return Err(CommandError::OrderClosed);
                }
                self.decide_on_active(info, command)?
            }
        };
        // Every event must satisfy its own schema, so no device writes one that others reject.
        let (schema, payload) = event.encode()?;
        OrderEvent::decode(&schema, &payload).map_err(|error| match error {
            DecodeError::Malformed(error) => CommandError::Invalid(error),
            _ => CommandError::Invalid(PayloadError::NotAMap),
        })?;
        Ok(event)
    }

    fn decide_on_active(
        &self,
        info: &OrderInfo,
        command: OrderCommand,
    ) -> Result<OrderEvent, CommandError> {
        match command {
            OrderCommand::Create(_) => Err(CommandError::AlreadyCreated),
            OrderCommand::ChangeAttributes(changed) => {
                check_attributes(info, &changed)?;
                Ok(OrderEvent::AttributesChanged(changed))
            }
            OrderCommand::AddLine(added) => {
                if self.line(added.line).is_some() {
                    return Err(CommandError::LineExists(added.line));
                }
                let priced_in_currency = added.item.unit_price.currency() == info.currency
                    && added.modifiers.iter().all(|modifier| modifier.is_priced_in(info.currency));
                if !priced_in_currency {
                    return Err(CommandError::WrongCurrency);
                }
                Ok(OrderEvent::LineAdded(added))
            }
            OrderCommand::ChangeLine(changed) => {
                let line = self.pending_line(changed.line)?;
                check_line_change(info, line, &changed)?;
                Ok(OrderEvent::LineChanged(changed))
            }
            OrderCommand::RemoveLine(line) => {
                self.pending_line(line)?;
                Ok(OrderEvent::LineRemoved { line })
            }
            OrderCommand::FireLines(lines) => {
                for line in lines.iter() {
                    self.pending_line(line)?;
                }
                Ok(OrderEvent::LinesFired { lines })
            }
            OrderCommand::VoidLine { line, reason } => {
                let found = self.line(line).ok_or(CommandError::UnknownLine(line))?;
                if *found.status() != LineStatus::Fired {
                    return Err(CommandError::LineNotFired(line));
                }
                Ok(OrderEvent::LineVoided { line, reason })
            }
            OrderCommand::CompLine { line, reason } => {
                let found = self.line(line).ok_or(CommandError::UnknownLine(line))?;
                if !found.is_live() {
                    return Err(CommandError::LineNotLive(line));
                }
                if found.comp().is_some() {
                    return Err(CommandError::AlreadyComped(line));
                }
                Ok(OrderEvent::LineComped { line, reason })
            }
            OrderCommand::Void(reason) => Ok(OrderEvent::Voided { reason }),
            OrderCommand::Abandon => {
                if self.live_lines().next().is_some() {
                    return Err(CommandError::HasLiveLines);
                }
                if let Some(fired) = self.lines().iter().find(|line| line.was_fired()) {
                    return Err(CommandError::LineWasFired(fired.id()));
                }
                Ok(OrderEvent::Abandoned)
            }
            OrderCommand::OpenCheck(check) => {
                if self.check(check).is_some() {
                    return Err(CommandError::CheckExists(check));
                }
                Ok(OrderEvent::CheckOpened { check })
            }
            OrderCommand::AllocateLines(allocated) => {
                for (id, shares) in allocated.lines() {
                    let line = self.line(id).ok_or(CommandError::UnknownLine(id))?;
                    if !line.is_live() {
                        return Err(CommandError::LineNotLive(id));
                    }
                    if let Some(share) =
                        shares.iter().find(|share| self.check(share.check).is_none())
                    {
                        return Err(CommandError::UnknownCheck(share.check));
                    }
                    if line.allocation() == shares.as_slice() {
                        return Err(CommandError::NoChange);
                    }
                }
                Ok(OrderEvent::LinesAllocated(allocated))
            }
        }
    }

    fn pending_line(&self, id: Id<Line>) -> Result<&Line, CommandError> {
        let line = self.line(id).ok_or(CommandError::UnknownLine(id))?;
        if *line.status() != LineStatus::Pending {
            return Err(CommandError::LineNotPending(id));
        }
        Ok(line)
    }
}

/// Whether setting an optional field changes it.
fn changes<T: PartialEq>(current: Option<&T>, change: Option<&Change<T>>) -> bool {
    match change {
        None => true,
        Some(Change::Set(value)) => current != Some(value),
        Some(Change::Clear) => current.is_some(),
    }
}

fn check_attributes(info: &OrderInfo, changed: &AttributesChanged) -> Result<(), CommandError> {
    let empty = changed.mode.is_none()
        && changed.revenue_center.is_none()
        && changed.table.is_none()
        && changed.guest_count.is_none()
        && changed.customer.is_none()
        && changed.owner.is_none();
    let all_change = changed.mode.is_none_or(|mode| mode != info.mode)
        && changes(info.revenue_center.as_ref(), changed.revenue_center.as_ref())
        && changes(info.table.as_ref(), changed.table.as_ref())
        && changes(info.guest_count.as_ref(), changed.guest_count.as_ref())
        && changes(info.customer.as_ref(), changed.customer.as_ref())
        && changes(info.owner.as_ref(), changed.owner.as_ref());
    if empty || !all_change {
        return Err(CommandError::NoChange);
    }
    Ok(())
}

fn check_line_change(
    info: &OrderInfo,
    line: &Line,
    changed: &LineChanged,
) -> Result<(), CommandError> {
    let empty = changed.quantity.is_none()
        && changed.modifiers.is_none()
        && changed.seat.is_none()
        && changed.course.is_none()
        && changed.notes.is_none();
    let all_change = changed.quantity.is_none_or(|quantity| quantity != line.quantity())
        && changed.modifiers.as_deref().is_none_or(|modifiers| modifiers != line.modifiers())
        && changes(line.seat().as_ref(), changed.seat.as_ref())
        && changes(line.course().as_ref(), changed.course.as_ref())
        && changes(line.notes(), changed.notes.as_ref());
    if empty || !all_change {
        return Err(CommandError::NoChange);
    }
    if changed.quantity.is_some_and(|quantity| quantity.unit() != line.quantity().unit()) {
        return Err(CommandError::WrongUnit(line.id()));
    }
    let modifiers_in_currency = changed.modifiers.as_deref().is_none_or(|modifiers| {
        modifiers.iter().all(|modifier| modifier.is_priced_in(info.currency))
    });
    if !modifiers_in_currency {
        return Err(CommandError::WrongCurrency);
    }
    Ok(())
}
