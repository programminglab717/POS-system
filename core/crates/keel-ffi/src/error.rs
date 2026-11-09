//! Errors (ADR-0025, decision 4): one enumeration, `KeelException` in Kotlin, with the refusals
//! a cashier can meet as variants of their own. No field is named `message`, which Kotlin's
//! exceptions already have.

use keel_runtime::RuntimeError;

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

impl From<RuntimeError> for KeelError {
    fn from(error: RuntimeError) -> KeelError {
        let reason = error.to_string();
        match error {
            RuntimeError::NotSignedIn => KeelError::NotSignedIn,
            RuntimeError::UnknownMember(member) => {
                KeelError::UnknownMember { member: member.to_string() }
            }
            RuntimeError::UnknownOrder(order) => {
                KeelError::UnknownOrder { order: order.to_string() }
            }
            RuntimeError::Ring(_) => KeelError::Catalog { reason },
            RuntimeError::Order(_) | RuntimeError::Checkout(_) | RuntimeError::Payment(_) => {
                KeelError::Refused { reason }
            }
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
            _ => KeelError::Device { reason },
        }
    }
}
