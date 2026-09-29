//! The store: opening it, writing to it, and reading it back.

use core::time::Duration;
use std::collections::BTreeMap;
use std::path::Path;

use keel_domain::aggregate::Aggregate;
use keel_domain::order::Order;
use keel_domain::payment::Payment;
use keel_events::envelope::{Device, Event, Location, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::Signer;
use keel_events::log::{LogConfig, LogHead, LogWriter};
use keel_types::{Entropy, Hlc, Id, Timestamp};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::error::StoreError;
use crate::faults::{Faults, NoFaults, Point, proceed};
use crate::outbox::{self, EffectState, Queued};
use crate::projection::{
    self, ORDER_COLUMNS, OrderState, OrderSummary, PAYMENT_COLUMNS, PaymentSummary, Projection,
};
use crate::rows;
use crate::schema;
use crate::write::{Quarantined, Reason, Writing};

/// Whose store it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreConfig {
    /// The device the store belongs to: whose log it writes.
    pub device: Id<Device>,
    /// The location the device is enrolled at. The store keeps that location's events only.
    pub location: Id<Location>,
    /// How far ahead of physical time a received HLC may move the device's clock.
    pub max_forward_drift: Duration,
}

/// A device's store: its own log, the events it received from other replicas, and the
/// quarantine, in one SQLite database.
pub struct Store<S, E> {
    db: Connection,
    location: Id<Location>,
    writer: LogWriter<S, E>,
    faults: Box<dyn Faults + Send>,
}

impl<S, E> core::fmt::Debug for Store<S, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Store")
            .field("location", &self.location)
            .field("writer", &self.writer)
            .finish_non_exhaustive()
    }
}

impl<S: Signer, E: Entropy> Store<S, E> {
    /// Opens the store at `path`, creating it for `config`'s device if it doesn't exist. The
    /// device's log writer signs with `signer` and draws identifiers from `entropy`, and resumes
    /// from the stored log and clock.
    ///
    /// # Errors
    /// [`StoreError::Settings`] if SQLite won't run with the durability settings,
    /// [`StoreError::NewerSchema`] if a newer kernel wrote the database,
    /// [`StoreError::NotThisDevice`] if it belongs to another device or location,
    /// [`StoreError::WrongKey`] if `signer` didn't sign the device's stored events, and
    /// [`StoreError::Database`] if the file can't be opened, read or written.
    pub fn open(
        path: impl AsRef<Path>,
        config: StoreConfig,
        signer: S,
        entropy: E,
    ) -> Result<Store<S, E>, StoreError> {
        Store::open_with_faults(path, config, signer, entropy, Box::new(NoFaults))
    }

    /// As [`Store::open`], with `faults` deciding whether each write goes on at each
    /// [`Point`]: for crash-safety tests, and the simulator.
    ///
    /// # Errors
    /// As [`Store::open`], and [`StoreError::Interrupted`] if `faults` interrupts creating the
    /// database's schema.
    pub fn open_with_faults(
        path: impl AsRef<Path>,
        config: StoreConfig,
        signer: S,
        entropy: E,
        mut faults: Box<dyn Faults + Send>,
    ) -> Result<Store<S, E>, StoreError> {
        let mut db = Connection::open(path)?;
        schema::configure(&db)?;
        schema::migrate(&mut db, config.device, config.location, &mut *faults)?;
        let identity = schema::identity(&db)?;
        if identity.device != config.device || identity.location != config.location {
            return Err(StoreError::NotThisDevice);
        }
        let head = rows::head(&db, config.device)?;
        if let Some(last) = event_at(&db, config.device, head.seq())?
            && last.key_id() != signer.public_key().key_id()
        {
            return Err(StoreError::WrongKey);
        }
        let log = LogConfig {
            device: config.device,
            location: config.location,
            head,
            latest_hlc: identity.clock,
            max_forward_drift: config.max_forward_drift,
        };
        let writer = LogWriter::new(log, signer, entropy);
        let mut store = Store { db, location: config.location, writer, faults };
        let stale = projection::stale(&store.db)?;
        if !stale.is_empty() {
            store.rebuild(&stale)?;
        }
        Ok(store)
    }

    /// Drops every projection and rebuilds it from the stored events, in one transaction. The
    /// store does this itself when it opens, for a projection whose version changed: this is for
    /// support, and tests.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the store can't be read or written, [`StoreError::Corrupt`]
    /// if an event doesn't read back as stored, and [`StoreError::Interrupted`] if a fault hook
    /// interrupts the rebuild, which then changes nothing.
    pub fn rebuild_projections(&mut self) -> Result<(), StoreError> {
        let all: Vec<&Projection> = projection::ALL.iter().collect();
        self.rebuild(&all)
    }

