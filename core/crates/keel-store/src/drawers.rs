//! The `drawers` projection (ADR-0031): one row per drawer session, so a device finds its open
//! session, and a day's report finds that day's sessions, without reading every event; and one
//! row per payment whose cash a session took, so a report finds which cash no drawer counted,
//! whatever day the session opened. Version 2 adds those; found in review.

use keel_domain::aggregate::{Aggregate, fold};
use keel_domain::drawer::{Drawer, DrawerClosed, DrawerInfo, Sums};
use keel_events::envelope;
use keel_events::event::SignedEvent;
use keel_types::{BusinessDate, Currency, Hlc, Id, Money};
use rusqlite::types::Value;
use rusqlite::{Connection, Row, params};

use crate::error::StoreError;
use crate::projection::{Columns, Projection, Span, business_date, bytes, count};
use crate::schema::{hlc, id};

/// The `drawers` projection.
pub(crate) const DRAWERS: Projection = Projection {
    name: "drawers",
    version: 2,
    kind: "drawer",
    key: "drawer_id",
    order: "drawer_id",
    create: "
        CREATE TABLE drawers (
            drawer_id BLOB PRIMARY KEY CHECK (length(drawer_id) = 16),
            state TEXT NOT NULL CHECK (state IN ('unopened', 'open', 'closed')),
            location BLOB,
            device BLOB,
            currency TEXT,
            float INTEGER,
            opened_by BLOB,
            business_date TEXT,
            taken INTEGER,
            paid_in INTEGER,
            paid_out INTEGER,
            dropped INTEGER,
            no_sales INTEGER NOT NULL,
            counted INTEGER,
            expected INTEGER,
            conflicts INTEGER NOT NULL,
            unreadable INTEGER NOT NULL,
            events INTEGER NOT NULL,
            first_hlc BLOB NOT NULL CHECK (length(first_hlc) = 8),
            last_hlc BLOB NOT NULL CHECK (length(last_hlc) = 8)
        ) STRICT;
        CREATE INDEX drawers_by_device ON drawers (device, state, first_hlc);
        CREATE INDEX drawers_by_date ON drawers (business_date, first_hlc);
        CREATE TABLE drawer_payments (
            payment_id BLOB NOT NULL CHECK (length(payment_id) = 16),
            drawer_id BLOB NOT NULL CHECK (length(drawer_id) = 16),
            PRIMARY KEY (payment_id, drawer_id)
        ) STRICT;
        CREATE INDEX drawer_payments_by_drawer ON drawer_payments (drawer_id);
    ",
    drop: "DROP TABLE IF EXISTS drawer_payments; DROP TABLE IF EXISTS drawers",
    project: project_drawer,
};

/// Where a drawer session is in its life, as the `drawers` projection keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DrawerState {
    /// The store holds events of the session, but not the one that opened it.
    Unopened,
    /// Opened, not yet closed.
    Open,
    /// Counted and closed.
    Closed,
}

impl DrawerState {
    /// Every state.
    pub const ALL: [DrawerState; 3] =
        [DrawerState::Unopened, DrawerState::Open, DrawerState::Closed];

    /// The state of `drawer`.
    pub const fn of(drawer: &Drawer) -> DrawerState {
        if drawer.info().is_none() {
            DrawerState::Unopened
        } else if drawer.closed().is_some() {
            DrawerState::Closed
        } else {
            DrawerState::Open
        }
    }

    pub(crate) const fn code(self) -> &'static str {
        match self {
            DrawerState::Unopened => "unopened",
            DrawerState::Open => "open",
            DrawerState::Closed => "closed",
        }
    }

    fn from_code(code: &str) -> Option<DrawerState> {
        DrawerState::ALL.into_iter().find(|state| state.code() == code)
    }
}

/// A drawer session, as the `drawers` projection keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawerSummary {
    /// The session.
    pub id: Id<Drawer>,
    /// Where it is in its life.
    pub state: DrawerState,
    /// What it was opened with; `None` until it is opened.
    pub info: Option<DrawerInfo>,
    /// The business date of the event that opened it.
    pub business_date: Option<BusinessDate>,
    /// What its cash came to; `None` until it is opened.
    pub sums: Option<Sums>,
    /// How many times it was opened with no sale.
    pub no_sales: u64,
    /// Its close, once counted.
    pub closed: Option<DrawerClosed>,
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

