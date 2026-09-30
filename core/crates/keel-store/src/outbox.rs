//! The outbox: effects waiting to happen, kept with the events that caused them (ADR-0017).
//!
//! An effect is something the kernel asks of the world outside it, such as charging a card or
//! printing a receipt. It is enqueued in the write that records its cause, so it exists exactly
//! when the cause does, and it goes through these states:
//!
//! - **pending**, waiting until it is due;
//! - **running**, once its executor has started it: an attempt is under way;
//! - **done**, or **failed** for good, for a person to look at;
//!
//! and a running effect can be made pending again, due later, to retry it. Every change happens
//! inside a write. So an executor starts an effect, and the start commits, before it acts; and the
//! events recording an outcome commit with the effect's finish. After a restart, a running effect
//! is in doubt: its attempt may or may not have happened. The store never restarts it on its own;
//! its executor finds out what happened, by a status query for a payment, and finishes or retries
//! it.

use keel_events::envelope::Event;
use keel_types::{Id, Timestamp};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::check::Problem;
use crate::error::StoreError;
use crate::rows;
use crate::schema::id;

/// An effect's kind, such as `print.receipt`: lowercase ASCII words of letters, digits and
/// underscores, joined by dots, at most 64 bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectKind(String);

impl EffectKind {
    /// The kind named `kind`.
    ///
    /// # Errors
    /// [`EffectError::Invalid`] if `kind` isn't a valid kind.
    pub fn new(kind: &str) -> Result<EffectKind, EffectError> {
        let word = |word: &str| {
            word.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && word.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        };
        if kind.len() > 64 || !kind.split('.').all(word) {
            return Err(EffectError::Invalid("an effect's kind"));
        }
        Ok(EffectKind(kind.to_owned()))
    }

    /// The kind's name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The longest idempotency key, in bytes.
pub const MAX_KEY: usize = 64;

/// The largest payload, in bytes.
pub const MAX_PAYLOAD: usize = 65_536;

/// An effect to enqueue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    /// Its idempotency key, which its executor dedupes on, such as a payment's identifier: 1 to
    /// [`MAX_KEY`] bytes, and unique in the store.
    pub key: Vec<u8>,
    /// What kind of effect it is, which decides its executor.
    pub kind: EffectKind,
    /// What to do, for the executor to read: at most [`MAX_PAYLOAD`] bytes.
    pub payload: Vec<u8>,
    /// The event that caused it, which the store must hold.
    pub cause: Option<Id<Event>>,
}

/// Where an effect is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EffectState {
    /// Waiting until it is due.
    Pending,
    /// Started: an attempt is under way, or was when the store last closed.
    Running,
    /// Done.
    Done,
    /// Failed for good, for a person to look at.
    Failed,
}

impl EffectState {
    /// Every state.
    pub const ALL: [EffectState; 4] =
        [EffectState::Pending, EffectState::Running, EffectState::Done, EffectState::Failed];

    /// The state's code, as the outbox stores it.
    pub const fn code(self) -> &'static str {
        match self {
            EffectState::Pending => "pending",
            EffectState::Running => "running",
            EffectState::Done => "done",
            EffectState::Failed => "failed",
        }
    }

    fn from_code(code: &str) -> Option<EffectState> {
        EffectState::ALL.into_iter().find(|state| state.code() == code)
    }
}

/// An effect in the outbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Queued {
    /// The effect.
    pub effect: Effect,
    /// Where it is.
    pub state: EffectState,
    /// How many times it was started.
    pub attempts: u32,
    /// When it was enqueued.
    pub enqueued: Timestamp,
    /// When it is due, or was.
    pub due: Timestamp,
    /// When it was last started.
    pub started: Option<Timestamp>,
}

/// What enqueuing an effect did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enqueued {
    /// The effect is pending.
    New,
    /// The outbox already held the same effect, under its key: nothing changed.
    Already,
}

