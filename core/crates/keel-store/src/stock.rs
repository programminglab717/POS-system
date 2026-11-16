//! The stock projections (ADR-0036): what each item's stock stream moved and counted, one row
//! per entry that applied; and what each order sold and returned, one row per line, as of its
//! close or its return. A level is computed from both by `keel_domain::stock::levels`, so it is
//! never stored.

use keel_domain::aggregate::fold;
use keel_domain::order::{Line, Order, OrderEvent, OrderStatus};
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

/// An item's stock levels, the unit they are in, and what was left out of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockLevels {
    /// The unit: its first stock event's.
    pub unit: Unit,
    /// On hand, reserved and available.
    pub levels: keel_domain::stock::Levels,
    /// What a person should look at: events of its stock that didn't apply, as one in another
    /// unit or at another location, or a release below zero; and sales or returns in another
    /// unit, which count for nothing.
    pub conflicts: u64,
}

/// What each item's stock stream moved and counted.
pub(crate) const STOCK_ENTRIES: Projection = Projection {
    name: "stock_entries",
    // 2: each stream's conflicts, found in review.
    version: 2,
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
        CREATE TABLE stock_conflicts (
            variant BLOB PRIMARY KEY CHECK (length(variant) = 16),
            conflicts INTEGER NOT NULL
        ) STRICT;
    ",
    drop: "DROP TABLE IF EXISTS stock_entries; DROP TABLE IF EXISTS stock_conflicts",
    project: project_stock,
};

/// What each order sold and returned.
pub(crate) const STOCK_FLOWS: Projection = Projection {
    name: "stock_flows",
    // 2: what the fold applied, each close taking what it adds: found in review.
    version: 2,
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
    db.execute("DELETE FROM stock_conflicts WHERE variant = ?1", [&variant[..]])?;
    let mut stock = Stock::new(stream.cast());
    for event in events {
        fold(&mut stock, event);
    }
    // What a person should look at, as an event in another unit, which applied nothing: found
    // in review, nothing said so.
    let conflicts = stock.conflicts().len().checked_add(stock.skipped().len());
    let conflicts =
        conflicts.and_then(|n| i64::try_from(n).ok()).ok_or(StoreError::OutOfRange("conflicts"))?;
    if conflicts > 0 {
        db.execute(
            "INSERT INTO stock_conflicts (variant, conflicts) VALUES (?1, ?2)",
            params![&variant[..], conflicts],
        )?;
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

/// What order `stream` sold and returned (ADR-0036, decision 2), from what its fold applied:
/// each close takes what its live lines came to beyond what earlier closes took, as of that
/// close, and gives back what they no longer come to; a void or an abandonment after a close
/// gives back everything the closes took, as of it; each return puts back what it returned, as
/// of the return. An order reopened keeps what it took until it is closed again or voided.
/// Found in review: an order reopened and closed again took all its lines a second time after a
/// count, and an event the fold didn't apply, such as a second return with one refund, moved
/// stock all the same.
fn project_flows(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let key = stream.to_bytes();
    db.execute("DELETE FROM stock_flows WHERE order_id = ?1", [&key[..]])?;
    let mut order = Order::new(stream.cast());
    // What the closes took so far, by variant and unit.
    let mut taken: Taken = Vec::new();
    let mut flows: Vec<(Id<Variant>, Timestamp, Quantity)> = Vec::new();
    for event in events {
        let was_closed = *order.status() == OrderStatus::Closed;
        let was_ended = ended(order.status());
        let body = event.body();
        let returning = matches!(
            OrderEvent::decode(&body.schema, &body.payload),
            Ok(OrderEvent::LinesReturned(_))
        );
        let returned_before = if returning { returned(&order) } else { Vec::new() };
        fold(&mut order, event);
        let Some(at) = time_of(body.hlc) else { continue };
        if !was_closed && *order.status() == OrderStatus::Closed {
            let live = sold(&order)?;
            for &(variant, before) in &taken {
                let now =
                    live.iter().find(|(known, q)| *known == variant && q.unit() == before.unit());
                let now = now.map_or(Quantity::zero(before.unit()), |(_, q)| *q);
                let back =
                    before.checked_sub(now).map_err(|_| StoreError::OutOfRange("a quantity"))?;
                if !back.is_zero() {
                    flows.push((variant, at, back));
                }
            }
            for &(variant, now) in &live {
                let before =
                    taken.iter().find(|(known, q)| *known == variant && q.unit() == now.unit());
                if before.is_none() {
                    let sold =
                        now.checked_neg().map_err(|_| StoreError::OutOfRange("a quantity"))?;
                    flows.push((variant, at, sold));
                }
            }
            taken = live;
        }
        if !was_ended && ended(order.status()) {
            flows.extend(taken.drain(..).map(|(variant, quantity)| (variant, at, quantity)));
        }
        if returning {
            for (line, before) in returned_before {
                let Some(found) = order.line(line) else { continue };
                let Some(after) = found.returned().map(|returned| returned.quantity) else {
                    continue;
                };
                let more = match before {
                    Some(before) => after.checked_sub(before),
                    None => Ok(after),
                };
                let more = more.map_err(|_| StoreError::OutOfRange("a quantity"))?;
                if more.is_positive() {
                    flows.push((found.item().variant, at, more));
                }
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

/// What an order's closes took, by variant and unit.
type Taken = Vec<(Id<Variant>, Quantity)>;

/// Whether an order is voided or abandoned: it takes nothing.
const fn ended(status: &OrderStatus) -> bool {
    matches!(status, OrderStatus::Voided(_) | OrderStatus::Abandoned)
}

/// What `order`'s live lines come to, by variant and unit.
fn sold(order: &Order) -> Result<Taken, StoreError> {
    let mut sold: Taken = Vec::new();
    for line in order.live_lines() {
        let (variant, quantity) = (line.item().variant, line.quantity());
        match sold.iter_mut().find(|(known, q)| *known == variant && q.unit() == quantity.unit()) {
            Some((_, total)) => {
                *total = total
                    .checked_add(quantity)
                    .map_err(|_| StoreError::OutOfRange("a quantity"))?;
            }
            None => sold.push((variant, quantity)),
        }
    }
    Ok(sold)
}

/// How much of each of `order`'s lines was returned.
fn returned(order: &Order) -> Vec<(Id<Line>, Option<Quantity>)> {
    order
        .lines()
        .iter()
        .map(|line| (line.id(), line.returned().map(|returned| returned.quantity)))
        .collect()
}

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
