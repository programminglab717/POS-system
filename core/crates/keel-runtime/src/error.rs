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
    /// No request waits for an approval: nothing the member asked for was refused for want of one.
    #[error("nothing waits for an approval")]
    NothingToApprove,
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
    /// The device's drawer is open already: one at a time (ADR-0031).
    #[error("a drawer is open: close it first")]
    DrawerOpen,
    /// The device has no drawer open.
    #[error("no drawer is open")]
    NoDrawer,
    /// The drawer refused the change.
    #[error(transparent)]
    Drawer(#[from] keel_domain::drawer::DrawerError),
    /// The order was paid in part by card: it is refunded to the card, which card refunds will
    /// do (ADR-0032).
    #[error(
        "the order wasn't paid in cash alone, as far as this register knows: a card's is refunded to the card, which isn't built yet"
    )]
    CardRefund,
    /// The catalog has no such item, variant or modifier (ADR-0033).
    #[error("the catalog has no item {0}")]
    UnknownItem(String),
    /// An item, or a modifier chosen, is sold out (ADR-0033).
    #[error("{0} is sold out")]
    SoldOut(String),
    /// A kitchen ticket refused the change (ADR-0034).
    #[error(transparent)]
    Kitchen(#[from] keel_domain::kitchen::KitchenError),
    /// An item's availability refused the change (ADR-0033).
    #[error(transparent)]
    Availability(#[from] keel_domain::availability::AvailabilityError),
    /// The order changed while the intent was being made: look at it again (ADR-0032).
    #[error("the order changed meanwhile: look at it again")]
    OrderChanged,
    /// The refund refused the change.
    #[error(transparent)]
    Refund(#[from] keel_domain::refund::RefundError),
    /// A discount takes more than is left of its line: it would take more as the line grew. Found
    /// in review (ADR-0030).
    #[error("only {left} is left to take off")]
    DiscountTooLarge {
        /// What is left of the line.
        left: Money,
    },
    /// A change to what a check charges while a payment toward it has been taken: the customer
    /// paid for what it was. Found in review (ADR-0030).
    #[error("check {0} is partly paid")]
    PartlyPaid(Id<keel_domain::order::Check>),
    /// An order split among checks is paid check by check (ADR-0030).
    #[error("the order is split: pay it check by check")]
    Split,
    /// An order is split among fewer than two checks, or more than the most, or has nothing to
    /// split (ADR-0030).
    #[error("nothing to split that way")]
    NothingToSplit,
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
