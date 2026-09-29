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
        """        proceed(faults, Point::Migrating)?;
        tx.commit()?;""",
        """        tx.commit()?;
        proceed(faults, Point::Migrating)?;""",
    ),
    (
        "a migration commits its tables apart from the rest",
        "src/schema.rs",
        """        tx.execute_batch(V1)?;
        tx.execute(
            "INSERT INTO store (singleton, device, location, clock) VALUES (1, ?1, ?2, ?3)",
            params![&device.to_bytes()[..], &location.to_bytes()[..], &hlc_bytes(Hlc::ZERO)[..]],
        )?;
        tx.execute_batch("PRAGMA user_version = 1")?;
        proceed(faults, Point::Migrating)?;
        tx.commit()?;""",
        """        tx.execute_batch(V1)?;
        tx.commit()?;
        proceed(faults, Point::Migrating)?;
        db.execute(
            "INSERT INTO store (singleton, device, location, clock) VALUES (1, ?1, ?2, ?3)",
            params![&device.to_bytes()[..], &location.to_bytes()[..], &hlc_bytes(Hlc::ZERO)[..]],
        )?;
        db.execute_batch("PRAGMA user_version = 1")?;""",
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
        schema::store_clock(&tx, self.writer.latest_hlc())?;
        proceed(&mut *self.faults, Point::Committing)?;
        tx.commit().map_err(StoreError::from)?;
        // The write has happened: only a crash can interrupt the store here.
        let _ = self.faults.proceed(Point::Committed);
        Ok(value)""",
        """        let value = f(&mut writing);
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
        };
        let value = f(&mut writing)?;
        schema::store_clock(&tx, self.writer.latest_hlc())?;""",
        """        proceed(&mut *self.faults, Point::Began)?;
        schema::store_clock(&tx, self.writer.latest_hlc())?;
        let mut writing = Writing {
            tx: &tx,
            writer: &mut self.writer,
            faults: &mut *self.faults,
            location: self.location,
        };
        let value = f(&mut writing)?;""",
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
        """        let event = pending.commit();
        proceed(self.faults, Point::Stored)?;""",
        "        let event = pending.commit();",
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
        "src/store.rs",
        "             ORDER BY hlc, origin_device, origin_seq\",",
        "             ORDER BY arrival\",",
    ),
    (
        "ties in a stream are broken by the later device first",
        "src/store.rs",
        "             ORDER BY hlc, origin_device, origin_seq\",",
        "             ORDER BY hlc, origin_device DESC, origin_seq\",",
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
]
