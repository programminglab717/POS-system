//! Projections: what the store derives from events, stream by stream (ADR-0017).
//!
//! A projection follows one kind of stream. Its rows for a stream are computed from the stream's
//! events alone: the store folds them in canonical order with the aggregate's fold, and maps the
//! state to columns. So replicas holding the same events hold the same rows, byte for byte,
//! whatever order the events arrived in. Orders and payments have a row per stream; sequencing
//! records a row per run (ADR-0020).
//!
//! Each write recomputes the rows of the streams it touched, before it commits. A projection
//! whose version the store didn't build is dropped and rebuilt from every stored stream of its
//! kind. Bump a projection's version whenever its columns change, or the fold it uses changes what
//! it computes: the golden tests pin each projection's rows for a fixed set of events, to catch
//! such a change.

use core::str::FromStr;

use keel_domain::aggregate::{Aggregate, fold};
use keel_domain::order::{Channel, Mode, Order, OrderInfo, OrderStatus, Stage};
use keel_domain::payment::{Payment, PaymentInfo, PaymentStatus, Tender};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{self, SequenceEvent};
use keel_events::envelope::{self, Event, StreamRef};
use keel_events::event::SignedEvent;
use keel_types::{BusinessDate, Currency, Hlc, Id, Money};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::StoreError;
use crate::rows::{self, seq_value};
use crate::schema::{hlc, hlc_bytes, id};

/// Replaces the rows of stream `id` in a projection, given the stream's events in canonical
/// order.
type Project = fn(&Connection, Id<envelope::Aggregate>, &[SignedEvent]) -> Result<(), StoreError>;

/// A projection: a table with rows for each stream of a kind.
pub(crate) struct Projection {
    /// Its table.
    pub(crate) name: &'static str,
    /// Its version. A store that built another version drops the table and rebuilds it.
    pub(crate) version: i64,
    /// The kind of stream it follows.
    pub(crate) kind: &'static str,
    /// The column that holds each row's stream identifier.
    pub(crate) key: &'static str,
    /// The columns that order a stream's rows: with `key`, the table's primary key.
    pub(crate) order: &'static str,
    /// Creates its table.
    create: &'static str,
    /// Drops its table.
    drop: &'static str,
    /// Replaces the row of a stream.
    project: Project,
}

/// Every projection.
pub(crate) const ALL: [Projection; 3] = [ORDERS, PAYMENTS, SEQUENCE];

impl Projection {
    /// Replaces the rows of stream `id`, given its events in canonical order.
    pub(crate) fn project(
        &self,
        db: &Connection,
        id: Id<envelope::Aggregate>,
        events: &[SignedEvent],
    ) -> Result<(), StoreError> {
        (self.project)(db, id, events)
    }
}

/// Recomputes the rows of `streams`: those a write touched.
pub(crate) fn update(db: &Connection, streams: &[StreamRef]) -> Result<(), StoreError> {
    for stream in streams {
        if let Some(projection) =
            ALL.iter().find(|projection| projection.kind == stream.kind.as_str())
        {
            let events = rows::stream_events(db, projection.kind, stream.id)?;
            projection.project(db, stream.id, &events)?;
        }
    }
    Ok(())
}

/// The projections whose version the store didn't build.
pub(crate) fn stale(db: &Connection) -> Result<Vec<&'static Projection>, StoreError> {
    let mut stale = Vec::new();
    for projection in &ALL {
        let built: Option<i64> = db
            .query_row(
                "SELECT version FROM projections WHERE name = ?1",
                [projection.name],
                |row| row.get(0),
            )
            .optional()?;
        if built != Some(projection.version) {
            stale.push(projection);
        }
    }
    Ok(stale)
}