fn project_drawer(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let mut drawer = Drawer::new(stream.cast());
    for event in events {
        fold(&mut drawer, event);
    }
    let span = Span::of(events)?;
    let info = drawer.info();
    let sums = drawer.sums();
    let closed = drawer.closed();
    db.execute(
        "INSERT OR REPLACE INTO drawers (drawer_id, state, location, device, currency, float, \
         opened_by, business_date, taken, paid_in, paid_out, dropped, no_sales, counted, \
         expected, conflicts, unreadable, events, first_hlc, last_hlc) VALUES (?1, ?2, ?3, ?4, \
         ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            &drawer.stream_id().to_bytes()[..],
            DrawerState::of(&drawer).code(),
            bytes(info.map(|info| info.location)),
            bytes(info.map(|info| info.device)),
            info.map(|info| info.float.currency().code()),
            info.map(|info| info.float.minor()),
            bytes(info.map(|info| info.opened_by)),
            info.and_then(|info| business_date(events, info.opened_by)),
            sums.map(|sums| sums.taken.minor()),
            sums.map(|sums| sums.paid_in.minor()),
            sums.map(|sums| sums.paid_out.minor()),
            sums.map(|sums| sums.dropped.minor()),
            i64::try_from(drawer.no_sales()).map_err(|_| StoreError::Corrupt("a count"))?,
            closed.map(|closed| closed.counted.minor()),
            closed.map(|closed| closed.expected.minor()),
            count(drawer.conflicts().len())?,
            count(drawer.skipped().len())?,
            span.events,
            &span.first[..],
            &span.last[..],
        ],
    )?;
    let session = &drawer.stream_id().to_bytes()[..];
    db.execute("DELETE FROM drawer_payments WHERE drawer_id = ?1", [session])?;
    let mut insert =
        db.prepare("INSERT INTO drawer_payments (payment_id, drawer_id) VALUES (?1, ?2)")?;
    for payment in drawer.payments() {
        insert.execute(params![&payment.to_bytes()[..], session])?;
    }
    Ok(())
}

/// The columns a drawer summary is read from, in order.
pub(crate) const DRAWER_COLUMNS: &str = "drawer_id, state, location, device, currency, float, \
    opened_by, business_date, taken, paid_in, paid_out, dropped, no_sales, counted, expected, \
    conflicts, unreadable, events, first_hlc, last_hlc";

/// Reads a drawer summary from a row of [`DRAWER_COLUMNS`].
pub(crate) fn drawer_summary(row: &Row<'_>) -> rusqlite::Result<Result<DrawerSummary, StoreError>> {
    let values: Vec<Value> = (0..20).map(|column| row.get(column)).collect::<Result<_, _>>()?;
    Ok(read_drawer(&values))
}

fn read_drawer(values: &[Value]) -> Result<DrawerSummary, StoreError> {
    let at = Columns(values);
    let state =
        DrawerState::from_code(&at.text(1)?).ok_or(StoreError::Corrupt("a drawer's state"))?;
    let currency = match at.optional_text(4)? {
        Some(code) => {
            Some(Currency::from_code(&code).map_err(|_| StoreError::Corrupt("a currency"))?)
        }
        None => None,
    };
    let money = |column: usize| -> Result<Option<Money>, StoreError> {
        let Some(minor) = at.optional_integer(column)? else { return Ok(None) };
        let currency = currency.ok_or(StoreError::Corrupt("an amount without a drawer"))?;
        Ok(Some(Money::from_minor(minor, currency)))
    };
    let info = match (at.optional_blob(2)?, money(5)?) {
        (Some(location), Some(float)) => Some(DrawerInfo {
            location: id(&location)?,
            device: id(&at.blob(3)?)?,
            float,
            opened_by: id(&at.blob(6)?)?,
        }),
        (None, None) => None,
        _ => return Err(StoreError::Corrupt("a drawer's opening")),
    };
    let sums = match (money(8)?, money(9)?, money(10)?, money(11)?) {
        (Some(taken), Some(paid_in), Some(paid_out), Some(dropped)) => {
            Some(Sums { taken, paid_in, paid_out, dropped })
        }
        (None, None, None, None) => None,
        _ => return Err(StoreError::Corrupt("a drawer's sums")),
    };
    let closed = match (money(13)?, money(14)?) {
        (Some(counted), Some(expected)) => Some(DrawerClosed { counted, expected }),
        (None, None) => None,
        _ => return Err(StoreError::Corrupt("a drawer's close")),
    };
    Ok(DrawerSummary {
        id: id(&at.blob(0)?)?,
        state,
        info,
        business_date: at.optional_date(7)?,
        sums,
        no_sales: at.unsigned(12)?,
        closed,
        conflicts: at.unsigned(15)?,
        unreadable: at.unsigned(16)?,
        events: at.unsigned(17)?,
        first: hlc(&at.blob(18)?)?,
        last: hlc(&at.blob(19)?)?,
    })
}
