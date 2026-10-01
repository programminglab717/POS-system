//! Who owns an order (ADR-0021): the device that may make its structural and money changes.
//!
//! An order's ownership is decided by its own events. The device that records `order.created`
//! owns the order under lease 0. A device that wants the order records a request, and the hub
//! answers it with a grant or a refusal; a manager's override takes the order without the hub.
//! Each change of owner names the lease it replaces and takes the next one, and applies only if
//! that lease is still the order's at that point in canonical order, so two changes made from
//! the same lease never both apply.
//!
//! The hub's answers are a function of what it holds ([`Order::answers`]): a request is refused
//! if its lease has moved on, if its device already owns the order, or while a payment of the
//! order is in progress; otherwise it is granted under the next lease.

use keel_events::cbor::Value;
use keel_events::envelope::{Device, Event};
use keel_types::Id;

use super::events::OrderEvent;
use super::state::Order;
use crate::codec::{Field, code_enum};
use crate::hub::Epoch;

#[cfg(test)]
mod tests;

/// How many times an order has changed owner: 0 under the device that created it. Every change
/// of owner names the lease it replaces, which makes each change a compare-and-swap. At most
/// 2^63 − 1, so that it fits the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lease(u64);

impl Lease {
    /// The lease of the device that created the order.
    pub const FIRST: Lease = Lease(0);

    /// The largest lease.
    pub const MAX: Lease = Lease((1 << 63) - 1);

    /// The lease numbered `n`, if it is at most [`Lease::MAX`].
    pub const fn new(n: u64) -> Option<Lease> {
        if n <= Lease::MAX.0 { Some(Lease(n)) } else { None }
    }

    /// Its number.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The lease after it: `None` after the largest.
    pub const fn next(self) -> Option<Lease> {
        match self.0.checked_add(1) {
            Some(n) => Lease::new(n),
            None => None,
        }
    }
}

impl Field for Lease {
    fn to_value(&self) -> Value {
        Value::Unsigned(self.0)
    }

    fn from_value(value: &Value) -> Option<Lease> {
        Lease::new(value.as_u64()?)
    }
}

/// Who owns an order: a device, under a lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ownership {
    /// The owning device.
    pub device: Id<Device>,
    /// The lease it holds the order under.
    pub lease: Lease,
}

/// A device's request for an order, waiting for the hub's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// The event that made the request.
    pub event: Id<Event>,
    /// The device that wants the order: the one that recorded the request.
    pub device: Id<Device>,
    /// The lease it saw, which the request would replace.
    pub lease: Lease,
}

code_enum! {
    /// Why the hub didn't give an order to the device that asked for it.
    pub enum Refusal {
        /// The order had changed owner since the device saw it: its lease moved on.
        LeaseMoved = 0,
        /// The device already owned the order.
        AlreadyOwner = 1,
        /// A payment of the order was in progress: started, with no outcome yet.
        PaymentInProgress = 2,
    }
}

/// The hub gave an order to the device that asked for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnershipGranted {
    /// The request it answers.
    pub request: Id<Event>,
    /// The device that now owns the order.
    pub device: Id<Device>,
    /// The new lease: the one after the request's.
    pub lease: Lease,
    /// The hub's epoch, by which a later hub can tell a deposed one's grants.
    pub epoch: Epoch,
}

impl Order {
    /// The hub's answers, in `epoch`, to the order's pending requests, in canonical order: each
    /// request is answered as if the answers before it had applied, so a grant moves the lease
    /// for the requests after it. `paying` says whether a payment of the order is in progress.
    /// No answers while the order's creation is missing: its requests wait for it.
    pub fn answers(&self, paying: bool, epoch: Epoch) -> Vec<OrderEvent> {
        let Some(mut ownership) = self.ownership() else { return Vec::new() };
        let mut answers = Vec::new();
        for request in self.requests() {
            let refusal = if request.lease != ownership.lease {
                Some(Refusal::LeaseMoved)
            } else if request.device == ownership.device {
                Some(Refusal::AlreadyOwner)
            } else if paying {
                Some(Refusal::PaymentInProgress)
            } else {
                None
            };
            // No lease follows the largest: no grant can replace it.
            let granted = request.lease.next().filter(|_| refusal.is_none());
            answers.push(match granted {
                Some(lease) => {
                    ownership = Ownership { device: request.device, lease };
                    OrderEvent::OwnershipGranted(OwnershipGranted {
                        request: request.event,
                        device: request.device,
                        lease,
                        epoch,
                    })
                }
                None => OrderEvent::OwnershipRefused {
                    request: request.event,
                    refusal: refusal.unwrap_or(Refusal::LeaseMoved),
                },
            });
        }
        answers
    }
}