/// Drops `projection`'s table and rebuilds it from every stored stream of its kind.
pub(crate) fn rebuild(db: &Connection, projection: &Projection) -> Result<(), StoreError> {
    db.execute_batch(projection.drop)?;
    db.execute_batch(projection.create)?;
    let streams: Vec<Vec<u8>> = {
        let mut statement = db.prepare(
            "SELECT DISTINCT stream_id FROM events WHERE stream_kind = ?1 ORDER BY stream_id",
        )?;
        let rows = statement.query_map([projection.kind], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for stream in streams {
        let stream = id(&stream)?;
        let events = rows::stream_events(db, projection.kind, stream)?;
        projection.project(db, stream, &events)?;
    }
    db.execute(
        "INSERT INTO projections (name, version) VALUES (?1, ?2) \
         ON CONFLICT (name) DO UPDATE SET version = excluded.version",
        params![projection.name, projection.version],
    )?;
    Ok(())
}

/// What every row records of its stream's events: how many, and the first and last HLCs in
/// canonical order.
struct Span {
    events: i64,
    first: [u8; 8],
    last: [u8; 8],
}

impl Span {
    fn of(events: &[SignedEvent]) -> Result<Span, StoreError> {
        let hlc_at =
            |event: Option<&SignedEvent>| hlc_bytes(event.map_or(Hlc::ZERO, |e| e.body().hlc));
        Ok(Span {
            events: count(events.len())?,
            first: hlc_at(events.first()),
            last: hlc_at(events.last()),
        })
    }
}

fn count(n: usize) -> Result<i64, StoreError> {
    i64::try_from(n).map_err(|_| StoreError::OutOfRange("a count"))
}

/// A stable code, such as a channel's, as stored.
fn code(code: Option<u64>) -> Result<Option<i64>, StoreError> {
    code.map(|code| i64::try_from(code).map_err(|_| StoreError::OutOfRange("a code"))).transpose()
}

/// The business date of the event `event`, one of `events`.
fn business_date(events: &[SignedEvent], event: Id<Event>) -> Option<String> {
    events
        .iter()
        .find(|stored| stored.body().event_id == event)
        .map(|stored| stored.body().business_date.to_string())
}

fn bytes<T>(id: Option<Id<T>>) -> Option<Vec<u8>> {
    id.map(|id| id.to_bytes().to_vec())
}

// ---------------------------------------------------------------------------------------------
// Orders.

const ORDERS: Projection = Projection {
    name: "orders",
    version: 1,
    kind: "order",
    key: "order_id",
    order: "order_id",
    create: "
        CREATE TABLE orders (
            order_id BLOB PRIMARY KEY CHECK (length(order_id) = 16),
            state TEXT NOT NULL
                CHECK (state IN ('uncreated', 'active', 'closed', 'voided', 'abandoned')),
            stage TEXT NOT NULL CHECK (stage IN ('draft', 'open', 'submitted')),
            location BLOB,
            channel INTEGER,
            mode INTEGER,
            currency TEXT,
            created_by BLOB,
            revenue_center BLOB,
            table_id BLOB,
            guest_count INTEGER,
            customer BLOB,
            owner BLOB,
            business_date TEXT,
            live_lines INTEGER NOT NULL,
            open_checks INTEGER NOT NULL,
            closed_checks INTEGER NOT NULL,
            conflicts INTEGER NOT NULL,
            unreadable INTEGER NOT NULL,
            events INTEGER NOT NULL,
            first_hlc BLOB NOT NULL CHECK (length(first_hlc) = 8),
            last_hlc BLOB NOT NULL CHECK (length(last_hlc) = 8)
        ) STRICT;
        CREATE INDEX orders_by_state ON orders (state, first_hlc, order_id);
    ",
    drop: "DROP TABLE IF EXISTS orders",
    project: project_order,
};

/// Where an order is in its life, as the `orders` projection keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OrderState {
    /// The store holds events of the order, but not the one that created it.
    Uncreated,
    /// Open for changes.
    Active,
    /// Closed.
    Closed,
    /// Voided as a whole.
    Voided,
    /// Dropped before anything in it was fired.
    Abandoned,
}

impl OrderState {
    /// Every state.
    pub const ALL: [OrderState; 5] = [
        OrderState::Uncreated,
        OrderState::Active,
        OrderState::Closed,
        OrderState::Voided,
        OrderState::Abandoned,
    ];

    /// The state of `order`.
    pub const fn of(order: &Order) -> OrderState {
        if order.info().is_none() {
            return OrderState::Uncreated;
        }
        match order.status() {
            OrderStatus::Active => OrderState::Active,
            OrderStatus::Closed => OrderState::Closed,
            OrderStatus::Voided(_) => OrderState::Voided,
            OrderStatus::Abandoned => OrderState::Abandoned,
        }
    }

    pub(crate) const fn code(self) -> &'static str {
        match self {
            OrderState::Uncreated => "uncreated",
            OrderState::Active => "active",
            OrderState::Closed => "closed",
            OrderState::Voided => "voided",
            OrderState::Abandoned => "abandoned",
        }
    }

    fn from_code(code: &str) -> Option<OrderState> {
        OrderState::ALL.into_iter().find(|state| state.code() == code)
    }
}

