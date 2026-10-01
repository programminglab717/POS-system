//! The store: opening it, writing to it, and reading it back.

use core::cell::Cell;
use core::time::Duration;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use keel_domain::aggregate::Aggregate;
use keel_domain::order::Order;
use keel_domain::payment::Payment;
use keel_events::envelope::{Device, Event, Location, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::Signer;
use keel_events::log::{LogConfig, LogHead, LogWriter};
use keel_types::{Entropy, Hlc, Id, Timestamp};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::check::{self, Problem};
use crate::error::StoreError;
use crate::faults::{Faults, NoFaults, Point, proceed};
use crate::key::StoreKey;
use crate::outbox::{self, EffectState, Queued};
use crate::projection::{
    self, ORDER_COLUMNS, OrderState, OrderSummary, PAYMENT_COLUMNS, PaymentSummary, Projection,
};
use crate::rows;
use crate::schema;
use crate::sequencing::{self, Sequenced, StoreSeq};
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

/// A device's store: its own log, the events it received from other replicas, the quarantine,
/// the projections and the outbox, in one SQLite database, encrypted with SQLCipher.
pub struct Store<S, E> {
    db: Connection,
    path: PathBuf,
    key: StoreKey,
    location: Id<Location>,
    writer: LogWriter<S, E>,
    faults: Box<dyn Faults + Send>,
    health: Cell<Health>,
    recovered: bool,
}

/// Whether the store can go on using its connection to the database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Health {
    /// It can.
    Sound,
    /// It met a damaged page or a malformed file, and refuses everything until it is reopened:
    /// SQLCipher refuses every read of its connection after a page fails.
    Damaged,
    /// A rekey left it without a connection it can trust, and it refuses everything until it is
    /// reopened.
    Closed,
}

impl Health {
    /// Ok if the store can go on.
    pub(crate) const fn usable(self) -> Result<(), StoreError> {
        match self {
            Health::Sound => Ok(()),
            Health::Damaged => Err(StoreError::Damaged),
            Health::Closed => Err(StoreError::Closed),
        }
    }
}

/// Passes `error` on, noting in `health` that the store is damaged if it says so.
pub(crate) fn noted(health: &Cell<Health>, error: StoreError) -> StoreError {
    if matches!(error, StoreError::Damaged) {
        health.set(Health::Damaged);
    }
    error
}

impl<S, E> core::fmt::Debug for Store<S, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .field("location", &self.location)
            .field("writer", &self.writer)
            .field("health", &self.health.get())
            .finish_non_exhaustive()
    }
}

impl<S: Signer, E: Entropy> Store<S, E> {
    /// Opens the store at `path` with `key`, creating it for `config`'s device if it doesn't
    /// exist. The device's log writer signs with `signer` and draws identifiers from `entropy`,
    /// and resumes from the stored log and clock.
    ///
    /// # Errors
    /// [`StoreError::KeyRejected`] if `key` doesn't open the store,
    /// [`StoreError::Settings`] if SQLite won't run with the durability settings,
    /// [`StoreError::NewerSchema`] if a newer kernel wrote the database,
    /// [`StoreError::NotThisDevice`] if it belongs to another device or location,
    /// [`StoreError::WrongSigner`] if `signer` didn't sign the device's stored events,
    /// [`StoreError::Damaged`] if a page it reads is damaged, and
    /// [`StoreError::Database`] if the file can't be opened, read or written.
    pub fn open(
        path: impl AsRef<Path>,
        key: StoreKey,
        config: StoreConfig,
        signer: S,
        entropy: E,
    ) -> Result<Store<S, E>, StoreError> {
        Store::open_with_faults(path, key, config, signer, entropy, Box::new(NoFaults))
    }