/// Why the outbox refused a change.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EffectError {
    /// No effect has the key.
    #[error("no effect has this key")]
    Unknown,
    /// Another effect has the key.
    #[error("another effect has this key")]
    KeyInUse,
    /// The effect's state doesn't allow the change.
    #[error("the effect is {}", .0.code())]
    State(EffectState),
    /// The effect isn't due yet.
    #[error("the effect isn't due until {0}")]
    NotDue(Timestamp),
    /// The event that caused the effect isn't stored.
    #[error("the event that caused the effect isn't stored")]
    UnknownCause,
    /// A key, kind or payload is out of bounds.
    #[error("{0} is invalid")]
    Invalid(&'static str),
}

/// The columns an effect is read from, in order.
const COLUMNS: &str = "key, kind, payload, cause, state, attempts, enqueued, due, started";

fn millis(at: Timestamp) -> i64 {
    at.as_millis()
}

fn timestamp(millis: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_millis(millis).map_err(|_| StoreError::Corrupt("a time"))
}

/// An effect's columns, as stored.
struct Stored {
    key: Vec<u8>,
    kind: String,
    payload: Vec<u8>,
    cause: Option<Vec<u8>>,
    state: String,
    attempts: i64,
    enqueued: i64,
    due: i64,
    started: Option<i64>,
}

impl Stored {
    fn read(self) -> Result<Queued, StoreError> {
        Ok(Queued {
            effect: Effect {
                key: self.key,
                kind: EffectKind::new(&self.kind)
                    .map_err(|_| StoreError::Corrupt("an effect's kind"))?,
                payload: self.payload,
                cause: self.cause.map(|cause| id(&cause)).transpose()?,
            },
            state: EffectState::from_code(&self.state)
                .ok_or(StoreError::Corrupt("an effect's state"))?,
            attempts: u32::try_from(self.attempts)
                .map_err(|_| StoreError::Corrupt("an attempt count"))?,
            enqueued: timestamp(self.enqueued)?,
            due: timestamp(self.due)?,
            started: self.started.map(timestamp).transpose()?,
        })
    }
}

fn queued(row: &Row<'_>) -> rusqlite::Result<Result<Queued, StoreError>> {
    let stored = Stored {
        key: row.get(0)?,
        kind: row.get(1)?,
        payload: row.get(2)?,
        cause: row.get(3)?,
        state: row.get(4)?,
        attempts: row.get(5)?,
        enqueued: row.get(6)?,
        due: row.get(7)?,
        started: row.get(8)?,
    };
    Ok(stored.read())
}

/// The effect with key `key`, if the outbox holds one.
pub(crate) fn get(db: &Connection, key: &[u8]) -> Result<Option<Queued>, StoreError> {
    db.query_row(&format!("SELECT {COLUMNS} FROM outbox WHERE key = ?1"), [key], queued)
        .optional()?
        .transpose()
}

/// Up to `limit` pending effects due by `now`, the earliest due first, then in the order they
/// were enqueued.
pub(crate) fn due(db: &Connection, now: Timestamp, limit: u32) -> Result<Vec<Queued>, StoreError> {
    let mut statement = db.prepare(&format!(
        "SELECT {COLUMNS} FROM outbox WHERE state = 'pending' AND due <= ?1 \
         ORDER BY due, seq LIMIT ?2"
    ))?;
    let rows = statement.query_map(params![millis(now), limit], queued)?;
    rows.map(|row| row?).collect()
}

/// The effects in `state`, in the order they were enqueued.
pub(crate) fn in_state(db: &Connection, state: EffectState) -> Result<Vec<Queued>, StoreError> {
    let mut statement =
        db.prepare(&format!("SELECT {COLUMNS} FROM outbox WHERE state = ?1 ORDER BY seq"))?;
    let rows = statement.query_map([state.code()], queued)?;
    rows.map(|row| row?).collect()
}