const fn stage_code(stage: Stage) -> &'static str {
    match stage {
        Stage::Draft => "draft",
        Stage::Open => "open",
        Stage::Submitted => "submitted",
    }
}

fn stage_from_code(code: &str) -> Option<Stage> {
    [Stage::Draft, Stage::Open, Stage::Submitted]
        .into_iter()
        .find(|stage| stage_code(*stage) == code)
}

/// An order, as the `orders` projection keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderSummary {
    /// The order.
    pub id: Id<Order>,
    /// Where it is in its life.
    pub state: OrderState,
    /// What its lines say of it.
    pub stage: Stage,
    /// What it was created with, and its attributes since; `None` until it is created.
    pub info: Option<OrderInfo>,
    /// The business date of the event that created it.
    pub business_date: Option<BusinessDate>,
    /// Its live lines: neither removed nor voided.
    pub live_lines: u64,
    /// Its open checks.
    pub open_checks: u64,
    /// Its closed checks.
    pub closed_checks: u64,
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

fn project_order(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let mut order = Order::new(stream.cast());
    for event in events {
        fold(&mut order, event);
    }
    let span = Span::of(events)?;
    let info = order.info();
    let checks = order.checks();
    let closed = checks.iter().filter(|check| check.closed().is_some()).count();
    db.execute(
        "INSERT OR REPLACE INTO orders (order_id, state, stage, location, channel, mode, currency, \
         created_by, revenue_center, table_id, guest_count, customer, owner, business_date, \
         live_lines, open_checks, closed_checks, conflicts, unreadable, events, first_hlc, \
         last_hlc) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
         ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![
            &order.id().to_bytes()[..],
            OrderState::of(&order).code(),
            stage_code(order.stage()),
            bytes(info.map(|info| info.location)),
            code(info.map(|info| info.channel.code()))?,
            code(info.map(|info| info.mode.code()))?,
            info.map(|info| info.currency.code()),
            bytes(info.map(|info| info.created_by)),
            bytes(info.and_then(|info| info.revenue_center)),
            bytes(info.and_then(|info| info.table)),
            info.and_then(|info| info.guest_count).map(core::num::NonZeroU16::get),
            bytes(info.and_then(|info| info.customer)),
            bytes(info.and_then(|info| info.owner)),
            info.and_then(|info| business_date(events, info.created_by)),
            count(order.live_lines().count())?,
            count(checks.len().saturating_sub(closed))?,
            count(closed)?,
            count(order.conflicts().len())?,
            count(order.skipped().len())?,
            span.events,
            &span.first[..],
            &span.last[..],
        ],
    )?;
    Ok(())
}