    /// As [`Store::open`], with `faults` deciding whether each write goes on at each
    /// [`Point`]: for crash-safety tests, and the simulator.
    ///
    /// # Errors
    /// As [`Store::open`], and [`StoreError::Interrupted`] if `faults` interrupts creating the
    /// database's schema.
    pub fn open_with_faults(
        path: impl AsRef<Path>,
        key: StoreKey,
        config: StoreConfig,
        signer: S,
        entropy: E,
        mut faults: Box<dyn Faults + Send>,
    ) -> Result<Store<S, E>, StoreError> {
        let path = path.as_ref().to_path_buf();
        // SQLite removes its journal when the store closes cleanly, so one left behind means the
        // store wasn't. If that can't be told, it is safer to assume it wasn't.
        let recovered = journal(&path).try_exists().unwrap_or(true);
        if recovered {
            // Before a connection that could move the journal away, even refused.
            schema::check_key(&path, &key)?;
        }
        let mut db = schema::connect(&path, &key)?;
        let (identity, head) = match opening(&mut db, config, &signer, &mut *faults) {
            Ok(opened) => opened,
            // What went wrong may have been a damaged page.
            Err(error) if schema::sound(&db) => return Err(error),
            Err(_) => return Err(StoreError::Damaged),
        };
        let log = LogConfig {
            device: config.device,
            location: config.location,
            head,
            latest_hlc: identity.clock,
            max_forward_drift: config.max_forward_drift,
        };
        let writer = LogWriter::new(log, signer, entropy);
        let mut store = Store {
            db,
            path,
            key,
            location: config.location,
            writer,
            faults,
            health: Cell::new(Health::Sound),
            recovered,
        };
        let stale = store.proven(projection::stale(&store.db))?;
        if !stale.is_empty() {
            let rebuilt = store.rebuild(&stale);
            store.proven(rebuilt)?;
        }
        store.proven(Ok(()))?;
        Ok(store)
    }

    /// Whether the store wasn't closed cleanly the last time: the process crashed or was killed,
    /// or the device lost power. SQLite recovered its journal when the store opened; run a
    /// [`Store::check`] when the device is next idle.
    pub const fn recovered(&self) -> bool {
        self.recovered
    }

    /// Re-encrypts the store with `key`, which becomes the store's. Every page is rewritten in
    /// one transaction, so a crash leaves the store under one key or the other.
    ///
    /// To rotate the key, store the new wrapped key as pending first, rekey, then make it the
    /// current one. After a crash, if the current key is refused, the pending one opens the
    /// store.
    ///
    /// # Errors
    /// [`StoreError::NotRekeyed`] if SQLCipher couldn't re-encrypt the store: it still has its
    /// old key, and is usable. [`StoreError::Interrupted`] if a fault hook interrupts the rekey
    /// before it begins, and [`StoreError::Damaged`] if the store's file doesn't authenticate
    /// under either key. Any other error leaves the store refusing everything
    /// ([`StoreError::Closed`]): open it again, with the new key if the old one is refused.
    pub fn rekey(&mut self, key: StoreKey) -> Result<(), StoreError> {
        self.health.get().usable()?;
        proceed(&mut *self.faults, Point::Rekeying)?;
        // SQLCipher 4.14 reports success even when it couldn't rewrite the store, and its
        // connection then encrypts the pages it writes with the new key, while the rest keep the
        // old one: one more write, and neither key opens the store. So the connection that ran
        // the rekey is never used again. The store opens its file afresh with the new key, and
        // proves every page authenticates under it; if the new key is refused, it opens the file
        // with the old one (ADR-0018).
        let _ = self.db.execute_batch(&key.pragma("rekey"));
        self.health.set(Health::Closed);
        match schema::connect(&self.path, &key) {
            Ok(db) => {
                self.db = db;
                if !check::pages(&self.db)?.is_empty() {
                    self.health.set(Health::Damaged);
                    return Err(StoreError::Damaged);
                }
                self.key = key;
            }
            Err(StoreError::KeyRejected) => {
                // The rekey didn't commit, so it wrote nothing: the store is as it was.
                self.db = match schema::connect(&self.path, &self.key) {
                    Err(StoreError::KeyRejected) => {
                        self.health.set(Health::Damaged);
                        return Err(StoreError::Damaged);
                    }
                    connected => connected?,
                };
                self.health.set(Health::Sound);
                return Err(StoreError::NotRekeyed);
            }
            Err(error) => return Err(error),
        }
        self.health.set(Health::Sound);
        // The rekey has happened: only a crash can interrupt the store here.
        let _ = self.faults.proceed(Point::Rekeyed);
        Ok(())
    }

