//! The `refunds` projection (ADR-0032): one row per refund, so a register finds an order's
//! refunds, and a day's report finds the day's, without reading every event.

use keel_domain::aggregate::{Aggregate, fold};
use keel_domain::payment::Tender;
use keel_domain::refund::{Refund, RefundCompleted, RefundInfo};
use keel_events::envelope;
use keel_events::event::SignedEvent;
use keel_types::{BusinessDate, Hlc, Id, Money};
use rusqlite::types::Value;
use rusqlite::{Connection, Row, params};

use crate::error::StoreError;
use crate::projection::{Columns, Projection, Span, business_date, bytes, code, count, currency};
use crate::schema::{hlc, id};

/// The `refunds` projection.
pub(crate) const REFUNDS: Projection = Projection {
    name: "refunds",
    version: 1,
    kind: "refund",
    key: "refund_id",
    order: "refund_id",
    create: "
        CREATE TABLE refunds (
            refund_id BLOB PRIMARY KEY CHECK (length(refund_id) = 16),
            state TEXT NOT NULL CHECK (state IN ('unstarted', 'initiated', 'completed')),
            location BLOB,
            device BLOB,
            order_id BLOB,
            tender INTEGER,
            amount INTEGER,
            currency TEXT,
            initiated_by BLOB,
            business_date TEXT,
            refunded INTEGER,
            rounding INTEGER,
            conflicts INTEGER NOT NULL,
            unreadable INTEGER NOT NULL,
            events INTEGER NOT NULL,
            first_hlc BLOB NOT NULL CHECK (length(first_hlc) = 8),
            last_hlc BLOB NOT NULL CHECK (length(last_hlc) = 8)
        ) STRICT;
        CREATE INDEX refunds_by_order ON refunds (order_id, first_hlc);
        CREATE INDEX refunds_by_date ON refunds (business_date, first_hlc);
    ",
    drop: "DROP TABLE IF EXISTS refunds",
    project: project_refund,
};

/// Where a refund is in its life, as the `refunds` projection keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RefundState {
    /// The store holds events of the refund, but not the one that started it.
    Unstarted,
    /// Started, not yet paid back.
    Initiated,
    /// Paid back.
    Completed,
}

impl RefundState {
    /// Every state.
    pub const ALL: [RefundState; 3] =
        [RefundState::Unstarted, RefundState::Initiated, RefundState::Completed];

    /// The state of `refund`.
    pub const fn of(refund: &Refund) -> RefundState {
        if refund.info().is_none() {
            RefundState::Unstarted
        } else if refund.completed().is_some() {
            RefundState::Completed
        } else {
            RefundState::Initiated
        }
    }

    pub(crate) const fn code(self) -> &'static str {
        match self {
            RefundState::Unstarted => "unstarted",
            RefundState::Initiated => "initiated",
            RefundState::Completed => "completed",
        }
    }

    fn from_code(code: &str) -> Option<RefundState> {
        RefundState::ALL.into_iter().find(|state| state.code() == code)
    }
}

/// A refund, as the `refunds` projection keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundSummary {
    /// The refund.
    pub id: Id<Refund>,
    /// Where it is in its life.
    pub state: RefundState,
    /// What it was started with; `None` until it is started.
    pub info: Option<RefundInfo>,
    /// The business date of the event that started it.
    pub business_date: Option<BusinessDate>,
    /// Its completion, once paid back.
    pub completed: Option<RefundCompleted>,
    /// The conflicts in its history, for a person to look at.
    pub conflicts: u64,
    /// Its events this kernel couldn't read: a newer kernel wrote them.
    pub unreadable: u64,
    /// How many events it has.
    pub events: u64,
    /// The HLC of its first event, in canonical order.
    pub first: Hlc,
    /// The HLC of its last event, in canonical order.
    pub last: Hlc,
}

fn project_refund(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let mut refund = Refund::new(stream.cast());
    for event in events {
        fold(&mut refund, event);
    }
    let span = Span::of(events)?;
    let info = refund.info();
    let completed = refund.completed();
    db.execute(
        "INSERT OR REPLACE INTO refunds (refund_id, state, location, device, order_id, tender, \
         amount, currency, initiated_by, business_date, refunded, rounding, conflicts, \
         unreadable, events, first_hlc, last_hlc) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, \
         ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            &refund.stream_id().to_bytes()[..],
            RefundState::of(&refund).code(),
            bytes(info.map(|info| info.location)),
            bytes(info.map(|info| info.device)),
            bytes(info.map(|info| info.order)),
            code(info.map(|info| info.tender.code()))?,
            info.map(|info| info.amount.minor()),
            info.map(|info| info.amount.currency().code()),
            bytes(info.map(|info| info.initiated_by)),
            info.and_then(|info| business_date(events, info.initiated_by)),
            completed.map(|completed| completed.amount.minor()),
            completed.and_then(|completed| completed.rounding).map(Money::minor),
            count(refund.conflicts().len())?,
            count(refund.skipped().len())?,
            span.events,
            &span.first[..],
            &span.last[..],
        ],
    )?;
    Ok(())
}

/// The columns a refund summary is read from, in order.
pub(crate) const REFUND_COLUMNS: &str = "refund_id, state, location, device, order_id, tender, \
    amount, currency, initiated_by, business_date, refunded, rounding, conflicts, unreadable, \
    events, first_hlc, last_hlc";

/// Reads a refund summary from a row of [`REFUND_COLUMNS`].
pub(crate) fn refund_summary(row: &Row<'_>) -> rusqlite::Result<Result<RefundSummary, StoreError>> {
    let values: Vec<Value> = (0..17).map(|column| row.get(column)).collect::<Result<_, _>>()?;
    Ok(read_refund(&values))
}

fn read_refund(values: &[Value]) -> Result<RefundSummary, StoreError> {
    let at = Columns(values);
    let state =
        RefundState::from_code(&at.text(1)?).ok_or(StoreError::Corrupt("a refund's state"))?;
    let info = match at.optional_blob(2)? {
        None => None,
        Some(location) => {
            let currency = currency(&at.text(7)?)?;
            Some(RefundInfo {
                location: id(&location)?,
                device: id(&at.blob(3)?)?,
                order: id(&at.blob(4)?)?,
                tender: Tender::from_code(at.unsigned(5)?)
                    .ok_or(StoreError::Corrupt("a refund's tender"))?,
                amount: Money::from_minor(at.integer(6)?, currency),
                initiated_by: id(&at.blob(8)?)?,
            })
        }
    };
    let money = |column: usize| -> Result<Option<Money>, StoreError> {
        let Some(minor) = at.optional_integer(column)? else { return Ok(None) };
        let currency = info
            .as_ref()
            .map(|info| info.amount.currency())
            .ok_or(StoreError::Corrupt("a completion without a refund"))?;
        Ok(Some(Money::from_minor(minor, currency)))
    };
    let completed = match money(10)? {
        Some(amount) => Some(RefundCompleted { amount, rounding: money(11)? }),
        None if at.optional_integer(11)?.is_none() => None,
        None => return Err(StoreError::Corrupt("a rounding without a completion")),
    };
    Ok(RefundSummary {
        id: id(&at.blob(0)?)?,
        state,
        info,
        business_date: at.optional_date(9)?,
        completed,
        conflicts: at.unsigned(12)?,
        unreadable: at.unsigned(13)?,
        events: at.unsigned(14)?,
        first: hlc(&at.blob(15)?)?,
        last: hlc(&at.blob(16)?)?,
    })
}
