//! The stock projections (ADR-0036): what each item's stock stream moved and counted, one row
//! per entry that applied; and what each order sold and returned, one row per line, as of its
//! close or its return. A level is computed from both by `keel_domain::stock::levels`, so it is
//! never stored.

use keel_domain::aggregate::fold;
use keel_domain::order::{Order, OrderEvent, OrderStatus};
use keel_domain::refs::Variant;
use keel_domain::schema::DomainEvent;
use keel_domain::stock::{Entry, EntryKind, Flow, Movement, Stock, time_of};
use keel_events::envelope;
use keel_events::event::SignedEvent;
use keel_types::{Id, Quantity, Timestamp, Unit};
use rusqlite::{Connection, Row, params};

use crate::error::StoreError;
use crate::projection::Projection;
use crate::schema::{hlc, hlc_bytes, id};

/// What each item's stock stream moved and counted.
pub(crate) const STOCK_ENTRIES: Projection = Projection {
    name: "stock_entries",
    version: 1,
    kind: "stock",
    key: "variant",
    order: "variant, event_id",
    create: "
        CREATE TABLE stock_entries (
            variant BLOB NOT NULL CHECK (length(variant) = 16),
            event_id BLOB NOT NULL CHECK (length(event_id) = 16),
            at INTEGER NOT NULL,
            hlc BLOB NOT NULL CHECK (length(hlc) = 8),
            movement INTEGER,
            micros INTEGER NOT NULL,
            unit TEXT NOT NULL,
            as_of INTEGER,
            PRIMARY KEY (variant, event_id)
        ) STRICT;
    ",
    drop: "DROP TABLE IF EXISTS stock_entries",
    project: project_stock,
};

/// What each order sold and returned.
pub(crate) const STOCK_FLOWS: Projection = Projection {
    name: "stock_flows",
    version: 1,
    kind: "order",
    key: "order_id",
    order: "order_id, seq",
    create: "
        CREATE TABLE stock_flows (
            order_id BLOB NOT NULL CHECK (length(order_id) = 16),
            seq INTEGER NOT NULL,
            variant BLOB NOT NULL CHECK (length(variant) = 16),
            at INTEGER NOT NULL,
            micros INTEGER NOT NULL,
            unit TEXT NOT NULL,
            PRIMARY KEY (order_id, seq)
        ) STRICT;
        CREATE INDEX stock_flows_by_variant ON stock_flows (variant, at);
    ",
    drop: "DROP TABLE IF EXISTS stock_flows",
    project: project_flows,
};

fn project_stock(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let variant = stream.to_bytes();
    db.execute("DELETE FROM stock_entries WHERE variant = ?1", [&variant[..]])?;
    let mut stock = Stock::new(stream.cast());
    for event in events {
        fold(&mut stock, event);
    }
    let mut insert = db.prepare(
        "INSERT INTO stock_entries (variant, event_id, at, hlc, movement, micros, unit, as_of) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for entry in stock.entries() {
        let (movement, quantity, as_of) = match entry.kind {
            EntryKind::Moved(movement, quantity) => (Some(movement), quantity, None),
            EntryKind::Counted { quantity, as_of } => (None, quantity, Some(as_of.as_micros())),
        };
        let movement = movement.map(|movement| i64::try_from(movement.code())).transpose();
        insert.execute(params![
            &variant[..],
            &entry.event.to_bytes()[..],
            entry.at.as_micros(),
            &hlc_bytes(entry.hlc)[..],
            movement.map_err(|_| StoreError::OutOfRange("a movement"))?,
            quantity.micros(),
            quantity.unit().code(),
            as_of,
        ])?;
    }
    Ok(())
}

