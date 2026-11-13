//! Errors (ADR-0025, decision 4): one enumeration, `KeelException` in Kotlin, with the refusals
//! a cashier can meet as variants of their own. No field is named `message`, which Kotlin's
//! exceptions already have.

use keel_runtime::RuntimeError;
use keel_store::StoreError;

use crate::records::Amount;

/// Why a call was refused, or failed.
#[derive(Debug, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum KeelError {
    /// No one is signed in.
    #[error("no one is signed in")]
    NotSignedIn,
    /// The profile's team doesn't have this member.
    #[error("{member} isn't on the team")]
    UnknownMember {
        /// The member asked for.
        member: String,
    },
    /// The PIN isn't the member's (ADR-0029).
    #[error("that PIN isn't theirs")]
    WrongPin,
    /// Too many wrong PINs on this register: PIN entry is locked for a while.
    #[error("too many wrong PINs: try again in {seconds} s")]
    LockedOut {
        /// How many seconds more.
        seconds: u64,
    },
    /// The action needs a manager's approval: ask one for their PIN, `approve`, and try again.
    #[error("{action} needs a manager's approval")]
    ApprovalRequired {
        /// The action, as ADR-0029 names it, such as `order.void`.
        action: String,
    },
    /// Nothing waits for an approval: approve only after an intent answers
    /// [`KeelError::ApprovalRequired`].
    #[error("nothing waits for an approval")]
    NothingToApprove,
    /// The approval given can't approve this: the approver's role doesn't permit it, or it is
    /// their own request.
    #[error("{approver} can't approve {action}")]
    NotApprover {
        /// Who approved.
        approver: String,
        /// What they can't approve.
        action: String,
    },
    /// The store holds no such order.
    #[error("no order {order}")]
    UnknownOrder {
        /// The order asked for.
        order: String,
    },
    /// The catalog refused the item or its choices: one not offered, too many or too few.
    #[error("{reason}")]
    Catalog {
        /// Why, for staff.
        reason: String,
    },
    /// The order, its checkout or its payment refused the change.
    #[error("{reason}")]
    Refused {
        /// Why, for staff.
        reason: String,
    },
    /// The cash tendered is short of what is due.
    #[error("{} is due", due.text)]
    TenderShort {
        /// What is due in cash, rounded as the location rounds cash.
        due: Amount,
    },
    /// The tender is in another currency than the location's.
    #[error("the tender is in another currency than the location's")]
    WrongCurrency,
    /// The tender is less than nothing.
    #[error("the tender is negative")]
    NegativeTender,
    /// No print job has this identifier, or it isn't the one with the driver.
    #[error("no print job {job} is waiting for that")]
    UnknownJob {
        /// The job.
        job: String,
    },
    /// The print job isn't unconfirmed.
    #[error("print job {job} isn't unconfirmed")]
    NotUnconfirmed {
        /// The job.
        job: String,
    },
    /// The order has no receipt to print again.
    #[error("order {order} has no receipt to print again")]
    NoReceipt {
        /// The order.
        order: String,
    },
    /// The store is open already: a register opens its store once, and must be closed before it
    /// opens again.
    #[error("the store is open already")]
    StoreInUse,
    /// The store's key doesn't open it: the key the platform's keystore gave isn't this store's.
    #[error("the key doesn't open the store")]
    WrongKey,
    /// The store was written by a newer kernel: the app was downgraded.
    #[error("the store is newer than this kernel")]
    StoreNewer,
    /// The store belongs to another device, or another location.
    #[error("the store is another device's")]
    NotThisDevice,
    /// The store is damaged: it needs restoring from its peers.
    #[error("the store is damaged")]
    Damaged,
    /// The register waits for its location's hub before it writes, so as never to fork its log:
    /// a few seconds after it joins its network, or once a hub answers.
    #[error("the register is waiting for its hub")]
    NotSettled,
    /// An argument the shell passed isn't valid: an identifier that doesn't parse, a key of the
    /// wrong length, a profile that doesn't decode.
    #[error("{reason}")]
    InvalidArgument {
        /// What, for the developer.
        reason: String,
    },
    /// The device failed: its store, or an amount out of range. Someone needs to look.
    #[error("{reason}")]
    Device {
        /// What, for support.
        reason: String,
    },
}

impl KeelError {
    /// An invalid argument, `what`.
    pub(crate) fn invalid(what: impl core::fmt::Display) -> KeelError {
        KeelError::InvalidArgument { reason: what.to_string() }
    }
}

impl From<StoreError> for KeelError {
    fn from(error: StoreError) -> KeelError {
        match error {
            StoreError::Busy => KeelError::StoreInUse,
            StoreError::KeyRejected => KeelError::WrongKey,
            StoreError::NewerSchema { .. } => KeelError::StoreNewer,
            StoreError::NotThisDevice | StoreError::WrongSigner => KeelError::NotThisDevice,
            StoreError::Damaged => KeelError::Damaged,
            other => KeelError::Device { reason: other.to_string() },
        }
    }
}

impl From<RuntimeError> for KeelError {
    fn from(error: RuntimeError) -> KeelError {
        let reason = error.to_string();
        match error {
            RuntimeError::NotSignedIn => KeelError::NotSignedIn,
            RuntimeError::NotSettled => KeelError::NotSettled,
            RuntimeError::RosterMismatch | RuntimeError::NotSyncing => {
                KeelError::InvalidArgument { reason }
            }
            RuntimeError::UnknownMember(member) => {
                KeelError::UnknownMember { member: member.to_string() }
            }
            RuntimeError::UnknownOrder(order) => {
                KeelError::UnknownOrder { order: order.to_string() }
            }
            RuntimeError::WrongPin => KeelError::WrongPin,
            RuntimeError::LockedOut { seconds } => KeelError::LockedOut { seconds },
            RuntimeError::ApprovalRequired(action) => {
                KeelError::ApprovalRequired { action: action.name().to_owned() }
            }
            RuntimeError::NothingToApprove => KeelError::NothingToApprove,
            RuntimeError::NotApprover { approver, action } => KeelError::NotApprover {
                approver: approver.to_string(),
                action: action.name().to_owned(),
            },
            RuntimeError::Ring(_) => KeelError::Catalog { reason },
            RuntimeError::Order(_)
            | RuntimeError::Checkout(_)
            | RuntimeError::Payment(_)
            | RuntimeError::NothingToSplit
            | RuntimeError::DiscountTooLarge { .. }
            | RuntimeError::PartlyPaid(_)
            | RuntimeError::Split
            | RuntimeError::DrawerOpen
            | RuntimeError::NoDrawer
            | RuntimeError::Drawer(_)
            | RuntimeError::CardRefund
            | RuntimeError::Refund(_) => KeelError::Refused { reason },
            RuntimeError::TenderShort { due } => KeelError::TenderShort {
                due: Amount {
                    minor: due.minor(),
                    currency: due.currency().code().to_owned(),
                    text: String::new(),
                },
            },
            RuntimeError::WrongCurrency => KeelError::WrongCurrency,
            RuntimeError::NegativeTender => KeelError::NegativeTender,
            RuntimeError::UnknownJob(job) => KeelError::UnknownJob { job: job.to_string() },
            RuntimeError::NotUnconfirmed(job) => KeelError::NotUnconfirmed { job: job.to_string() },
            RuntimeError::NoReceipt(order) => KeelError::NoReceipt { order: order.to_string() },
            RuntimeError::Store(store) => store.into(),
            _ => KeelError::Device { reason },
        }
    }
}