/// The columns an order summary is read from, in order.
pub(crate) const ORDER_COLUMNS: &str = "order_id, state, stage, location, channel, mode, \
    currency, created_by, revenue_center, table_id, guest_count, customer, owner, \
    business_date, live_lines, open_checks, closed_checks, conflicts, unreadable, events, \
    first_hlc, last_hlc";

/// Reads an order summary from a row of [`ORDER_COLUMNS`].
pub(crate) fn order_summary(row: &Row<'_>) -> rusqlite::Result<Result<OrderSummary, StoreError>> {
    let values: Vec<Value> = (0..22).map(|column| row.get(column)).collect::<Result<_, _>>()?;
    Ok(read_order(&values))
}

fn read_order(values: &[Value]) -> Result<OrderSummary, StoreError> {
    let at = Columns(values);
    let state =
        OrderState::from_code(&at.text(1)?).ok_or(StoreError::Corrupt("an order's state"))?;
    let stage = stage_from_code(&at.text(2)?).ok_or(StoreError::Corrupt("an order's stage"))?;
    let info = match at.optional_blob(3)? {
        None => None,
        Some(location) => Some(OrderInfo {
            location: id(&location)?,
            channel: Channel::from_code(at.unsigned(4)?)
                .ok_or(StoreError::Corrupt("an order's channel"))?,
            currency: currency(&at.text(6)?)?,
            created_by: id(&at.blob(7)?)?,
            mode: Mode::from_code(at.unsigned(5)?).ok_or(StoreError::Corrupt("an order's mode"))?,
            revenue_center: at.optional_id(8)?,
            table: at.optional_id(9)?,
            guest_count: at
                .optional_integer(10)?
                .map(|count| {
                    u16::try_from(count)
                        .ok()
                        .and_then(core::num::NonZeroU16::new)
                        .ok_or(StoreError::Corrupt("a guest count"))
                })
                .transpose()?,
            customer: at.optional_id(11)?,
            owner: at.optional_id(12)?,
        }),
    };
    Ok(OrderSummary {
        id: id(&at.blob(0)?)?,
        state,
        stage,
        info,
        business_date: at.optional_date(13)?,
        live_lines: at.unsigned(14)?,
        open_checks: at.unsigned(15)?,
        closed_checks: at.unsigned(16)?,
        conflicts: at.unsigned(17)?,
        unreadable: at.unsigned(18)?,
        events: at.unsigned(19)?,
        first: hlc(&at.blob(20)?)?,
        last: hlc(&at.blob(21)?)?,
    })
}

// ---------------------------------------------------------------------------------------------
// Payments.

const PAYMENTS: Projection = Projection {
    name: "payments",
    version: 1,
    kind: "payment",
    key: "payment_id",
    order: "payment_id",
    create: "
        CREATE TABLE payments (
            payment_id BLOB PRIMARY KEY CHECK (length(payment_id) = 16),
            state TEXT NOT NULL CHECK (state IN ('uninitiated', 'initiated', 'authorized', \
                'captured', 'failed', 'voided')),
            location BLOB,
            order_id BLOB,
            check_id BLOB,
            tender INTEGER,
            amount INTEGER,
            currency TEXT,
            initiated_by BLOB,
            business_date TEXT,
            captured INTEGER,
            tip INTEGER,
            conflicts INTEGER NOT NULL,
            unreadable INTEGER NOT NULL,
            events INTEGER NOT NULL,
            first_hlc BLOB NOT NULL CHECK (length(first_hlc) = 8),
            last_hlc BLOB NOT NULL CHECK (length(last_hlc) = 8)
        ) STRICT;
        CREATE INDEX payments_by_order ON payments (order_id, first_hlc, payment_id);
    ",
    drop: "DROP TABLE IF EXISTS payments",
    project: project_payment,
};

