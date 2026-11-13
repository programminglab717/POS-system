//! The `availability` projection (ADR-0033): one row per item whose availability changed, so a
//! register asks whether an item is sold out, and lists those that are, without reading its
//! events; and how many availability events the store holds, which a shell follows to know when
//! to read the menu again.

use keel_domain::aggregate::{Aggregate, fold};
use keel_domain::availability::{Availability, ItemKind};
use keel_events::envelope;
use keel_events::event::SignedEvent;
use keel_types::{BusinessDate, Hlc, Id, Timestamp};
use rusqlite::types::Value;
use rusqlite::{Connection, Row, params};

use crate::error::StoreError;
use crate::projection::{Columns, Projection, Span, business_date, bytes, count};
use crate::schema::{hlc, id};

/// The `availability` projection.
pub(crate) const AVAILABILITY: Projection = Projection {
    name: "availability",
    version: 1,
    kind: "availability",
    key: "item_id",
    order: "item_id",
    create: "
        CREATE TABLE availability (
            item_id BLOB PRIMARY KEY CHECK (length(item_id) = 16),
            kind INTEGER,
            available INTEGER,
            until INTEGER,
            location BLOB,
            changed_by BLOB,
            device BLOB,
            business_date TEXT,
            conflicts INTEGER NOT NULL,
            unreadable INTEGER NOT NULL,
            events INTEGER NOT NULL,
            first_hlc BLOB NOT NULL CHECK (length(first_hlc) = 8),
            last_hlc BLOB NOT NULL CHECK (length(last_hlc) = 8)
        ) STRICT;
        CREATE INDEX availability_sold_out ON availability (available, until);
    ",
    drop: "DROP TABLE IF EXISTS availability",
    project: project_availability,
};

/// An item's availability, as the `availability` projection keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailabilitySummary {
    /// The item: a variant's or a modifier's identifier.
    pub id: Id<Availability>,
    /// The change that stands, if one applied: what kind of item, whether on sale, and until
    /// when.
    pub latest: Option<(ItemKind, bool, Option<Timestamp>)>,
    /// The event that made it.
    pub changed_by: Option<Id<envelope::Event>>,
    /// The business date of that event.
    pub business_date: Option<BusinessDate>,
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

impl AvailabilitySummary {
    /// Whether the item is sold out at `now`: 86'd, and not yet back on sale.
    pub fn sold_out_at(&self, now: Timestamp) -> bool {
        self.latest.is_some_and(|(_, available, until)| {
            !available && until.is_none_or(|until| now < until)
        })
    }
}

fn project_availability(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let mut availability = Availability::new(stream.cast());
    for event in events {
        fold(&mut availability, event);
    }
    let span = Span::of(events)?;
    let latest = availability.latest();
    db.execute(
        "INSERT OR REPLACE INTO availability (item_id, kind, available, until, location, \
         changed_by, device, business_date, conflicts, unreadable, events, first_hlc, last_hlc) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            &availability.stream_id().to_bytes()[..],
            latest.map(|latest| i64::from(latest.changed.kind == ItemKind::Modifier)),
            latest.map(|latest| i64::from(latest.changed.available)),
            latest.and_then(|latest| latest.changed.until).map(Timestamp::as_micros),
            bytes(availability.location()),
            bytes(latest.map(|latest| latest.event)),
            bytes(latest.map(|latest| latest.device)),
            latest.and_then(|latest| business_date(events, latest.event)),
            count(availability.conflicts().len())?,
            count(availability.skipped().len())?,
            span.events,
            &span.first[..],
            &span.last[..],
        ],
    )?;
    Ok(())
}

/// The columns an availability summary is read from, in order.
pub(crate) const AVAILABILITY_COLUMNS: &str = "item_id, kind, available, until, changed_by, \
    business_date, conflicts, unreadable, events, first_hlc, last_hlc";

/// Reads an availability summary from a row of [`AVAILABILITY_COLUMNS`].
pub(crate) fn availability_summary(
    row: &Row<'_>,
) -> rusqlite::Result<Result<AvailabilitySummary, StoreError>> {
    let values: Vec<Value> = (0..11).map(|column| row.get(column)).collect::<Result<_, _>>()?;
    Ok(read_availability(&values))
}

fn read_availability(values: &[Value]) -> Result<AvailabilitySummary, StoreError> {
    let at = Columns(values);
    let latest = match (at.optional_integer(1)?, at.optional_integer(2)?) {
        (Some(kind), Some(available)) => {
            let kind = match kind {
                0 => ItemKind::Variant,
                1 => ItemKind::Modifier,
                _ => return Err(StoreError::Corrupt("an item's kind")),
            };
            let until = at
                .optional_integer(3)?
                .map(Timestamp::from_micros)
                .transpose()
                .map_err(|_| StoreError::Corrupt("a time"))?;
            Some((kind, available != 0, until))
        }
        (None, None) => None,
        _ => return Err(StoreError::Corrupt("an item's availability")),
    };
    Ok(AvailabilitySummary {
        id: id(&at.blob(0)?)?,
        latest,
        changed_by: at.optional_id(4)?,
        business_date: at.optional_date(5)?,
        conflicts: at.unsigned(6)?,
        unreadable: at.unsigned(7)?,
        events: at.unsigned(8)?,
        first: hlc(&at.blob(9)?)?,
        last: hlc(&at.blob(10)?)?,
    })
}
