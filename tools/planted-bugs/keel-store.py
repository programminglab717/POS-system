"""Planted bugs for keel-store: see run.py."""

BUGS = [
    # Settings. Only the unit tests, which read them from the store's connection, can catch
    # these: a process that crashes loses no commit either way, and nothing outside the store can
    # read a connection's settings. Commits that don't wait for the disk show only when power is
    # lost, which keel-sim will simulate.
    (
        "the journal isn't the WAL",
        "src/schema.rs",
        """    let journal: String = db.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Settings("the WAL journal"));
    }""",
        """    let _journal: String =
        db.query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))?;""",
        "unit",
    ),
    (
        "commits don't wait for the disk",
        "src/schema.rs",
        """    db.execute_batch("PRAGMA synchronous = FULL; PRAGMA trusted_schema = OFF;")?;
    // FULL is 2.
    let synchronous: i64 = db.query_row("PRAGMA synchronous", [], |row| row.get(0))?;
    if synchronous != 2 {
        return Err(StoreError::Settings("synchronous commits"));
    }""",
        """    db.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA trusted_schema = OFF;")?;""",
        "unit",
    ),
    (
        "the schema is trusted",
        "src/schema.rs",
        "PRAGMA trusted_schema = OFF;",
        "PRAGMA trusted_schema = ON;",
        "unit",
    ),
    # Opening. Only the unit tests make a newer kernel's database; the property tests open stores
    # as they left them, and as strangers.
    (
        "a newer kernel's database opens",
        "src/schema.rs",
        """    if version > VERSION {
        return Err(StoreError::NewerSchema { found: version, known: VERSION });
    }
""",
        "",
        "unit",
    ),
    (
        "a migration commits before its last point",
        "src/schema.rs",
        """    proceed(faults, Point::Migrating)?;
    tx.commit()?;""",
        """    tx.commit()?;
    proceed(faults, Point::Migrating)?;""",
    ),
    (
        "a migration commits its tables apart from the rest",
        "src/schema.rs",
        """    if version < 1 {
        tx.execute_batch(V1)?;
        tx.execute(
            "INSERT INTO store (singleton, device, location, clock) VALUES (1, ?1, ?2, ?3)",
            params![&device.to_bytes()[..], &location.to_bytes()[..], &hlc_bytes(Hlc::ZERO)[..]],
        )?;
    }
    if version < 2 {
        tx.execute_batch(V2)?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    proceed(faults, Point::Migrating)?;
    tx.commit()?;""",
        """    if version < 1 {
        tx.execute_batch(V1)?;
    }
    tx.commit()?;
    proceed(faults, Point::Migrating)?;
    if version < 1 {
        db.execute(
            "INSERT INTO store (singleton, device, location, clock) VALUES (1, ?1, ?2, ?3)",
            params![&device.to_bytes()[..], &location.to_bytes()[..], &hlc_bytes(Hlc::ZERO)[..]],
        )?;
    }
    if version < 2 {
        db.execute_batch(V2)?;
    }
    db.pragma_update(None, "user_version", VERSION)?;""",
    ),
    (
        "a version 1 store isn't given the outbox",
        "src/schema.rs",
        """    if version < 2 {
        tx.execute_batch(V2)?;
    }""",
        """    if version < 1 {
        tx.execute_batch(V2)?;
    }""",
        "unit",
    ),
    (
        "a store opens for another device",
        "src/store.rs",
        "if identity.device != config.device || identity.location != config.location {",
        "if identity.location != config.location {",
    ),
    (
        "a store opens at another location",
        "src/store.rs",
        "if identity.device != config.device || identity.location != config.location {",
        "if identity.device != config.device {",
    ),
    (
        "a store opens with another key",
        "src/store.rs",
        """        if let Some(last) = event_at(&db, config.device, head.seq())?
            && last.key_id() != signer.public_key().key_id()
        {
            return Err(StoreError::WrongKey);
        }
""",
        "",
    ),
    (
        "a reopened writer starts a new log",
        "src/store.rs",
        "        let head = rows::head(&db, config.device)?;",
        "        let head = LogHead::EMPTY;",
    ),
    (
        "a reopened writer's clock starts from its log",
        "src/store.rs",
        "            latest_hlc: identity.clock,",
        "            latest_hlc: Hlc::ZERO,",
    ),
    # Writes: one transaction each, and the writer and clock as stored after a failure.
    (
        "a failed write leaves the writer where it got to",
        "src/store.rs",
        """        if written.is_err() {
            // The log and clock as stored decide; if they can't be read, nothing was written.
            let head = rows::head(&self.db, self.writer.device());
            let clock = schema::identity(&self.db).map(|identity| identity.clock);
            let (head, clock) = head.and_then(|head| Ok((head, clock?))).unwrap_or(before);
            self.writer.restore(head, clock);
        }""",
        "",
    ),
    (
        "a failed write keeps the clock it moved on",
        "src/store.rs",
        "            self.writer.restore(head, clock);",
        "            self.writer.restore(head, self.writer.latest_hlc());",
    ),
    (
        "a failed caller's write commits",
        "src/store.rs",
        """        let value = f(&mut writing)?;
        let touched = writing.touched;
        projection::update(&tx, &touched)?;
        schema::store_clock(&tx, self.writer.latest_hlc())?;
        proceed(&mut *self.faults, Point::Committing)?;
        tx.commit().map_err(StoreError::from)?;
        // The write has happened: only a crash can interrupt the store here.
        let _ = self.faults.proceed(Point::Committed);
        Ok(value)""",
        """        let value = f(&mut writing);
        let touched = writing.touched;
        projection::update(&tx, &touched)?;
        schema::store_clock(&tx, self.writer.latest_hlc())?;
        proceed(&mut *self.faults, Point::Committing)?;
        tx.commit().map_err(StoreError::from)?;
        // The write has happened: only a crash can interrupt the store here.
        let _ = self.faults.proceed(Point::Committed);
        value""",
    ),
    (
        "the clock isn't stored",
        "src/store.rs",
        """        schema::store_clock(&tx, self.writer.latest_hlc())?;
""",
        "",
    ),
    (
        "the clock is stored as it was when the write began",
        "src/store.rs",
        """        proceed(&mut *self.faults, Point::Began)?;
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
        schema::store_clock(&tx, self.writer.latest_hlc())?;""",
        """        proceed(&mut *self.faults, Point::Began)?;
        schema::store_clock(&tx, self.writer.latest_hlc())?;
        let mut writing = Writing {
            tx: &tx,
            writer: &mut self.writer,
            faults: &mut *self.faults,
            location: self.location,
            touched: Vec::new(),
        };
        let value = f(&mut writing)?;
        let touched = writing.touched;
        projection::update(&tx, &touched)?;""",
    ),
    (
        "the Committing point comes after the commit",
        "src/store.rs",
        """        proceed(&mut *self.faults, Point::Committing)?;
        tx.commit().map_err(StoreError::from)?;""",
        """        tx.commit().map_err(StoreError::from)?;
        proceed(&mut *self.faults, Point::Committing)?;""",
    ),
    (
        "a refusal after the commit fails the write",
        "src/store.rs",
        "        let _ = self.faults.proceed(Point::Committed);",
        "        proceed(&mut *self.faults, Point::Committed)?;",
    ),
    (
        "appending doesn't reach the Stored point",
        "src/write.rs",
        """        self.touch(&event);
        proceed(self.faults, Point::Stored)?;
        Ok(event)""",
        """        self.touch(&event);
        Ok(event)""",
    ),
    # Receiving: the order of the checks, and what becomes of each event.
    (
        "events for other locations are stored",
        "src/write.rs",
        """        if body.location != self.location {
            return self.quarantine(bytes, Reason::OtherLocation);
        }
""",
        "",
    ),
    (
        "the location is checked only for the next event of a log",
        "src/write.rs",
        "        if body.location != self.location {",
        """        if body.location != self.location
            && rows::head(self.tx, body.origin_device)?.seq() + 1 == body.origin_seq.get()
        {""",
    ),
    (
        "positions beyond what a store holds aren't refused",
        "src/write.rs",
        """        if rows::seq_value(seq).is_err() {
            return self.quarantine(bytes, Reason::OutOfRange);
        }
""",
        "",
    ),
    (
        "a store holds positions up to 2^64 - 1",
        "src/rows.rs",
        """    i64::try_from(seq).map_err(|_| StoreError::OutOfRange("a sequence number"))""",
        """    Ok(i64::try_from(seq).unwrap_or(i64::MAX))""",
    ),
    (
        "the last position a store holds is refused",
        "src/rows.rs",
        """    i64::try_from(seq).map_err(|_| StoreError::OutOfRange("a sequence number"))""",
        """    i64::try_from(seq)
        .ok()
        .filter(|&seq| seq < i64::MAX)
        .ok_or(StoreError::OutOfRange("a sequence number"))""",
    ),
    (
        "an event with a used identifier is stored",
        "src/write.rs",
        """            Ok(Link::Next) if rows::has_id(self.tx, &body.event_id.to_bytes())? => {
                self.quarantine(bytes, Reason::DuplicateId)
            }
""",
        "",
    ),
    (
        "a replayed event is taken for one with a used identifier",
        "src/write.rs",
        "            Ok(Link::Next) if rows::has_id(self.tx, &body.event_id.to_bytes())? => {",
        "            Ok(_) if rows::has_id(self.tx, &body.event_id.to_bytes())? => {",
    ),
    (
        "the device's own events from elsewhere don't move its writer",
        "src/write.rs",
        "                if device == self.writer.device() {",
        "                if false {",
    ),
    (
        "received clocks aren't observed",
        "src/write.rs",
        "                    let _ = self.writer.observe(body.hlc, now);",
        "                    let _ = (body.hlc, now);",
    ),
    (
        "received clocks beyond the drift limit move the device's clock",
        "src/write.rs",
        "                    let _ = self.writer.observe(body.hlc, now);",
        """                    let (head, clock) = (self.writer.head(), self.writer.latest_hlc());
                    self.writer.restore(head, clock.max(body.hlc));
                    let _ = now;""",
    ),
    (
        "an earlier event is compared with the one before it",
        "src/write.rs",
        "            Ok(Link::Earlier) => match rows::hash_at(self.tx, device, seq)? {",
        "            Ok(Link::Earlier) => match rows::hash_at(self.tx, device, seq - 1)? {",
    ),
    (
        "a fork is taken for a duplicate",
        "src/write.rs",
        "                Some(_) => self.quarantine(bytes, Reason::Fork),",
        "                Some(_) => Ok(Received::Duplicate),",
    ),
    (
        "an event after a gap is stored",
        "src/write.rs",
        "            Err(ChainError::Gap { head, .. }) => Ok(Received::Gap { head }),",
        """            Err(ChainError::Gap { .. }) => {
                rows::insert(self.tx, &event)?;
                Ok(Received::Stored(Box::new(event)))
            }""",
    ),
    (
        "a gap reports the event's position, not the log's",
        "src/write.rs",
        "            Err(ChainError::Gap { head, .. }) => Ok(Received::Gap { head }),",
        "            Err(ChainError::Gap { .. }) => Ok(Received::Gap { head: seq }),",
    ),
    (
        "revoked events are quarantined as bad signatures",
        "src/write.rs",
        "            Rejection::Revoked { .. } => Reason::Revoked,",
        "            Rejection::Revoked { .. } => Reason::BadSignature,",
    ),
    (
        "misplaced events are quarantined as refused for another reason",
        "src/write.rs",
        "            Rejection::WrongLocation => Reason::WrongLocation,",
        "            Rejection::WrongLocation => Reason::Rejected,",
    ),
    (
        "clocks that went back are quarantined as broken links",
        "src/write.rs",
        "            ChainError::ClockRegressed { .. } => Reason::ClockRegressed,",
        "            ChainError::ClockRegressed { .. } => Reason::BrokenLink,",
    ),
    (
        "two reasons share a code",
        "src/write.rs",
        """            Reason::ClockRegressed => "clock_regressed",""",
        """            Reason::ClockRegressed => "broken_link",""",
    ),
    # The quarantine.
    (
        "the quarantine keeps the latest reason",
        "src/write.rs",
        "             ON CONFLICT (digest) DO NOTHING\",",
        "             ON CONFLICT (digest) DO UPDATE SET reason = excluded.reason\",",
    ),
    (
        "a message refused again for another reason is kept again",
        "src/write.rs",
        "        let digest = EventHash::of(bytes);",
        "        let digest = EventHash::of(&[bytes, reason.code().as_bytes()].concat());",
    ),
    (
        "quarantining doesn't reach the Stored point",
        "src/write.rs",
        """            rusqlite::params![&digest.as_bytes()[..], reason.code(), bytes],
        )?;
        proceed(self.faults, Point::Stored)?;""",
        """            rusqlite::params![&digest.as_bytes()[..], reason.code(), bytes],
        )?;""",
    ),
    (
        "the quarantine reads back in another order",
        "src/store.rs",
        "\"SELECT reason, message FROM quarantine ORDER BY arrival\"",
        "\"SELECT reason, message FROM quarantine ORDER BY digest\"",
    ),
    # Reads.
    (
        "HLCs are stored little-endian, so they don't sort as they compare",
        "src/schema.rs",
        """    hlc.to_u64().to_be_bytes()
}

/// An HLC from its stored bytes.
pub(crate) fn hlc(bytes: &[u8]) -> Result<Hlc, StoreError> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| StoreError::Corrupt("an HLC"))?;
    Ok(Hlc::from_u64(u64::from_be_bytes(bytes)))""",
        """    hlc.to_u64().to_le_bytes()
}

/// An HLC from its stored bytes.
pub(crate) fn hlc(bytes: &[u8]) -> Result<Hlc, StoreError> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| StoreError::Corrupt("an HLC"))?;
    Ok(Hlc::from_u64(u64::from_le_bytes(bytes)))""",
    ),
    (
        "streams are read in the order events arrived",
        "src/rows.rs",
        "         ORDER BY hlc, origin_device, origin_seq\",",
        "         ORDER BY arrival\",",
    ),
    (
        "ties in a stream are broken by the later device first",
        "src/rows.rs",
        "         ORDER BY hlc, origin_device, origin_seq\",",
        "         ORDER BY hlc, origin_device DESC, origin_seq\",",
    ),
    # Ordering ties by HLC alone isn't listed: SQLite reads a stream through its index, which is
    # ordered by device after HLC, so the result is the same.
    (
        "the version vector gives each log's first position",
        "src/store.rs",
        "\"SELECT origin_device, MAX(origin_seq) FROM events GROUP BY origin_device\"",
        "\"SELECT origin_device, MIN(origin_seq) FROM events GROUP BY origin_device\"",
    ),
    # Counting each log's events instead isn't listed: logs are stored without gaps, so the count
    # is the last position.
    (
        "a log reads from the position it was asked to start after",
        "src/store.rs",
        "origin_seq > ?2",
        "origin_seq >= ?2",
    ),
    (
        "a log reads one more event than asked",
        "src/store.rs",
        "ORDER BY origin_seq LIMIT ?3",
        "ORDER BY origin_seq LIMIT ?3 + 1",
    ),
    (
        "reading a log past the positions a store holds fails",
        "src/store.rs",
        "        let Ok(after) = rows::seq_value(after) else { return Ok(Vec::new()) };",
        "        let after = rows::seq_value(after)?;",
    ),
    (
        "a head is its log's first event",
        "src/rows.rs",
        "             ORDER BY origin_seq DESC LIMIT 1\",",
        "             ORDER BY origin_seq LIMIT 1\",",
    ),
    (
        "an event is looked up by its hash",
        "src/store.rs",
        "\"SELECT message, hash FROM events WHERE event_id = ?1\",",
        "\"SELECT message, hash FROM events WHERE hash = ?1\",",
    ),
    # Only a unit test changes a stored row; the integrity checks of slice 3 will look for them.
    (
        "a stored event's hash isn't checked",
        "src/rows.rs",
        """    if event.hash().as_bytes()[..] != *hash {
        return Err(StoreError::Corrupt("an event's hash"));
    }
""",
        "",
        "unit",
    ),
    # Projections (ADR-0017).
    (
        "a write doesn't update its projections",
        "src/store.rs",
        """        projection::update(&tx, &touched)?;
""",
        "",
    ),
    (
        "appended events don't mark their streams",
        "src/write.rs",
        """        self.touch(&event);
        proceed(self.faults, Point::Stored)?;""",
        "        proceed(self.faults, Point::Stored)?;",
    ),
    (
        "received events don't mark their streams",
        "src/write.rs",
        """                rows::insert(self.tx, &event)?;
                self.touch(&event);""",
        "                rows::insert(self.tx, &event)?;",
    ),
    (
        "a stream is reported each time a write touches it",
        "src/write.rs",
        """        if !self.touched.contains(stream) {
            self.touched.push(stream.clone());
        }""",
        "        self.touched.push(stream.clone());",
    ),
    (
        "projections fold each device's events together",
        "src/projection.rs",
        "            let events = rows::stream_events(db, projection.kind, stream.id)?;",
        """            let mut events = rows::stream_events(db, projection.kind, stream.id)?;
            events.sort_by_key(|event| (event.body().origin_device, event.body().origin_seq));""",
    ),
    (
        "a rebuild folds a stream backwards",
        "src/projection.rs",
        "        let events = rows::stream_events(db, projection.kind, stream)?;",
        """        let mut events = rows::stream_events(db, projection.kind, stream)?;
        events.reverse();""",
    ),
    (
        "a rebuild keeps only the first stream",
        "src/projection.rs",
        "ORDER BY stream_id\",",
        "ORDER BY stream_id LIMIT 1\",",
    ),
    (
        "stale projections aren't rebuilt when the store opens",
        "src/store.rs",
        "        if !stale.is_empty() {",
        "        if false {",
    ),
    # A rebuild that forgets its version rebuilds again at every opening, to the same rows; and no
    # property test changes a version. The unit tests read the versions back.
    (
        "a rebuild doesn't record its version",
        "src/projection.rs",
        """    db.execute(
        "INSERT INTO projections (name, version) VALUES (?1, ?2) \\
         ON CONFLICT (name) DO UPDATE SET version = excluded.version",
        params![projection.name, projection.version],
    )?;
""",
        "",
        "unit",
    ),
    (
        "a changed version doesn't rebuild",
        "src/projection.rs",
        "        if built != Some(projection.version) {",
        "        if built.is_none() {",
        "unit",
    ),
    (
        "rebuilds don't reach their fault point",
        "src/store.rs",
        """        proceed(&mut *self.faults, Point::Rebuilding)?;
""",
        "",
    ),
    # Interrupting a rebuild that already committed changes nothing a crash test can see: the
    # rebuilt rows are right. The unit test puts wrong rows in first.
    (
        "a rebuild commits before its fault point",
        "src/store.rs",
        """        proceed(&mut *self.faults, Point::Rebuilding)?;
        tx.commit()?;""",
        """        tx.commit()?;
        proceed(&mut *self.faults, Point::Rebuilding)?;""",
        "unit",
    ),
    (
        "loading folds each device's events together",
        "src/projection.rs",
        "    let events = rows::stream_events(db, A::Event::STREAM, aggregate.stream_id())?;",
        """    let mut events = rows::stream_events(db, A::Event::STREAM, aggregate.stream_id())?;
    events.sort_by_key(|event| (event.body().origin_device, event.body().origin_seq));""",
    ),
    (
        "an order's business date is its last event's",
        "src/projection.rs",
        "            info.and_then(|info| business_date(events, info.created_by)),",
        "            events.last().map(|event| event.body().business_date.to_string()),",
    ),
    (
        "open and closed checks are swapped",
        "src/projection.rs",
        """            count(checks.len().saturating_sub(closed))?,
            count(closed)?,""",
        """            count(closed)?,
            count(checks.len().saturating_sub(closed))?,""",
    ),
    (
        "every line counts as live",
        "src/projection.rs",
        "            count(order.live_lines().count())?,",
        "            count(order.lines().len())?,",
    ),
    (
        "an order's unreadable events aren't counted",
        "src/projection.rs",
        "            count(order.skipped().len())?,",
        "            count(0)?,",
    ),
    (
        "an order's conflicts aren't counted",
        "src/projection.rs",
        "            count(order.conflicts().len())?,",
        "            count(0)?,",
    ),
    (
        "a payment's conflicts aren't counted",
        "src/projection.rs",
        "            count(payment.conflicts().len())?,",
        "            count(0)?,",
    ),
    (
        "a payment's tip is its amount",
        "src/projection.rs",
        "            captured.and_then(|captured| captured.tip).map(Money::minor),",
        "            captured.map(|captured| captured.amount.minor()),",
    ),
    (
        "an order without its creation reads as active",
        "src/projection.rs",
        """        if order.info().is_none() {
            return OrderState::Uncreated;
        }""",
        "",
    ),
    (
        "a payment without its initiation reads as initiated",
        "src/projection.rs",
        """        if payment.info().is_none() {
            return PaymentState::Uninitiated;
        }""",
        "",
    ),
    (
        "a stream's first and last HLCs are swapped",
        "src/projection.rs",
        """            first: hlc_at(events.first()),
            last: hlc_at(events.last()),""",
        """            first: hlc_at(events.last()),
            last: hlc_at(events.first()),""",
    ),
    (
        "a stream's event count is one short",
        "src/projection.rs",
        "            events: count(events.len())?,",
        "            events: count(events.len().saturating_sub(1))?,",
    ),
    (
        "open and submitted stages share a code",
        "src/projection.rs",
        """        Stage::Open => "open",""",
        """        Stage::Open => "submitted",""",
    ),
    (
        "orders in a state are listed by identifier",
        "src/store.rs",
        "WHERE state = ?1 ORDER BY first_hlc, order_id\"",
        "WHERE state = ?1 ORDER BY order_id\"",
    ),
    (
        "an order's payments are found by their check",
        "src/store.rs",
        "FROM payments WHERE order_id = ?1 \\",
        "FROM payments WHERE check_id = ?1 \\",
    ),
    # The outbox (ADR-0017).
    (
        "enqueuing a key again inserts it again",
        "src/outbox.rs",
        """    if let Some(existing) = get(db, &effect.key)? {
        return if existing.effect == *effect {
            Ok(Enqueued::Already)
        } else {
            Err(EffectError::KeyInUse.into())
        };
    }
""",
        "",
    ),
    (
        "another effect under a key is taken for the same",
        "src/outbox.rs",
        "        return if existing.effect == *effect {",
        "        return if existing.effect.key == effect.key {",
    ),
    (
        "an effect's cause isn't checked",
        "src/outbox.rs",
        """    if let Some(cause) = effect.cause
        && !rows::has_id(db, &cause.to_bytes())?
    {
        return Err(EffectError::UnknownCause.into());
    }
""",
        "",
    ),
    (
        "effects start before they are due",
        "src/outbox.rs",
        """    if queued.due > now {
        return Err(EffectError::NotDue(queued.due).into());
    }
""",
        "",
    ),
    (
        "a start doesn't count its attempt",
        "src/outbox.rs",
        """    let attempts = queued.attempts.checked_add(1).ok_or(StoreError::OutOfRange("attempts"))?;""",
        "    let attempts = queued.attempts;",
    ),
    (
        "a start keeps the first start's time",
        "src/outbox.rs",
        "\"UPDATE outbox SET state = 'running', attempts = ?2, started = ?3 WHERE key = ?1\",",
        "\"UPDATE outbox SET state = 'running', attempts = ?2, started = coalesce(started, ?3) WHERE key = ?1\",",
    ),
    (
        "finishing an effect fails it",
        "src/write.rs",
        "        outbox::end(self.tx, key, EffectState::Done)",
        "        outbox::end(self.tx, key, EffectState::Failed)",
    ),
    (
        "an effect is retried whatever its state",
        "src/outbox.rs",
        """    expect(db, key, EffectState::Running)?;
    db.execute(
        "UPDATE outbox SET state = 'pending', due = ?2 WHERE key = ?1",""",
        """    get(db, key)?.ok_or(EffectError::Unknown)?;
    db.execute(
        "UPDATE outbox SET state = 'pending', due = ?2 WHERE key = ?1",""",
    ),
    (
        "a retry keeps its due time",
        "src/outbox.rs",
        "\"UPDATE outbox SET state = 'pending', due = ?2 WHERE key = ?1\",",
        "\"UPDATE outbox SET state = 'pending' WHERE key = ?1 AND ?2 IS NOT NULL\",",
    ),
    (
        "due effects come in the order they were enqueued",
        "src/outbox.rs",
        "         ORDER BY due, seq LIMIT ?2\"",
        "         ORDER BY seq LIMIT ?2\"",
    ),
    (
        "running effects are due again",
        "src/outbox.rs",
        "WHERE state = 'pending' AND due <= ?1",
        "WHERE state IN ('pending', 'running') AND due <= ?1",
    ),
    (
        "one more effect is due than asked",
        "src/outbox.rs",
        "         ORDER BY due, seq LIMIT ?2\"",
        "         ORDER BY due, seq LIMIT ?2 + 1\"",
    ),
    (
        "pending effects are listed as running",
        "src/outbox.rs",
        "FROM outbox WHERE state = ?1 ORDER BY seq",
        "FROM outbox WHERE state = ?1 OR state = 'pending' ORDER BY seq",
    ),
    # The property tests only make valid kinds and keys; the unit tests try the invalid ones.
    # A kind starting with a capital isn't listed: every letter must be lowercase anyway.
    (
        "an effect's kind may start with a digit",
        "src/outbox.rs",
        "word.as_bytes().first().is_some_and(u8::is_ascii_lowercase)",
        "word.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)",
        "unit",
    ),
    (
        "an empty key is accepted",
        "src/outbox.rs",
        "    if effect.key.is_empty() || effect.key.len() > MAX_KEY {",
        "    if effect.key.len() > MAX_KEY {",
        "unit",
    ),
]