/// Where a payment is in its life, as the `payments` projection keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PaymentState {
    /// The store holds events of the payment, but not the one that initiated it.
    Uninitiated,
    /// Started, with no outcome yet.
    Initiated,
    /// The card is held for an amount, not yet charged.
    Authorized,
    /// The money moved.
    Captured,
    /// The attempt failed: no money moved.
    Failed,
    /// Voided before it was captured: no money moved.
    Voided,
}

impl PaymentState {
    /// Every state.
    pub const ALL: [PaymentState; 6] = [
        PaymentState::Uninitiated,
        PaymentState::Initiated,
        PaymentState::Authorized,
        PaymentState::Captured,
        PaymentState::Failed,
        PaymentState::Voided,
    ];

    /// The state of `payment`.
    pub const fn of(payment: &Payment) -> PaymentState {
        if payment.info().is_none() {
            return PaymentState::Uninitiated;
        }
        match payment.status() {
            PaymentStatus::Initiated => PaymentState::Initiated,
            PaymentStatus::Authorized(_) => PaymentState::Authorized,
            PaymentStatus::Captured(_) => PaymentState::Captured,
            PaymentStatus::Failed(_) => PaymentState::Failed,
            PaymentStatus::Voided(_) => PaymentState::Voided,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            PaymentState::Uninitiated => "uninitiated",
            PaymentState::Initiated => "initiated",
            PaymentState::Authorized => "authorized",
            PaymentState::Captured => "captured",
            PaymentState::Failed => "failed",
            PaymentState::Voided => "voided",
        }
    }

    fn from_code(code: &str) -> Option<PaymentState> {
        PaymentState::ALL.into_iter().find(|state| state.code() == code)
    }
}

/// A payment, as the `payments` projection keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentSummary {
    /// The payment.
    pub id: Id<Payment>,
    /// Where it is in its life.
    pub state: PaymentState,
    /// What it was initiated with; `None` until it is initiated.
    pub info: Option<PaymentInfo>,
    /// The business date of the event that initiated it.
    pub business_date: Option<BusinessDate>,
    /// Once captured: the amount applied to the check.
    pub captured: Option<Money>,
    /// Once captured: the tip on top, if any.
    pub tip: Option<Money>,
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

fn project_payment(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let mut payment = Payment::new(stream.cast());
    for event in events {
        fold(&mut payment, event);
    }
    let span = Span::of(events)?;
    let info = payment.info();
    let captured = payment.captured();
    db.execute(
        "INSERT OR REPLACE INTO payments (payment_id, state, location, order_id, check_id, tender, \
         amount, currency, initiated_by, business_date, captured, tip, conflicts, unreadable, \
         events, first_hlc, last_hlc) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, \
         ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            &payment.id().to_bytes()[..],
            PaymentState::of(&payment).code(),
            bytes(info.map(|info| info.location)),
            bytes(info.map(|info| info.order)),
            bytes(info.map(|info| info.check)),
            code(info.map(|info| info.tender.code()))?,
            info.map(|info| info.amount.minor()),
            info.map(|info| info.amount.currency().code()),
            bytes(info.map(|info| info.initiated_by)),
            info.and_then(|info| business_date(events, info.initiated_by)),
            captured.map(|captured| captured.amount.minor()),
            captured.and_then(|captured| captured.tip).map(Money::minor),
            count(payment.conflicts().len())?,
            count(payment.skipped().len())?,
            span.events,
            &span.first[..],
            &span.last[..],
        ],
    )?;
    Ok(())
}

/// The columns a payment summary is read from, in order.
pub(crate) const PAYMENT_COLUMNS: &str = "payment_id, state, location, order_id, check_id, \
    tender, amount, currency, initiated_by, business_date, captured, tip, conflicts, unreadable, \
    events, first_hlc, last_hlc";

/// Reads a payment summary from a row of [`PAYMENT_COLUMNS`].
pub(crate) fn payment_summary(
    row: &Row<'_>,
) -> rusqlite::Result<Result<PaymentSummary, StoreError>> {
    let values: Vec<Value> = (0..17).map(|column| row.get(column)).collect::<Result<_, _>>()?;
    Ok(read_payment(&values))
}