    fn rebuild(&mut self, projections: &[&Projection]) -> Result<(), StoreError> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for projection in projections {
            projection::rebuild(&tx, projection)?;
        }
        proceed(&mut *self.faults, Point::Rebuilding)?;
        tx.commit()?;
        Ok(())
    }

    /// Runs `f` as one write: the events it appends and receives are stored together, or none
    /// is. If the write fails, the device's log writer goes back to the log and clock as stored,
    /// so it is never ahead of them. The events `f` appends leave the store only when this
    /// returns `Ok`: don't send them anywhere before.
    ///
    /// # Errors
    /// `f`'s error, or a [`StoreError`] if the write can't begin or commit, or a fault hook
    /// interrupts it.
    pub fn write<T, Er>(
        &mut self,
        f: impl FnOnce(&mut Writing<'_, S, E>) -> Result<T, Er>,
    ) -> Result<T, Er>
    where
        Er: From<StoreError>,
    {
        let before = (self.writer.head(), self.writer.latest_hlc());
        let written = self.try_write(f);
        if written.is_err() {
            // The log and clock as stored decide; if they can't be read, nothing was written.
            let head = rows::head(&self.db, self.writer.device());
            let clock = schema::identity(&self.db).map(|identity| identity.clock);
            let (head, clock) = head.and_then(|head| Ok((head, clock?))).unwrap_or(before);
            self.writer.restore(head, clock);
        }
        written
    }

    fn try_write<T, Er>(
        &mut self,
        f: impl FnOnce(&mut Writing<'_, S, E>) -> Result<T, Er>,
    ) -> Result<T, Er>
    where
        Er: From<StoreError>,
    {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StoreError::from)?;
        proceed(&mut *self.faults, Point::Began)?;
        let mut writing = Writing {
            tx: &tx,
            writer: &mut self.writer,
            faults: &mut *self.faults,
            location: self.location,
            touched: Vec::new(),
        };
        let value = f(&mut writing)?;
        let touched = writing.touched;
        projection::update(&tx, &touched)?;
        schema::store_clock(&tx, self.writer.latest_hlc())?;
        proceed(&mut *self.faults, Point::Committing)?;
        tx.commit().map_err(StoreError::from)?;
        // The write has happened: only a crash can interrupt the store here.
        let _ = self.faults.proceed(Point::Committed);
        Ok(value)
    }

    /// The database, for tests that check its settings.
    #[cfg(test)]
    pub(crate) const fn db(&self) -> &Connection {
        &self.db
    }

    /// The device the store belongs to.
    pub const fn device(&self) -> Id<Device> {
        self.writer.device()
    }

    /// The location the store keeps events for.
    pub const fn location(&self) -> Id<Location> {
        self.location
    }

    /// The device's clock: the latest HLC its log writer issued or observed.
    pub const fn clock(&self) -> Hlc {
        self.writer.latest_hlc()
    }

    /// The last event of `device`'s log that the store holds; [`LogHead::EMPTY`] if none.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the store can't be read, and [`StoreError::Corrupt`] if what
    /// it reads doesn't make sense.
    pub fn head(&self, device: Id<Device>) -> Result<LogHead, StoreError> {
        rows::head(&self.db, device)
    }

    /// How far into each device's log the store holds: the last sequence number of every device
    /// it has events from.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn version_vector(&self) -> Result<BTreeMap<Id<Device>, u64>, StoreError> {
        let mut statement = self
            .db
            .prepare("SELECT origin_device, MAX(origin_seq) FROM events GROUP BY origin_device")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)))?;
        rows.map(|row| {
            let (device, seq) = row?;
            let seq = u64::try_from(seq).map_err(|_| StoreError::Corrupt("a sequence number"))?;
            Ok((schema::id(&device)?, seq))
        })
        .collect()
    }

    /// Up to `limit` events of `device`'s log, in order, from the one after `after`.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn log(
        &self,
        device: Id<Device>,
        after: u64,
        limit: u32,
    ) -> Result<Vec<SignedEvent>, StoreError> {
        let Ok(after) = rows::seq_value(after) else { return Ok(Vec::new()) };
        let mut statement = self.db.prepare(
            "SELECT message, hash FROM events WHERE origin_device = ?1 AND origin_seq > ?2 \
             ORDER BY origin_seq LIMIT ?3",
        )?;
        let rows = statement
            .query_map(params![&device.to_bytes()[..], after, limit], rows::stored_event)?;
        rows::collect(rows)
    }

    /// The events of `stream`, in canonical order: by HLC, then device, then position in the
    /// device's log.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn stream(&self, stream: &StreamRef) -> Result<Vec<SignedEvent>, StoreError> {
        rows::stream_events(&self.db, stream.kind.as_str(), stream.id)
    }

    /// The event with identifier `id`, if the store holds it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn event(&self, id: Id<Event>) -> Result<Option<SignedEvent>, StoreError> {
        self.db
            .query_row(
                "SELECT message, hash FROM events WHERE event_id = ?1",
                [&id.to_bytes()[..]],
                rows::stored_event,
            )
            .optional()?
            .transpose()
    }

    /// Folds `aggregate` from its stream's events in canonical order: the first step of a
    /// command. A command that must see exactly the state it changes loads it inside its write,
    /// with [`Writing::load`].
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn load<A: Aggregate>(&self, aggregate: A) -> Result<A, StoreError> {
        projection::load(&self.db, aggregate)
    }

    /// The order `id`, as the `orders` projection keeps it; `None` if the store holds no event
    /// of it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn order(&self, id: Id<Order>) -> Result<Option<OrderSummary>, StoreError> {
        self.db
            .query_row(
                &format!("SELECT {ORDER_COLUMNS} FROM orders WHERE order_id = ?1"),
                [&id.to_bytes()[..]],
                projection::order_summary,
            )
            .optional()?
            .transpose()
    }

    /// The orders in `state`, the earliest first by the HLC of their first event.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn orders(&self, state: OrderState) -> Result<Vec<OrderSummary>, StoreError> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {ORDER_COLUMNS} FROM orders WHERE state = ?1 ORDER BY first_hlc, order_id"
        ))?;
        let rows = statement.query_map([state.code()], projection::order_summary)?;
        rows.map(|row| row?).collect()
    }

    /// The payment `id`, as the `payments` projection keeps it; `None` if the store holds no
    /// event of it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn payment(&self, id: Id<Payment>) -> Result<Option<PaymentSummary>, StoreError> {
        self.db
            .query_row(
                &format!("SELECT {PAYMENT_COLUMNS} FROM payments WHERE payment_id = ?1"),
                [&id.to_bytes()[..]],
                projection::payment_summary,
            )
            .optional()?
            .transpose()
    }

    /// The payments initiated for order `order`, the earliest first by the HLC of their first
    /// event.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn payments_of(&self, order: Id<Order>) -> Result<Vec<PaymentSummary>, StoreError> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {PAYMENT_COLUMNS} FROM payments WHERE order_id = ?1 \
             ORDER BY first_hlc, payment_id"
        ))?;
        let rows = statement.query_map([&order.to_bytes()[..]], projection::payment_summary)?;
        rows.map(|row| row?).collect()
    }

    /// The effect with key `key`, if the outbox holds one.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn effect(&self, key: &[u8]) -> Result<Option<Queued>, StoreError> {
        outbox::get(&self.db, key)
    }

    /// Up to `limit` pending effects due by `now`, for their executors to start: the earliest
    /// due first, then in the order they were enqueued.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn due_effects(&self, now: Timestamp, limit: u32) -> Result<Vec<Queued>, StoreError> {
        outbox::due(&self.db, now, limit)
    }

    /// The running effects, in the order they were enqueued. After a restart, each is in doubt:
    /// find out whether its attempt happened, then finish or retry it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn running_effects(&self) -> Result<Vec<Queued>, StoreError> {
        outbox::in_state(&self.db, EffectState::Running)
    }

    /// Every effect in the outbox, in the order they were enqueued.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn effects(&self) -> Result<Vec<Queued>, StoreError> {
        outbox::all(&self.db)
    }

    /// Everything in the quarantine, in the order it arrived.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn quarantine(&self) -> Result<Vec<Quarantined>, StoreError> {
        let mut statement =
            self.db.prepare("SELECT reason, message FROM quarantine ORDER BY arrival")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
        rows.map(|row| {
            let (code, message) = row?;
            let reason = Reason::from_code(&code).ok_or(StoreError::Corrupt("a reason"))?;
            Ok(Quarantined { reason, message })
        })
        .collect()
    }
}

/// The event at `seq` in `device`'s log, if the store holds it.
fn event_at(
    db: &Connection,
    device: Id<Device>,
    seq: u64,
) -> Result<Option<SignedEvent>, StoreError> {
    db.query_row(
        "SELECT message, hash FROM events WHERE origin_device = ?1 AND origin_seq = ?2",
        params![&device.to_bytes()[..], rows::seq_value(seq)?],
        rows::stored_event,
    )
    .optional()?
    .transpose()
}
