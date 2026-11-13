//! The `kitchen_tickets` projection (ADR-0034): one row per kitchen ticket, so a screen lists a
//! station's tickets, a register finds those no screen showed, and a day's times are read,
//! without folding every ticket; and how many kitchen events the store holds, which a screen
//! follows to know when to read its tickets again.

use keel_domain::aggregate::{Aggregate, fold};
use keel_domain::kitchen::{KitchenTicket, Moment, RerouteTo};
use keel_domain::order::Order;
use keel_domain::refs::Station;
use keel_events::envelope::{self, Device};
use keel_events::event::SignedEvent;
use keel_types::{BusinessDate, Hlc, Id, Timestamp};
use rusqlite::types::Value;
use rusqlite::{Connection, Row, params};

use crate::error::StoreError;
use crate::projection::{Columns, Projection, Span, business_date, bytes, count};
use crate::schema::{hlc, id};

/// The `kitchen_tickets` projection.
pub(crate) const KITCHEN_TICKETS: Projection = Projection {
    name: "kitchen_tickets",
    version: 1,
    kind: "kitchen_ticket",
    key: "ticket_id",
    order: "ticket_id",
    create: "
        CREATE TABLE kitchen_tickets (
            ticket_id BLOB PRIMARY KEY CHECK (length(ticket_id) = 16),
            order_id BLOB,
            station BLOB,
            sent_at INTEGER,
            sent_by BLOB,
            shown_at INTEGER,
            bumped INTEGER NOT NULL,
            bumped_at INTEGER,
            rerouted INTEGER,
            rerouted_at INTEGER,
            business_date TEXT,
            conflicts INTEGER NOT NULL,
            unreadable INTEGER NOT NULL,
            events INTEGER NOT NULL,
            first_hlc BLOB NOT NULL CHECK (length(first_hlc) = 8),
            last_hlc BLOB NOT NULL CHECK (length(last_hlc) = 8)
        ) STRICT;
        CREATE INDEX kitchen_tickets_open ON kitchen_tickets (bumped, sent_at);
        CREATE INDEX kitchen_tickets_order ON kitchen_tickets (order_id);
        CREATE INDEX kitchen_tickets_day ON kitchen_tickets (business_date);
    ",
    drop: "DROP TABLE IF EXISTS kitchen_tickets",
    project: project_ticket,
};

/// A kitchen ticket, as the `kitchen_tickets` projection keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KitchenSummary {
    /// The ticket.
    pub id: Id<KitchenTicket>,
    /// Its order; `None` until it is sent.
    pub order: Option<Id<Order>>,
    /// Its station; `None` until it is sent.
    pub station: Option<Id<Station>>,
    /// When it was sent, and by which device.
    pub sent: Option<(Timestamp, Id<Device>)>,
    /// When a screen first showed it.
    pub shown_at: Option<Timestamp>,
    /// Whether its station is done with it.
    pub bumped: bool,
    /// When it was bumped, if it is.
    pub bumped_at: Option<Timestamp>,
    /// Where it was rerouted, and when, if it was.
    pub rerouted: Option<(RerouteTo, Timestamp)>,
    /// The business date it was sent on.
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

fn micros(moment: Option<Moment>) -> Result<Option<i64>, StoreError> {
    moment
        .map(|moment| moment.at().map(Timestamp::as_micros).ok_or(StoreError::Corrupt("a time")))
        .transpose()
}

fn project_ticket(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let mut ticket = KitchenTicket::new(stream.cast());
    for event in events {
        fold(&mut ticket, event);
    }
    let span = Span::of(events)?;
    let sent = ticket.sent();
    let rerouted = ticket.rerouted();
    db.execute(
        "INSERT OR REPLACE INTO kitchen_tickets (ticket_id, order_id, station, sent_at, sent_by, \
         shown_at, bumped, bumped_at, rerouted, rerouted_at, business_date, conflicts, \
         unreadable, events, first_hlc, last_hlc) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            &ticket.stream_id().to_bytes()[..],
            bytes(sent.map(|(sent, _)| sent.order)),
            bytes(sent.map(|(sent, _)| sent.station)),
            micros(sent.map(|(_, moment)| moment))?,
            bytes(sent.map(|(_, moment)| moment.device)),
            micros(ticket.shown())?,
            i64::from(ticket.is_bumped()),
            micros(ticket.bumped())?,
            rerouted.map(|(to, _)| i64::try_from(to.code()).unwrap_or(i64::MAX)),
            micros(rerouted.map(|(_, moment)| moment))?,
            sent.and_then(|(_, moment)| business_date(events, moment.event)),
            count(ticket.conflicts().len())?,
            count(ticket.skipped().len())?,
            span.events,
            &span.first[..],
            &span.last[..],
        ],
    )?;
    Ok(())
}

/// The columns a kitchen summary is read from, in order.
pub(crate) const KITCHEN_COLUMNS: &str = "ticket_id, order_id, station, sent_at, sent_by, \
    shown_at, bumped, bumped_at, rerouted, rerouted_at, business_date, conflicts, unreadable, \
    events, first_hlc, last_hlc";

/// Reads a kitchen summary from a row of [`KITCHEN_COLUMNS`].
pub(crate) fn kitchen_summary(
    row: &Row<'_>,
) -> rusqlite::Result<Result<KitchenSummary, StoreError>> {
    let values: Vec<Value> = (0..16).map(|column| row.get(column)).collect::<Result<_, _>>()?;
    Ok(read_kitchen(&values))
}

fn time(micros: Option<i64>) -> Result<Option<Timestamp>, StoreError> {
    micros.map(Timestamp::from_micros).transpose().map_err(|_| StoreError::Corrupt("a time"))
}

fn read_kitchen(values: &[Value]) -> Result<KitchenSummary, StoreError> {
    let at = Columns(values);
    let sent = match (time(at.optional_integer(3)?)?, at.optional_id(4)?) {
        (Some(when), Some(device)) => Some((when, device)),
        (None, None) => None,
        _ => return Err(StoreError::Corrupt("a ticket's sending")),
    };
    let rerouted = match (at.optional_integer(8)?, time(at.optional_integer(9)?)?) {
        (Some(code), Some(when)) => {
            let code = u64::try_from(code).map_err(|_| StoreError::Corrupt("a reroute"))?;
            let to = RerouteTo::from_code(code).ok_or(StoreError::Corrupt("a reroute"))?;
            Some((to, when))
        }
        (None, None) => None,
        _ => return Err(StoreError::Corrupt("a reroute")),
    };
    Ok(KitchenSummary {
        id: id(&at.blob(0)?)?,
        order: at.optional_id(1)?,
        station: at.optional_id(2)?,
        sent,
        shown_at: time(at.optional_integer(5)?)?,
        bumped: at.integer(6)? != 0,
        bumped_at: time(at.optional_integer(7)?)?,
        rerouted,
        business_date: at.optional_date(10)?,
        conflicts: at.unsigned(11)?,
        unreadable: at.unsigned(12)?,
        events: at.unsigned(13)?,
        first: hlc(&at.blob(14)?)?,
        last: hlc(&at.blob(15)?)?,
    })
}
