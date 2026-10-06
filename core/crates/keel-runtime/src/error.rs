//! Why an intent was refused, or a view couldn't be made.

use keel_domain::checkout::CheckoutError;
use keel_domain::order::{CommandError, Order};
use keel_domain::payment::PaymentError;
use keel_domain::profile::RingError;
use keel_domain::schema::SchemaError;
use keel_events::envelope::{EnvelopeError, TeamMember};
use keel_pricing::PricingError;
use keel_store::StoreError;
use keel_types::{BusinessDateError, Id, IdError, Money, MoneyError, QuantityError};

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
    /// An event's envelope couldn't be made.
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
}