fn read_payment(values: &[Value]) -> Result<PaymentSummary, StoreError> {
    let at = Columns(values);
    let state =
        PaymentState::from_code(&at.text(1)?).ok_or(StoreError::Corrupt("a payment's state"))?;
    let info = match at.optional_blob(2)? {
        None => None,
        Some(location) => {
            let currency = currency(&at.text(7)?)?;
            Some(PaymentInfo {
                location: id(&location)?,
                order: id(&at.blob(3)?)?,
                check: id(&at.blob(4)?)?,
                tender: Tender::from_code(at.unsigned(5)?)
                    .ok_or(StoreError::Corrupt("a payment's tender"))?,
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
            .ok_or(StoreError::Corrupt("a capture without a payment"))?;
        Ok(Some(Money::from_minor(minor, currency)))
    };
    Ok(PaymentSummary {
        id: id(&at.blob(0)?)?,
        state,
        business_date: at.optional_date(9)?,
        captured: money(10)?,
        tip: money(11)?,
        info,
        conflicts: at.unsigned(12)?,
        unreadable: at.unsigned(13)?,
        events: at.unsigned(14)?,
        first: hlc(&at.blob(15)?)?,
        last: hlc(&at.blob(16)?)?,
    })
}

// ---------------------------------------------------------------------------------------------
// Reading columns.

/// A row's values, read by column.
struct Columns<'a>(&'a [Value]);

impl Columns<'_> {
    fn value(&self, column: usize) -> Result<&Value, StoreError> {
        self.0.get(column).ok_or(StoreError::Corrupt("a missing column"))
    }

    fn optional_blob(&self, column: usize) -> Result<Option<Vec<u8>>, StoreError> {
        match self.value(column)? {
            Value::Null => Ok(None),
            Value::Blob(bytes) => Ok(Some(bytes.clone())),
            _ => Err(StoreError::Corrupt("a column that should be bytes")),
        }
    }

    fn blob(&self, column: usize) -> Result<Vec<u8>, StoreError> {
        self.optional_blob(column)?.ok_or(StoreError::Corrupt("a missing value"))
    }

    fn optional_id<T>(&self, column: usize) -> Result<Option<Id<T>>, StoreError> {
        self.optional_blob(column)?.map(|bytes| id(&bytes)).transpose()
    }

    fn optional_integer(&self, column: usize) -> Result<Option<i64>, StoreError> {
        match self.value(column)? {
            Value::Null => Ok(None),
            Value::Integer(value) => Ok(Some(*value)),
            _ => Err(StoreError::Corrupt("a column that should be an integer")),
        }
    }

    fn integer(&self, column: usize) -> Result<i64, StoreError> {
        self.optional_integer(column)?.ok_or(StoreError::Corrupt("a missing value"))
    }

    fn unsigned(&self, column: usize) -> Result<u64, StoreError> {
        u64::try_from(self.integer(column)?).map_err(|_| StoreError::Corrupt("a negative count"))
    }

    fn text(&self, column: usize) -> Result<String, StoreError> {
        match self.value(column)? {
            Value::Text(text) => Ok(text.clone()),
            _ => Err(StoreError::Corrupt("a column that should be text")),
        }
    }

    fn optional_date(&self, column: usize) -> Result<Option<BusinessDate>, StoreError> {
        match self.value(column)? {
            Value::Null => Ok(None),
            Value::Text(text) => BusinessDate::from_str(text)
                .map(Some)
                .map_err(|_| StoreError::Corrupt("a business date")),
            _ => Err(StoreError::Corrupt("a column that should be a date")),
        }
    }
}

fn currency(code: &str) -> Result<Currency, StoreError> {
    Currency::from_code(code).map_err(|_| StoreError::Corrupt("a currency"))
}

/// Loads `aggregate` from its stream's events in canonical order.
pub(crate) fn load<A: Aggregate>(db: &Connection, mut aggregate: A) -> Result<A, StoreError> {
    use keel_domain::schema::DomainEvent;
    let events = rows::stream_events(db, A::Event::STREAM, aggregate.stream_id())?;
    for event in &events {
        fold(&mut aggregate, event);
    }
    Ok(aggregate)
}

// ---------------------------------------------------------------------------------------------
// Sequencing records.

/// A row for each run of each sequencing record: which record it is in and where, who wrote the
/// record and where in their log, and the numbers it gives the stretch of a device's log it
/// covers (ADR-0020). A row a run, not an event, since a record may claim a run of any length.
/// Each run's length is indexed, so that a search for the runs covering an event need look no
/// further than the longest run of its device. Whether the run confirms anything depends on the
/// events the store holds, so it is worked out as the store is read, not kept
/// ([`crate::sequencing`]).
const SEQUENCE: Projection = Projection {
    name: "sequence",
    version: 1,
    kind: sequence::STREAM,
    key: "record",
    order: "record, run",
    create: "
        CREATE TABLE sequence (
            record BLOB NOT NULL CHECK (length(record) = 16),
            run INTEGER NOT NULL CHECK (run >= 0),
            author BLOB NOT NULL CHECK (length(author) = 16),
            author_seq INTEGER NOT NULL CHECK (author_seq >= 1),
            epoch INTEGER NOT NULL CHECK (epoch >= 1),
            number INTEGER NOT NULL CHECK (number >= 1),
            device BLOB NOT NULL CHECK (length(device) = 16),
            from_seq INTEGER NOT NULL CHECK (from_seq >= 1),
            to_seq INTEGER NOT NULL CHECK (to_seq >= from_seq),
            last_hash BLOB NOT NULL CHECK (length(last_hash) = 32),
            PRIMARY KEY (record, run)
        ) STRICT;
        CREATE INDEX sequence_by_event ON sequence (device, to_seq);
        CREATE INDEX sequence_by_number ON sequence (epoch, number);
        CREATE INDEX sequence_by_author ON sequence (author, epoch, number);
        CREATE INDEX sequence_by_record ON sequence (author, author_seq);
        CREATE INDEX sequence_by_run_length ON sequence (device, to_seq - from_seq);
        CREATE INDEX sequence_by_epoch_run_length ON sequence (epoch, to_seq - from_seq);
    ",
    drop: "DROP TABLE IF EXISTS sequence",
    project: project_sequence,
};

/// The runs of the record `stream`, whose first event in canonical order is the record: a record
/// is a stream of one event. A record this kernel can't read numbers nothing.
fn project_sequence(
    db: &Connection,
    stream: Id<envelope::Aggregate>,
    events: &[SignedEvent],
) -> Result<(), StoreError> {
    let record_id = stream.to_bytes();
    db.execute("DELETE FROM sequence WHERE record = ?1", [&record_id[..]])?;
    let Some(first) = events.first() else { return Ok(()) };
    let body = first.body();
    let Ok(SequenceEvent::Assigned(record)) = SequenceEvent::decode(&body.schema, &body.payload)
    else {
        return Ok(());
    };
    let mut statement = db.prepare(
        "INSERT INTO sequence (record, run, author, author_seq, epoch, number, device, from_seq, \
         to_seq, last_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for (index, (number, run)) in record.numbered().enumerate() {
        statement.execute(params![
            &record_id[..],
            count(index)?,
            &body.origin_device.to_bytes()[..],
            seq_value(body.origin_seq.get())?,
            seq_value(record.epoch)?,
            seq_value(number)?,
            &run.device.to_bytes()[..],
            seq_value(run.from)?,
            seq_value(run.to)?,
            &run.last.as_bytes()[..],
        ])?;
    }
    Ok(())
}
