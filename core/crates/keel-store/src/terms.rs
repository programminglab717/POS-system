//! Hub terms (ADR-0022): the claims the store holds, the chain of terms they make, and claiming
//! the Store Hub's role.
//!
//! - **Claims.** The claims projection keeps a row for each claim the store holds: a stream of
//!   kind `hub` whose first event in canonical order is a `hub.claimed` event, by its claimant.
//!   A claim this kernel can't read claims nothing.
//! - **Terms.** Each write that touches a claim works out the chain of terms again, from every
//!   claim the store holds ([`hub::chain`]), and keeps it in the `terms` table: each term's
//!   epoch, device and claim, the position of the claim in the device's log, and the term's cut.
//!   A sequencing record counts when its device holds the term of its epoch, and it follows the
//!   claim and lies within the cut ([`COUNTING`]); confirmation, the feed and sequencing count
//!   no others. The winning claim's term is the hub's.
//! - **Claiming.** The store claims the hub's role when it holds the whole chain and every
//!   record that counts on it: the next epoch, succeeding the winning claim, cutting each device
//!   on the chain where the store holds its log to. A claim isn't a business event; it is filed
//!   under the business date of the latest event the store stored, or the UTC date when the
//!   store holds none.

use core::num::NonZeroU8;
use std::collections::BTreeMap;

use keel_domain::hub::{self, Claim, ClaimError, Claimed, Cut, Epoch, HubEvent, Term};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{self, Actor, Component, Device, StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::Signer;
use keel_events::log::{AppendError, EventDraft};
use keel_types::{BusinessDate, BusinessDayPolicy, Entropy, Id, Timestamp};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StoreError;
use crate::rows::{self, seq_value};
use crate::schema::id;
use crate::store::noted;
use crate::write::Writing;

/// The sequencing runs that count: those of records whose device holds the term of their epoch,
/// that follow the term's claim and lie within its cut. Joined as `s`, the runs, and `t`, the
/// terms; the runs are read first, by whatever index the query's conditions on them suit, and
/// each looks up its term.
pub(crate) const COUNTING: &str = "sequence AS s CROSS JOIN terms AS t ON t.epoch = s.epoch \
    AND t.device = s.author AND s.author_seq > t.after \
    AND (t.through IS NULL OR s.author_seq <= t.through)";

/// The claims projection's tables: a row for each claim, and the chain they make.
pub(crate) const CREATE: &str = "
    CREATE TABLE claims (
        stream BLOB PRIMARY KEY CHECK (length(stream) = 16),
        claim BLOB NOT NULL UNIQUE CHECK (length(claim) = 16),
        device BLOB NOT NULL CHECK (length(device) = 16),
        position INTEGER NOT NULL CHECK (position >= 1),
        epoch INTEGER NOT NULL CHECK (epoch >= 1),
        priority INTEGER NOT NULL CHECK (priority BETWEEN 1 AND 255),
        previous BLOB CHECK (length(previous) = 16),
        cuts BLOB CHECK (length(cuts) % 24 = 0),
        CHECK ((previous IS NULL) = (cuts IS NULL))
    ) STRICT;
    CREATE TABLE terms (
        epoch INTEGER PRIMARY KEY CHECK (epoch >= 1),
        device BLOB NOT NULL CHECK (length(device) = 16),
        claim BLOB NOT NULL CHECK (length(claim) = 16),
        after INTEGER NOT NULL CHECK (after >= 1),
        through INTEGER CHECK (through >= 0)
    ) STRICT;
";

/// Drops the claims projection's tables.
pub(crate) const DROP: &str = "DROP TABLE IF EXISTS claims; DROP TABLE IF EXISTS terms;";

/// Replaces the row of the claim `stream`, whose first event in canonical order is the claim,
/// and works out the chain again.
pub(crate) fn project_claims(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let key = stream.to_bytes();
    db.execute("DELETE FROM claims WHERE stream = ?1", [&key[..]])?;
    if let Some(first) = events.first() {
        let body = first.body();
        if let Ok(HubEvent::Claimed(claimed)) = HubEvent::decode(&body.schema, &body.payload) {
            let succession = claimed.succeeds.as_ref();
            db.execute(
                "INSERT INTO claims (stream, claim, device, position, epoch, priority, previous, \
                 cuts) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    &key[..],
                    &body.event_id.to_bytes()[..],
                    &body.origin_device.to_bytes()[..],
                    seq_value(body.origin_seq.get())?,
                    seq_value(claimed.epoch.get())?,
                    i64::from(claimed.priority.get()),
                    succession.map(|succession| succession.previous.to_bytes().to_vec()),
                    succession.map(|succession| cuts_bytes(&succession.cuts)),
                ],
            )?;
        }
    }
    settle(db)
}