/// What order `stream` sold and returned (ADR-0036, decision 2): a closed order's live lines, as
/// of its last close; each return's lines, as of the return. An order voided or abandoned, or not
/// closed, sells nothing.
fn project_flows(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let key = stream.to_bytes();
    db.execute("DELETE FROM stock_flows WHERE order_id = ?1", [&key[..]])?;
    let mut order = Order::new(stream.cast());
    let mut closed_at: Option<Timestamp> = None;
    let mut returns: Vec<(Timestamp, Vec<Returned>)> = Vec::new();
    for event in events {
        fold(&mut order, event);
        let body = event.body();
        let Ok(decoded) = OrderEvent::decode(&body.schema, &body.payload) else { continue };
        let Some(at) = time_of(body.hlc) else { continue };
        match decoded {
            OrderEvent::Closed => closed_at = Some(at),
            OrderEvent::LinesReturned(returned) => returns
                .push((at, returned.lines.iter().map(|each| (each.line, each.quantity)).collect())),
            _ => {}
        }
    }
    if *order.status() != OrderStatus::Closed {
        return Ok(());
    }
    let Some(closed_at) = closed_at else { return Ok(()) };
    let mut flows: Vec<(Id<Variant>, Timestamp, Quantity)> = Vec::new();
    for line in order.live_lines() {
        let sold =
            line.quantity().checked_neg().map_err(|_| StoreError::OutOfRange("a quantity"))?;
        flows.push((line.item().variant, closed_at, sold));
    }
    for (at, lines) in returns {
        for (line, quantity) in lines {
            if let Some(found) = order.line(line) {
                flows.push((found.item().variant, at, quantity));
            }
        }
    }
    let mut insert = db.prepare(
        "INSERT INTO stock_flows (order_id, seq, variant, at, micros, unit) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (seq, (variant, at, quantity)) in flows.into_iter().enumerate() {
        let seq = i64::try_from(seq).map_err(|_| StoreError::OutOfRange("a flow"))?;
        insert.execute(params![
            &key[..],
            seq,
            &variant.to_bytes()[..],
            at.as_micros(),
            quantity.micros(),
            quantity.unit().code(),
        ])?;
    }
    Ok(())
}

/// A line returned, and how much of it.
type Returned = (Id<keel_domain::order::Line>, Quantity);

fn time(micros: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_micros(micros).map_err(|_| StoreError::Corrupt("a stock time"))
}

fn quantity(micros: i64, unit: &str) -> Result<Quantity, StoreError> {
    let unit = Unit::from_code(unit).map_err(|_| StoreError::Corrupt("a stock unit"))?;
    Ok(Quantity::from_micros(micros, unit))
}

/// Reads an entry from a row of `event_id, at, hlc, movement, micros, unit, as_of`.
pub(crate) fn entry(row: &Row<'_>) -> rusqlite::Result<Result<Entry, StoreError>> {
    let event: Vec<u8> = row.get(0)?;
    let at: i64 = row.get(1)?;
    let stamp: Vec<u8> = row.get(2)?;
    let movement: Option<i64> = row.get(3)?;
    let micros: i64 = row.get(4)?;
    let unit: String = row.get(5)?;
    let as_of: Option<i64> = row.get(6)?;
    Ok((|| {
        let quantity = quantity(micros, &unit)?;
        let kind = match (movement, as_of) {
            (Some(code), None) => {
                let code = u64::try_from(code).map_err(|_| StoreError::Corrupt("a movement"))?;
                let movement =
                    Movement::from_code(code).ok_or(StoreError::Corrupt("a movement"))?;
                EntryKind::Moved(movement, quantity)
            }
            (None, Some(as_of)) => EntryKind::Counted { quantity, as_of: time(as_of)? },
            _ => return Err(StoreError::Corrupt("a stock entry")),
        };
        Ok(Entry { event: id(&event)?, at: time(at)?, hlc: hlc(&stamp)?, kind })
    })())
}

/// Reads a flow from a row of `at, micros, unit`.
pub(crate) fn flow(row: &Row<'_>) -> rusqlite::Result<Result<Flow, StoreError>> {
    let at: i64 = row.get(0)?;
    let micros: i64 = row.get(1)?;
    let unit: String = row.get(2)?;
    Ok((|| Ok(Flow { at: time(at)?, delta: quantity(micros, &unit)? }))())
}