/// Checks every effect: that it reads, its cause is stored, and its state, attempts and start
/// time agree. An effect is started once for each attempt, and keeps its last start time.
pub(crate) fn check(db: &Connection, problems: &mut Vec<Problem>) -> Result<(), StoreError> {
    let mut statement = db.prepare(&format!("SELECT {COLUMNS} FROM outbox ORDER BY seq"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let key: Vec<u8> = row.get(0)?;
        let sound = match queued(row)? {
            Ok(queued) => {
                let caused = match queued.effect.cause {
                    Some(cause) => rows::has_id(db, &cause.to_bytes())?,
                    None => true,
                };
                caused
                    && (queued.attempts == 0) == queued.started.is_none()
                    && (queued.state == EffectState::Pending || queued.attempts > 0)
            }
            Err(StoreError::Corrupt(_)) => false,
            Err(error) => return Err(error),
        };
        if !sound {
            problems.push(Problem::Effect { key });
        }
    }
    Ok(())
}

/// Every effect, in the order they were enqueued.
pub(crate) fn all(db: &Connection) -> Result<Vec<Queued>, StoreError> {
    let mut statement = db.prepare(&format!("SELECT {COLUMNS} FROM outbox ORDER BY seq"))?;
    let rows = statement.query_map([], queued)?;
    rows.map(|row| row?).collect()
}

/// Enqueues `effect`, due at `now`.
pub(crate) fn enqueue(
    db: &Connection,
    effect: &Effect,
    now: Timestamp,
) -> Result<Enqueued, StoreError> {
    if effect.key.is_empty() || effect.key.len() > MAX_KEY {
        return Err(EffectError::Invalid("an effect's key").into());
    }
    if effect.payload.len() > MAX_PAYLOAD {
        return Err(EffectError::Invalid("an effect's payload").into());
    }
    if let Some(existing) = get(db, &effect.key)? {
        return if existing.effect == *effect {
            Ok(Enqueued::Already)
        } else {
            Err(EffectError::KeyInUse.into())
        };
    }
    if let Some(cause) = effect.cause
        && !rows::has_id(db, &cause.to_bytes())?
    {
        return Err(EffectError::UnknownCause.into());
    }
    db.execute(
        "INSERT INTO outbox (key, kind, payload, cause, state, attempts, enqueued, due) \
         VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?5)",
        params![
            &effect.key,
            effect.kind.as_str(),
            &effect.payload,
            effect.cause.map(|cause| cause.to_bytes().to_vec()),
            millis(now),
        ],
    )?;
    Ok(Enqueued::New)
}

/// The effect with key `key`, which must be in `state`.
fn expect(db: &Connection, key: &[u8], state: EffectState) -> Result<Queued, StoreError> {
    let queued = get(db, key)?.ok_or(EffectError::Unknown)?;
    if queued.state != state {
        return Err(EffectError::State(queued.state).into());
    }
    Ok(queued)
}

/// Starts the pending effect with key `key` at `now`: it must be due.
pub(crate) fn start(db: &Connection, key: &[u8], now: Timestamp) -> Result<Queued, StoreError> {
    let queued = expect(db, key, EffectState::Pending)?;
    if queued.due > now {
        return Err(EffectError::NotDue(queued.due).into());
    }
    let attempts = queued.attempts.checked_add(1).ok_or(StoreError::OutOfRange("attempts"))?;
    db.execute(
        "UPDATE outbox SET state = 'running', attempts = ?2, started = ?3 WHERE key = ?1",
        params![key, attempts, millis(now)],
    )?;
    Ok(Queued { state: EffectState::Running, attempts, started: Some(now), ..queued })
}

/// Ends the running effect with key `key`, as done or failed.
pub(crate) fn end(db: &Connection, key: &[u8], state: EffectState) -> Result<(), StoreError> {
    expect(db, key, EffectState::Running)?;
    db.execute("UPDATE outbox SET state = ?2 WHERE key = ?1", params![key, state.code()])?;
    Ok(())
}

/// Makes the running effect with key `key` pending again, due at `due`.
pub(crate) fn retry(db: &Connection, key: &[u8], due: Timestamp) -> Result<(), StoreError> {
    expect(db, key, EffectState::Running)?;
    db.execute(
        "UPDATE outbox SET state = 'pending', due = ?2 WHERE key = ?1",
        params![key, millis(due)],
    )?;
    Ok(())
}
