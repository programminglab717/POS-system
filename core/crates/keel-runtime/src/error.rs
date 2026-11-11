//! Why an intent was refused, or a view couldn't be made.

use keel_domain::checkout::CheckoutError;
use keel_domain::order::{CommandError, Order};
use keel_domain::payment::PaymentError;
use keel_domain::profile::RingError;
use keel_domain::schema::SchemaError;
use keel_domain::staff::Action;
use keel_events::envelope::{EnvelopeError, TeamMember};
use keel_pricing::PricingError;
use keel_store::StoreError;
use keel_types::{BusinessDateError, Id, IdError, Money, MoneyError, QuantityError};

use crate::print::Job;

/// Why an intent was refused, or a view couldn't be made.
///
/// The refusals a cashier can meet come first, each saying what to put right; the failures
/// after them are the device's, and need someone to look at the store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// No team member is signed in.
    #[error("no one is signed in")]
    NotSignedIn,
    /// The profile has no such team member.
    #[error("{0} isn't on the team")]
    UnknownMember(Id<TeamMember>),
    /// The PIN isn't the member's (ADR-0029).
    #[error("that PIN isn't theirs")]
    WrongPin,
    /// Too many wrong PINs: PIN entry is locked for a while.
    #[error("too many wrong PINs: try again in {seconds} s")]
    LockedOut {
        /// How many seconds more.
        seconds: u64,
    },
    /// The action needs a manager's approval, which no one has given.
    #[error("{} needs a manager's approval", .0.name())]
    ApprovalRequired(Action),
    /// The approval waiting can't approve this: the approver's role doesn't permit it, or it is
    /// their own request.
    #[error("{approver} can't approve {}", action.name())]
    NotApprover {
        /// Who approved.
        approver: Id<TeamMember>,
        /// What they can't approve.
        action: Action,
    },
    /// The store holds no such order.
    #[error("no order {0}")]
    UnknownOrder(Id<Order>),
    /// The catalog refused the item or its choices.
    #[error(transparent)]
    Ring(#[from] RingError),
    /// The order refused the change: it is closed, the line isn't there, and the like.
    #[error(transparent)]
    Order(#[from] CommandError),
    /// Checkout refused: the order can't be paid as it stands.
    #[error(transparent)]
    Checkout(#[from] CheckoutError),
    /// The payment refused the change.
    #[error(transparent)]
    Payment(#[from] PaymentError),
    /// The cash tendered is short of what is due.
    #[error("{due} is due")]
    TenderShort {
        /// What is due in cash, rounded as the location rounds cash.
        due: Money,
    },
    /// The tender is in another currency than the location's.
    #[error("the tender is in another currency than the location's")]
    WrongCurrency,
    /// The tender is less than nothing.
    #[error("the tender is negative")]
    NegativeTender,
    /// No print job has this identifier, or it isn't the one with the driver.
    #[error("no print job {0} is waiting for that")]
    UnknownJob(Id<Job>),
    /// The print job isn't unconfirmed: it is queued, with the driver, or over.
    #[error("print job {0} isn't unconfirmed")]
    NotUnconfirmed(Id<Job>),
    /// The order isn't closed, or has no receipt to print again.
    #[error("order {0} has no receipt to print again")]
    NoReceipt(Id<Order>),
    /// The device's log isn't settled yet: it waits to hear from its peers before it writes,
    /// so it never forks its log.
    #[error("the device is catching up with its peers")]
    NotSettled,
    /// The roster is another location's, or doesn't have this device, or names another key for
    /// it.
    #[error("the roster isn't this device's location's")]
    RosterMismatch,
    /// The runtime isn't replicating: start it first.
    #[error("the runtime isn't replicating")]
    NotSyncing,
    /// Only the location's cloud publishes its profile (ADR-0028).
    #[error("this device isn't its location's cloud")]
    NotCloud,
    /// The profile can't be published: it is too large.
    #[error("the profile can't be published: {0}")]
    Publication(String),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// An amount couldn't be worked out without overflowing.
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// A quantity was out of range.
    #[error(transparent)]
    Quantity(#[from] QuantityError),
    /// The order couldn't be priced.
    #[error(transparent)]
    Pricing(#[from] PricingError),
    /// An identifier couldn't be made.
    #[error(transparent)]
    Id(#[from] IdError),
    /// The clock's time has no business date.
    #[error(transparent)]
    BusinessDate(#[from] BusinessDateError),
    /// An event couldn't be encoded.
    #[error(transparent)]
    Schema(#[from] SchemaError),
    /// A payload the runtime made breaks its schema's rules.
    #[error(transparent)]
    Payload(#[from] keel_domain::codec::PayloadError),
    /// A document couldn't be made or read.
    #[error(transparent)]
    Doc(#[from] keel_doc::DocError),
    /// The outbox refused an effect.
    #[error(transparent)]
    Effect(#[from] keel_store::EffectError),
    /// A time out of range.
    #[error("a time out of range")]
    Time,
    /// An event's envelope couldn't be made.
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
}