    /// Checks the whole store, and reports every problem it finds, in order: none if the store is
    /// sound (ADR-0018). It moves the WAL into the database file, then checks:
    ///
    /// 1. that every page of the file authenticates, and stops if one doesn't;
    /// 2. SQLite's structure: b-trees, indexes against their tables, types and constraints, and
    ///    stops if it is malformed;
    /// 3. every event: that it decodes, hashes to its stored hash, and is filed under the columns
    ///    its body gives; and every device's log: no gaps from its first event, each event linked
    ///    to the one before it by hash, with a later HLC;
    /// 4. the store's identity, and the device's clock against its last event;
    /// 5. every projection row, against a rebuild from the stored events, which changes nothing;
    /// 6. every effect in the outbox, and every quarantined message.
    ///
    /// Signatures aren't verified again: the store verified each received event before storing
    /// it, and changing a stored event takes the key. The check takes time in proportion to the
    /// store, and nothing else can use the store while it runs: run it when the device is idle.
    /// It works on a damaged store too, which stays damaged until it is reopened.
    ///
    /// # Errors
    /// [`StoreError::Busy`] if another connection keeps the WAL from moving into the database
    /// file, [`StoreError::Closed`] if a rekey left the store without a connection, and
    /// [`StoreError::Database`] if the store can't be read.
    pub fn check(&mut self) -> Result<Vec<Problem>, StoreError> {
        match self.health.get() {
            Health::Sound => {}
            Health::Damaged => {
                // SQLCipher refuses every read of a connection after a page failed: check with a
                // fresh one. The key opened the store before, so a key refused now is a damaged
                // first page.
                self.db = match schema::connect(&self.path, &self.key) {
                    Err(StoreError::KeyRejected) => return Ok(vec![Problem::Page(1)]),
                    connected => connected?,
                };
            }
            Health::Closed => return Err(StoreError::Closed),
        }
        let checked = check::run(&mut self.db, self.writer.device(), self.location);
        self.proven(checked)
    }

