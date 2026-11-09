//! Print jobs (ADR-0024, decisions 5 and 7): receipts and drawer kicks as effects in the store's
//! outbox, handed to a driver one at a time and never sent again by themselves once any of them
//! may have been sent.

use core::time::Duration;

use keel_doc::{Confirm, Doc};
use keel_store::{Effect, EffectKind, EffectState, Queued};
use keel_types::{Id, Timestamp};

use crate::error::RuntimeError;

/// A print job, as its identifier names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Job {}

/// The kind of a receipt's job.
pub const RECEIPT: &str = "print.receipt";
/// The kind of a drawer kick's job.
pub const DRAWER: &str = "print.drawer";

/// What a job prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// A receipt.
    Receipt,
    /// The cash drawer's pulse.
    Drawer,
}

impl JobKind {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            JobKind::Receipt => RECEIPT,
            JobKind::Drawer => DRAWER,
        }
    }

    pub(crate) fn of(kind: &EffectKind) -> Option<JobKind> {
        match kind.as_str() {
            RECEIPT => Some(JobKind::Receipt),
            DRAWER => Some(JobKind::Drawer),
            _ => None,
        }
    }
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobState {
    /// Waiting for the printer, or to be tried again.
    Queued,
    /// With the driver.
    Sending,
    /// Some of it may have been sent, and no one knows whether it printed: staff decide.
    Unconfirmed,
}

/// A job, as the register shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobView {
    /// The job.
    pub job: Id<Job>,
    /// What it prints.
    pub kind: JobKind,
    /// Where it is.
    pub state: JobState,
    /// How many times it was handed to the driver.
    pub attempts: u32,
}

/// A job for the driver: the bytes to send, and how the printer says they printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintJob {
    /// The job.
    pub job: Id<Job>,
    /// What it prints.
    pub kind: JobKind,
    /// The bytes, rendered for the printer.
    pub bytes: Vec<u8>,
    /// How the printer says it is done.
    pub confirm: Confirm,
    /// The process ID the driver asks the printer to answer with.
    pub process_id: [u8; 4],
}

/// What staff decide about an unconfirmed job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// It printed.
    Printed,
    /// Print it again, marked as a reprint.
    Again,
    /// Leave it: it ends without a print anyone knows of.
    Dismiss,
}

/// The effect of a new job.
pub(crate) fn effect(
    job: Id<Job>,
    kind: JobKind,
    doc: &Doc,
    cause: Option<Id<keel_events::envelope::Event>>,
) -> Result<Effect, RuntimeError> {
    Ok(Effect {
        key: job.to_bytes().to_vec(),
        kind: EffectKind::new(kind.code())?,
        payload: doc.encode(),
        cause,
    })
}

/// The job a queued effect is, if it is one.
pub(crate) fn job_of(queued: &Queued) -> Option<(Id<Job>, JobKind)> {
    let kind = JobKind::of(&queued.effect.kind)?;
    let key: [u8; 16] = queued.effect.key.as_slice().try_into().ok()?;
    Some((Id::from_bytes(key).ok()?, kind))
}

/// The view of a job, where `sending` is the job with the driver.
pub(crate) fn view(queued: &Queued, sending: Option<Id<Job>>) -> Option<JobView> {
    let (job, kind) = job_of(queued)?;
    let state = match queued.state {
        EffectState::Pending => JobState::Queued,
        EffectState::Running if sending == Some(job) => JobState::Sending,
        EffectState::Running => JobState::Unconfirmed,
        _ => return None,
    };
    Some(JobView { job, kind, state, attempts: queued.attempts })
}

/// How long a job that wasn't sent waits before it is tried again: 2 s after its first
/// attempt, doubling to at most 60 s.
pub(crate) fn backoff(attempts: u32) -> Duration {
    let doublings = attempts.saturating_sub(1).min(5);
    let wait = 2_u64.checked_shl(doublings).unwrap_or(60);
    Duration::from_secs(wait.min(60))
}

/// `now`, `wait` later.
pub(crate) fn after(now: Timestamp, wait: Duration) -> Result<Timestamp, RuntimeError> {
    let millis = i64::try_from(wait.as_millis()).map_err(|_| RuntimeError::Time)?;
    let later = now.as_millis().checked_add(millis).ok_or(RuntimeError::Time)?;
    Timestamp::from_millis(later).map_err(|_| RuntimeError::Time)
}
