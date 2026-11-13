//! Print jobs (ADR-0024, decisions 5 and 7): receipts and drawer kicks as effects in the store's
//! outbox, handed to a driver one at a time and never sent again by themselves once any of them
//! may have been sent.

use core::time::Duration;

use keel_doc::{Confirm, Doc};
use keel_domain::profile::Printer;
use keel_events::cbor::{Map, Value};
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
/// The kind of a kitchen ticket's job, or a void slip's, at a kitchen printer (ADR-0034).
pub const KITCHEN: &str = "print.kitchen";

/// What a job prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// A receipt.
    Receipt,
    /// The cash drawer's pulse.
    Drawer,
    /// A kitchen ticket, or a void slip, at the kitchen printer it names (ADR-0034).
    Kitchen,
}

impl JobKind {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            JobKind::Receipt => RECEIPT,
            JobKind::Drawer => DRAWER,
            JobKind::Kitchen => KITCHEN,
        }
    }

    pub(crate) fn of(kind: &EffectKind) -> Option<JobKind> {
        match kind.as_str() {
            RECEIPT => Some(JobKind::Receipt),
            DRAWER => Some(JobKind::Drawer),
            KITCHEN => Some(JobKind::Kitchen),
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
    /// The printer it goes to, for a kitchen job; `None` for the register's own.
    pub to: Option<Printer>,
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

/// The effect of a new kitchen job, `doc` printed at `printer`: its payload `{1: document, 2:
/// host, 3: port}`, in canonical CBOR.
pub(crate) fn kitchen_effect(
    job: Id<Job>,
    doc: &Doc,
    printer: &Printer,
    cause: Option<Id<keel_events::envelope::Event>>,
) -> Result<Effect, RuntimeError> {
    let payload = Value::Map(
        Map::from_entries(vec![
            (Value::Unsigned(1), Value::Bytes(doc.encode())),
            (Value::Unsigned(2), Value::from(printer.host.as_str())),
            (Value::Unsigned(3), Value::Unsigned(u64::from(printer.port.get()))),
        ])
        .map_err(|_| RuntimeError::Time)?,
    );
    Ok(Effect {
        key: job.to_bytes().to_vec(),
        kind: EffectKind::new(KITCHEN)?,
        payload: payload.encode(),
        cause,
    })
}

/// A queued job's document, and the printer it names: none for the register's own. A kitchen
/// job's payload is `{1: document, 2: host, 3: port}`; another's, the document.
pub(crate) fn document(queued: &Queued) -> Result<(Doc, Option<Printer>), RuntimeError> {
    if JobKind::of(&queued.effect.kind) != Some(JobKind::Kitchen) {
        return Ok((Doc::decode(&queued.effect.payload)?, None));
    }
    let unreadable = || RuntimeError::Time;
    let value = keel_events::cbor::decode(&queued.effect.payload).map_err(|_| unreadable())?;
    let map = value.as_map().ok_or_else(unreadable)?;
    let get = |key: u64| map.get(&Value::Unsigned(key)).ok_or_else(unreadable);
    let doc = Doc::decode(get(1)?.as_bytes().ok_or_else(unreadable)?)?;
    let host = get(2)?.as_text().ok_or_else(unreadable)?.to_owned();
    let port = get(3)?
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .and_then(core::num::NonZeroU16::new)
        .ok_or_else(unreadable)?;
    Ok((doc, Some(Printer { host, port })))
}

/// A kitchen job's effect, `doc` in place of its document, for the same printer.
pub(crate) fn again(job: Id<Job>, queued: &Queued, doc: &Doc) -> Result<Effect, RuntimeError> {
    match (JobKind::of(&queued.effect.kind), document(queued)?) {
        (Some(JobKind::Kitchen), (_, Some(printer))) => {
            kitchen_effect(job, doc, &printer, queued.effect.cause)
        }
        (Some(kind), _) => effect(job, kind, doc, queued.effect.cause),
        (None, _) => Err(RuntimeError::UnknownJob(job)),
    }
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

/// The longest a job waits before it is tried again.
pub(crate) const MAX_WAIT: Duration = Duration::from_secs(60);

/// The number a job's process ID is made from, for its attempt `attempts`: from its identifier's
/// random bits and the attempt, so that the printer's word for one job is never taken for
/// another's.
pub(crate) fn process_number(job: Id<Job>, attempts: u32) -> u64 {
    let bytes = job.to_bytes();
    let tail: [u8; 8] = bytes.get(8..).and_then(|tail| tail.try_into().ok()).unwrap_or_default();
    // Wrapping is the point: only the number's last digits are used.
    u64::from_be_bytes(tail).wrapping_add(u64::from(attempts))
}

/// How long a job that wasn't sent waits before it is tried again: 2 s after its first
/// attempt, doubling to at most 60 s.
pub(crate) fn backoff(attempts: u32) -> Duration {
    let doublings = attempts.saturating_sub(1).min(5);
    let wait = 2_u64.checked_shl(doublings).unwrap_or(60);
    Duration::from_secs(wait).min(MAX_WAIT)
}

/// `now`, `wait` later.
pub(crate) fn after(now: Timestamp, wait: Duration) -> Result<Timestamp, RuntimeError> {
    let millis = i64::try_from(wait.as_millis()).map_err(|_| RuntimeError::Time)?;
    let later = now.as_millis().checked_add(millis).ok_or(RuntimeError::Time)?;
    Timestamp::from_millis(later).map_err(|_| RuntimeError::Time)
}