/// Works out the chain from every claim the store holds, and keeps it in the `terms` table.
fn settle(db: &Connection) -> Result<(), StoreError> {
    let chain = hub::chain(&claims(db)?);
    db.execute("DELETE FROM terms", [])?;
    let mut statement = db.prepare(
        "INSERT INTO terms (epoch, device, claim, after, through) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for term in chain {
        statement.execute(params![
            seq_value(term.epoch.get())?,
            &term.device.to_bytes()[..],
            &term.claim.to_bytes()[..],
            seq_value(term.after)?,
            term.through.map(seq_value).transpose()?,
        ])?;
    }
    Ok(())
}

/// A claim's cuts as stored: for each, the device's 16 bytes and the position's 8, big-endian.
fn cuts_bytes(cuts: &[Cut]) -> Vec<u8> {
    cuts.iter()
        .flat_map(|cut| cut.device.to_bytes().into_iter().chain(cut.position.to_be_bytes()))
        .collect()
}

/// Every claim the store holds, from the claims projection.
pub(crate) fn claims(db: &Connection) -> Result<Vec<Claim>, StoreError> {
    let corrupt = || StoreError::Corrupt("a claim");
    let mut statement = db.prepare(
        "SELECT claim, device, position, epoch, priority, previous, cuts FROM claims \
         ORDER BY stream",
    )?;
    let mut rows = statement.query([])?;
    let mut claims = Vec::new();
    while let Some(row) = rows.next()? {
        let previous: Option<Vec<u8>> = row.get(5)?;
        let cuts: Option<Vec<u8>> = row.get(6)?;
        let succeeds = match (previous, cuts) {
            (Some(previous), Some(cuts)) => Some(hub::Succession {
                previous: id(&previous)?,
                cuts: cuts
                    .chunks(24)
                    .map(|cut| {
                        let (device, position) = cut.split_at_checked(16).ok_or_else(corrupt)?;
                        let position = <[u8; 8]>::try_from(position).map_err(|_| corrupt())?;
                        Ok(Cut { device: id(device)?, position: u64::from_be_bytes(position) })
                    })
                    .collect::<Result<_, StoreError>>()?,
            }),
            (None, None) => None,
            _ => return Err(corrupt()),
        };
        let epoch = Epoch::new(unsigned(row.get(3)?)?).ok_or_else(corrupt)?;
        let priority = u8::try_from(row.get::<_, i64>(4)?)
            .ok()
            .and_then(NonZeroU8::new)
            .ok_or_else(corrupt)?;
        claims.push(Claim {
            event: id(&row.get::<_, Vec<u8>>(0)?)?,
            device: id(&row.get::<_, Vec<u8>>(1)?)?,
            position: unsigned(row.get(2)?)?,
            claimed: Claimed::new(epoch, priority, succeeds).map_err(|_| corrupt())?,
        });
    }
    Ok(claims)
}