    /// Drops every projection and rebuilds it from the stored events, in one transaction. The
    /// store does this itself when it opens, for a projection whose version changed: this is for
    /// support, and tests.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the store can't be read or written, [`StoreError::Corrupt`]
    /// if an event doesn't read back as stored, [`StoreError::Damaged`] if the store is damaged,
    /// and [`StoreError::Interrupted`] if a fault hook interrupts the rebuild, which then changes
    /// nothing.
    pub fn rebuild_projections(&mut self) -> Result<(), StoreError> {
        self.health.get().usable()?;
        let all: Vec<&Projection> = projection::ALL.iter().collect();
        let rebuilt = self.rebuild(&all);
        self.proven(rebuilt)
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
    /// interrupts it. If the write met a damaged page, [`StoreError::Damaged`] instead of either,
    /// since what went wrong may come of what it read there.
    pub fn write<T, Er>(
        &mut self,
        f: impl FnOnce(&mut Writing<'_, S, E>) -> Result<T, Er>,
    ) -> Result<T, Er>
    where
        Er: From<StoreError>,
    {
        self.health.get().usable()?;
        let before = (self.writer.head(), self.writer.latest_hlc());
        let written = self.try_write(f);
        if written.is_err() {
            // What went wrong may have been a damaged page: then the write's own error, which
            // may come of what it read there, gives way.
            if !schema::sound(&self.db) {
                self.health.set(Health::Damaged);
            }
            // The log and clock as stored decide; if they can't be read, nothing was written. A
            // damaged store isn't read again.
            let stored = (self.health.get() == Health::Sound)
                .then(|| {
                    let head = rows::head(&self.db, self.writer.device())?;
                    let clock = schema::identity(&self.db)?.clock;
                    Ok::<_, StoreError>((head, clock))
                })
                .and_then(Result::ok);
            let (head, clock) = stored.unwrap_or(before);
            self.writer.restore(head, clock);
            self.health.get().usable()?;
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
        let health = &self.health;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| noted(health, error.into()))?;
        proceed(&mut *self.faults, Point::Began)?;
        let mut writing = Writing {
            tx: &tx,
            writer: &mut self.writer,
            faults: &mut *self.faults,
            location: self.location,
            touched: Vec::new(),
            health,
        };
        let value = f(&mut writing)?;
        let touched = writing.touched;
        projection::update(&tx, &touched).map_err(|error| noted(health, error))?;
        schema::store_clock(&tx, self.writer.latest_hlc()).map_err(|error| noted(health, error))?;
        proceed(&mut *self.faults, Point::Committing)?;
        tx.commit().map_err(|error| noted(health, error.into()))?;
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

    /// Reads the store with `f`, unless it is damaged.
    fn read<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.health.get().usable()?;
        self.proven(f(&self.db))
    }

    /// `result`, unless making it met a damaged page: then [`StoreError::Damaged`], whatever
    /// it read discarded, and the store is damaged from then on. See [`schema::sound`].
    fn proven<T>(&self, result: Result<T, StoreError>) -> Result<T, StoreError> {
        if matches!(result, Err(StoreError::Damaged)) || !schema::sound(&self.db) {
            self.health.set(Health::Damaged);
            return Err(StoreError::Damaged);
        }
        result
    }

    /// The last event of `device`'s log that the store holds; [`LogHead::EMPTY`] if none.
    ///
    /// # Errors
    /// [`StoreError::Database`] if the store can't be read, [`StoreError::Corrupt`] if what it
    /// reads doesn't make sense, and [`StoreError::Damaged`] if the store is damaged.
    pub fn head(&self, device: Id<Device>) -> Result<LogHead, StoreError> {
        self.read(|db| rows::head(db, device))
    }

    /// How far into each device's log the store holds: the last sequence number of every device
    /// it has events from.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn version_vector(&self) -> Result<BTreeMap<Id<Device>, u64>, StoreError> {
        self.read(|db| {
            let mut statement = db.prepare(
                "SELECT origin_device, MAX(origin_seq) FROM events GROUP BY origin_device",
            )?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)))?;
            rows.map(|row| {
                let (device, seq) = row?;
                let seq =
                    u64::try_from(seq).map_err(|_| StoreError::Corrupt("a sequence number"))?;
                Ok((schema::id(&device)?, seq))
            })
            .collect()
        })
    }

    /// Numbers, as the Store Hub in `epoch`, the events of each device's log after the last
    /// position any sequencing record covers, except records, in the order the store received
    /// them, and appends the records at physical time `now`, in a write of their own (ADR-0020).
    /// Returns the records: none when nothing is new. Numbers follow on from the store's last
    /// record of the epoch, so a hub must sequence only once its log is settled.
    ///
    /// # Errors
    /// As [`Store::write`], and [`StoreError::OutOfRange`] if the epoch isn't from 1 to 2^63 − 1
    /// or its numbers run out.
    pub fn sequence(&mut self, epoch: u64, now: Timestamp) -> Result<Vec<SignedEvent>, StoreError> {
        self.write(|writing| writing.sequence(epoch, now))
    }

    /// Answers, as the Store Hub in `epoch`, every request for an order the store holds that no
    /// answer names yet, at physical time `now`, in a write of its own (ADR-0021): each is
    /// granted, or refused if its lease has moved on, its device already owns the order, or a
    /// payment of the order is in progress. Returns the answers: none when no request waits.
    ///
    /// # Errors
    /// As [`Store::write`], and [`StoreError::OutOfRange`] if the epoch isn't from 1 to 2^63 − 1.
    pub fn answer_requests(
        &mut self,
        epoch: u64,
        now: Timestamp,
    ) -> Result<Vec<SignedEvent>, StoreError> {
        self.write(|writing| writing.answer_requests(epoch, now))
    }

    /// For each device, how far into its log the store holds confirmed events: the longest start
    /// of its log in which each event is confirmed, or a sequencing record (ADR-0020).
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn confirmed(&self) -> Result<BTreeMap<Id<Device>, u64>, StoreError> {
        self.read(sequencing::confirmed)
    }

    /// The epoch and number of the event at `position` of `device`'s log, once confirmed: `None`
    /// while it is provisional. Where records disagree, its first number counts.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn store_seq(
        &self,
        device: Id<Device>,
        position: u64,
    ) -> Result<Option<StoreSeq>, StoreError> {
        self.read(|db| sequencing::store_seq(db, device, position))
    }

    /// Up to `limit` of the confirmed events of `epoch` numbered after `after`, in number order:
    /// the store's feed. An event appears under its first number only.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn sequenced(
        &self,
        epoch: u64,
        after: u64,
        limit: u32,
    ) -> Result<Vec<Sequenced>, StoreError> {
        self.read(|db| sequencing::sequenced(db, epoch, after, limit))
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
        self.read(|db| {
            let Ok(after) = rows::seq_value(after) else { return Ok(Vec::new()) };
            let mut statement = db.prepare(
                "SELECT message, hash FROM events WHERE origin_device = ?1 AND origin_seq > ?2 \
                 ORDER BY origin_seq LIMIT ?3",
            )?;
            let rows = statement
                .query_map(params![&device.to_bytes()[..], after, limit], rows::stored_event)?;
            rows::collect(rows)
        })
    }

    /// The events of `stream`, in canonical order: by HLC, then device, then position in the
    /// device's log.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn stream(&self, stream: &StreamRef) -> Result<Vec<SignedEvent>, StoreError> {
        self.read(|db| rows::stream_events(db, stream.kind.as_str(), stream.id))
    }

    /// The event with identifier `id`, if the store holds it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn event(&self, id: Id<Event>) -> Result<Option<SignedEvent>, StoreError> {
        self.read(|db| {
            db.query_row(
                "SELECT message, hash FROM events WHERE event_id = ?1",
                [&id.to_bytes()[..]],
                rows::stored_event,
            )
            .optional()?
            .transpose()
        })
    }

    /// Folds `aggregate` from its stream's events in canonical order: the first step of a
    /// command. A command that must see exactly the state it changes loads it inside its write,
    /// with [`Writing::load`].
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn load<A: Aggregate>(&self, aggregate: A) -> Result<A, StoreError> {
        self.read(|db| projection::load(db, aggregate))
    }

    /// The order `id`, as the `orders` projection keeps it; `None` if the store holds no event
    /// of it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn order(&self, id: Id<Order>) -> Result<Option<OrderSummary>, StoreError> {
        self.read(|db| {
            db.query_row(
                &format!("SELECT {ORDER_COLUMNS} FROM orders WHERE order_id = ?1"),
                [&id.to_bytes()[..]],
                projection::order_summary,
            )
            .optional()?
            .transpose()
        })
    }

    /// The orders in `state`, the earliest first by the HLC of their first event.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn orders(&self, state: OrderState) -> Result<Vec<OrderSummary>, StoreError> {
        self.read(|db| {
            let mut statement = db.prepare(&format!(
                "SELECT {ORDER_COLUMNS} FROM orders WHERE state = ?1 ORDER BY first_hlc, order_id"
            ))?;
            let rows = statement.query_map([state.code()], projection::order_summary)?;
            rows.map(|row| row?).collect()
        })
    }

    /// The payment `id`, as the `payments` projection keeps it; `None` if the store holds no
    /// event of it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn payment(&self, id: Id<Payment>) -> Result<Option<PaymentSummary>, StoreError> {
        self.read(|db| {
            db.query_row(
                &format!("SELECT {PAYMENT_COLUMNS} FROM payments WHERE payment_id = ?1"),
                [&id.to_bytes()[..]],
                projection::payment_summary,
            )
            .optional()?
            .transpose()
        })
    }

    /// The payments initiated for order `order`, the earliest first by the HLC of their first
    /// event.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn payments_of(&self, order: Id<Order>) -> Result<Vec<PaymentSummary>, StoreError> {
        self.read(|db| {
            let mut statement = db.prepare(&format!(
                "SELECT {PAYMENT_COLUMNS} FROM payments WHERE order_id = ?1 \
                 ORDER BY first_hlc, payment_id"
            ))?;
            let rows = statement.query_map([&order.to_bytes()[..]], projection::payment_summary)?;
            rows.map(|row| row?).collect()
        })
    }

    /// The effect with key `key`, if the outbox holds one.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn effect(&self, key: &[u8]) -> Result<Option<Queued>, StoreError> {
        self.read(|db| outbox::get(db, key))
    }

    /// Up to `limit` pending effects due by `now`, for their executors to start: the earliest
    /// due first, then in the order they were enqueued.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn due_effects(&self, now: Timestamp, limit: u32) -> Result<Vec<Queued>, StoreError> {
        self.read(|db| outbox::due(db, now, limit))
    }

    /// The running effects, in the order they were enqueued. After a restart, each is in doubt:
    /// find out whether its attempt happened, then finish or retry it.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn running_effects(&self) -> Result<Vec<Queued>, StoreError> {
        self.read(|db| outbox::in_state(db, EffectState::Running))
    }

    /// Every effect in the outbox, in the order they were enqueued.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn effects(&self) -> Result<Vec<Queued>, StoreError> {
        self.read(outbox::all)
    }

    /// Everything in the quarantine, in the order it arrived.
    ///
    /// # Errors
    /// As [`Store::head`].
    pub fn quarantine(&self) -> Result<Vec<Quarantined>, StoreError> {
        self.read(|db| {
            let mut statement =
                db.prepare("SELECT reason, message FROM quarantine ORDER BY arrival")?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
            rows.map(|row| {
                let (code, message) = row?;
                let reason = Reason::from_code(&code).ok_or(StoreError::Corrupt("a reason"))?;
                Ok(Quarantined { reason, message })
            })
            .collect()
        })
    }
}

/// Brings the store in `db` up to date, and checks it belongs to `config`'s device, signed by
/// `signer`: who it belongs to, and the head of the device's log.
fn opening<S: Signer>(
    db: &mut Connection,
    config: StoreConfig,
    signer: &S,
    faults: &mut dyn Faults,
) -> Result<(schema::Identity, LogHead), StoreError> {
    schema::migrate(db, config.device, config.location, faults)?;
    let identity = schema::identity(db)?;
    if identity.device != config.device || identity.location != config.location {
        return Err(StoreError::NotThisDevice);
    }
    let head = rows::head(db, config.device)?;
    if let Some(last) = event_at(db, config.device, head.seq())?
        && last.key_id() != signer.public_key().key_id()
    {
        return Err(StoreError::WrongSigner);
    }
    Ok((identity, head))
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

/// The WAL journal of the database at `path`, which SQLite keeps beside it.
fn journal(path: &Path) -> PathBuf {
    let mut journal = path.as_os_str().to_owned();
    journal.push("-wal");
    PathBuf::from(journal)
}