/// The chain of terms the store keeps: from the winning claim's term back.
pub(crate) fn chain(db: &Connection) -> Result<Vec<Term>, StoreError> {
    let corrupt = || StoreError::Corrupt("a term");
    let mut statement =
        db.prepare("SELECT epoch, device, claim, after, through FROM terms ORDER BY epoch DESC")?;
    let mut rows = statement.query([])?;
    let mut terms = Vec::new();
    while let Some(row) = rows.next()? {
        terms.push(Term {
            epoch: Epoch::new(unsigned(row.get(0)?)?).ok_or_else(corrupt)?,
            device: id(&row.get::<_, Vec<u8>>(1)?)?,
            claim: id(&row.get::<_, Vec<u8>>(2)?)?,
            after: unsigned(row.get(3)?)?,
            through: row.get::<_, Option<i64>>(4)?.map(unsigned).transpose()?,
        });
    }
    Ok(terms)
}

/// The winning claim's term: the hub's, first on the chain. `None` while the store holds no
/// claim.
pub(crate) fn winning(db: &Connection) -> Result<Option<Term>, StoreError> {
    Ok(chain(db)?.into_iter().next())
}

/// The epoch of the term the store's device holds as the hub: refused if it doesn't hold the
/// winning claim.
pub(crate) fn own_epoch(db: &Connection, own: Id<Device>) -> Result<Epoch, StoreError> {
    match winning(db)? {
        Some(term) if term.device == own => Ok(term.epoch),
        _ => Err(StoreError::NotHub),
    }
}

fn unsigned(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt("a term"))
}

/// Why a claim can't be made: its epochs have run out, or too many devices hold terms.
fn unrecordable<T>(_: T) -> StoreError {
    StoreError::OutOfRange("a claim")
}

impl<S: Signer, E: Entropy> Writing<'_, S, E> {
    /// Claims the hub's role at `priority`, at physical time `now`, unless the store already
    /// holds the winning claim. Returns the claim.
    pub(crate) fn claim(
        &mut self,
        priority: NonZeroU8,
        now: Timestamp,
    ) -> Result<Option<SignedEvent>, StoreError> {
        let health = self.health;
        let own = self.writer.device();
        let (chain, held, business_date) = (|| {
            let chain = chain(self.tx)?;
            let mut held = BTreeMap::new();
            for term in &chain {
                held.insert(term.device, rows::head(self.tx, term.device)?.seq());
            }
            Ok((chain, held, latest_date(self.tx, now)?))
        })()
        .map_err(|error| noted(health, error))?;
        if chain.first().is_some_and(|term| term.device == own) {
            return Ok(None);
        }
        let held = |device| held.get(&device).copied().unwrap_or(0);
        let claimed = match Claimed::succeeding(&chain, priority, held) {
            Ok(claimed) => claimed,
            Err(ClaimError::Behind) => return Err(StoreError::Behind),
            Err(error) => return Err(unrecordable(error)),
        };
        let (schema, payload) = HubEvent::Claimed(claimed).encode().map_err(unrecordable)?;
        let stream = StreamRef {
            kind: StreamKind::new(hub::STREAM).map_err(unrecordable)?,
            id: self.writer.generate_id(now).map_err(AppendError::from)?,
        };
        let draft = EventDraft {
            stream,
            schema,
            business_date,
            actor: Actor::System(Component::new("hub").map_err(unrecordable)?),
            approval: None,
            causation: None,
            correlation: None,
            payload,
        };
        Ok(Some(self.append(draft, now)?))
    }
}

/// The business date of the latest event the store stored, or the UTC date of `now` if it holds
/// none.
fn latest_date(db: &Connection, now: Timestamp) -> Result<BusinessDate, StoreError> {
    let latest = db
        .query_row(
            "SELECT message, hash FROM events ORDER BY arrival DESC LIMIT 1",
            [],
            rows::stored_event,
        )
        .optional()?;
    if let Some(event) = latest {
        return Ok(event?.body().business_date);
    }
    BusinessDayPolicy::new("UTC", 0, 0)
        .and_then(|utc| utc.business_date_of(now))
        .map_err(|_| StoreError::OutOfRange("a claim's business date"))
}
