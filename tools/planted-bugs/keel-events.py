"""Planted bugs for keel-events: see run.py."""

BUGS = [
    # Canonical CBOR.
    (
        "decoder accepts a non-shortest one-byte argument",
        "src/cbor.rs",
        "if byte < 24 { Err(not_shortest) } else { Ok(u64::from(byte)) }",
        "Ok(u64::from(byte))",
    ),
    (
        "decoder accepts non-shortest 8-byte arguments",
        "src/cbor.rs",
        "if value <= 0xFFFF_FFFF { Err(not_shortest) } else { Ok(value) }",
        "Ok(value)",
    ),
    (
        "decoder accepts unsorted map keys",
        "src/cbor.rs",
        """                            core::cmp::Ordering::Less => {
                                return Err(error_at(key_start, CborErrorKind::UnsortedKeys));
                            }""",
        "                            core::cmp::Ordering::Less => {}",
    ),
    (
        "decoder accepts duplicate map keys",
        "src/cbor.rs",
        """                            core::cmp::Ordering::Equal => {
                                return Err(error_at(key_start, CborErrorKind::DuplicateKey));
                            }""",
        "                            core::cmp::Ordering::Equal => {}",
    ),
    (
        "decoder accepts trailing bytes",
        "src/cbor.rs",
        "    if reader.position == bytes.len() {",
        "    if reader.position <= bytes.len() {",
    ),
    (
        "encoder writes maps in insertion order",
        "src/cbor.rs",
        """        keyed.sort_by(|left, right| left.0.cmp(&right.0));
""",
        "",
    ),
    (
        "decoder skips UTF-8 validation",
        "src/cbor.rs",
        """                String::from_utf8(bytes)
                    .map(Value::Text)""",
        """                Ok::<_, ()>(String::from_utf8_lossy(&bytes).into_owned())
                    .map(Value::Text)""",
    ),
    # Keys, signatures and COSE.
    (
        "verify accepts high-S ECDSA",
        "src/keys.rs",
        """                if signature.normalize_s() != signature {
                    return Err(SignatureError::NotLowS);
                }
""",
        "",
    ),
    (
        "signer skips low-S normalization",
        "src/keys.rs",
        """                Ok(Signature(signature.normalize_s().to_bytes().to_vec()))
            }
            SigningKey::EdDsa""",
        """                Ok(Signature(signature.to_bytes().to_vec()))
            }
            SigningKey::EdDsa""",
    ),
    # Five byte-level checks of COSE messages and keys are the known-answer tests' to catch
    # (marked "unit"). The property tests sign and verify with the same code, so they can't see
    # a Sig_structure or a key identifier computed wrongly on both sides; only vectors that
    # Python's pycose checked can. A key identifier that doesn't match is refused anyway, for its
    # signature; and no generator makes an unprotected header or a weak Ed25519 key.
    (
        "COSE verify ignores the key identifier",
        "src/cose.rs",
        "if key.algorithm() != self.algorithm || key.key_id() != self.key_id {",
        "if key.algorithm() != self.algorithm {",
        "unit",
    ),
    (
        "Sig_structure omits the protected header",
        "src/cose.rs",
        """        Value::from(protected),
        Value::Bytes(Vec::new()),""",
        """        Value::Bytes(Vec::new()),
        Value::Bytes(Vec::new()),""",
        "unit",
    ),
    (
        "decoder tolerates an unprotected header",
        "src/cose.rs",
        "        if !unprotected.as_map().is_some_and(Map::is_empty) {",
        "        if unprotected.as_map().is_none() {",
        "unit",
    ),
    (
        "key id hashes the raw key bytes, not the COSE_Key",
        "src/keys.rs",
        "KeyId(sha256(&Value::Map(self.cose_key()).encode()))",
        "KeyId(sha256(&self.to_bytes()))",
        "unit",
    ),
    (
        "Ed25519 uses lax verification",
        "src/keys.rs",
        "key.verify_strict(message, &signature)",
        "key.verify(message, &signature)",
    ),
    (
        "weak Ed25519 keys accepted",
        "src/keys.rs",
        """        if key.is_weak() {
            return Err(invalid);
        }
""",
        "",
        "unit",
    ),
    # The envelope.
    (
        "encode drops the correlation",
        "src/envelope.rs",
        """            entries.push((key::CORRELATION, id_value(correlation)));
""",
        "",
    ),
    (
        "encode swaps actor kinds",
        "src/envelope.rs",
        "Actor::TeamMember(id) => (Actor::TEAM_MEMBER, id_value(*id)),",
        "Actor::TeamMember(id) => (Actor::CUSTOMER, id_value(*id)),",
    ),
    (
        "decode skips the format check",
        "src/envelope.rs",
        """        if format != Some(FORMAT) {
            return Err(EnvelopeError::UnsupportedFormat);
        }
""",
        "",
    ),
    (
        "decode ignores unknown keys",
        "src/envelope.rs",
        """            let slot = key
                .as_u64()
                .and_then(|key| usize::try_from(key).ok())
                .and_then(|index| fields.get_mut(index))
                .ok_or_else(|| EnvelopeError::UnknownField(key.to_string()))?;
            *slot = Some(value);""",
        """            let Some(slot) = key
                .as_u64()
                .and_then(|key| usize::try_from(key).ok())
                .and_then(|index| fields.get_mut(index))
            else {
                continue;
            };
            *slot = Some(value);""",
    ),
    (
        "decode reads negative keys as their magnitude",
        "src/envelope.rs",
        """            let slot = key
                .as_u64()
""",
        """            let slot = key
                .as_i64()
                .map(i64::unsigned_abs)
""",
    ),
    (
        "schema version 0 read as 1",
        "src/envelope.rs",
        """            .and_then(core::num::NonZeroU32::new)
""",
        """            .and_then(|version| core::num::NonZeroU32::new(version.max(1)))
""",
    ),
    (
        "schema version truncated to 32 bits",
        "src/envelope.rs",
        """            .and_then(|version| u32::try_from(version).ok())
""",
        """            .map(|version| version as u32)
""",
    ),
    (
        "origin sequence 0 read as 1",
        "src/envelope.rs",
        """                .and_then(NonZeroU64::new)
""",
        """                .and_then(|seq| NonZeroU64::new(seq.max(1)))
""",
    ),
    (
        "HLC read as a signed integer",
        "src/envelope.rs",
        """                required(key::HLC, "hybrid logical clock")?
                    .as_u64()
""",
        """                required(key::HLC, "hybrid logical clock")?
                    .as_i64()
                    .map(|n| n as u64)
""",
    ),
    (
        "business date trimmed before parsing",
        "src/envelope.rs",
        """        let business_date = text(required(key::BUSINESS_DATE, "business date")?, "business date")?
            .parse()""",
        """        let business_date = text(required(key::BUSINESS_DATE, "business date")?, "business date")?
            .trim()
            .parse()""",
    ),
    (
        "actor accepts extra elements",
        "src/envelope.rs",
        "let Some([kind, id_or_name]) = value.as_array() else {",
        "let Some([kind, id_or_name, ..]) = value.as_array() else {",
    ),
    (
        "payload not validated",
        "src/envelope.rs",
        """        cbor::decode(&bytes)?;
""",
        "",
    ),
    (
        "previous hash of any length",
        "src/envelope.rs",
        """            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .map(EventHash::from_bytes)""",
        """            .map(|bytes| { let mut h = [0_u8; 32]; h.iter_mut().zip(bytes).for_each(|(d, s)| *d = *s); h })
            .map(EventHash::from_bytes)""",
    ),
    (
        "null optional read as absent",
        "src/envelope.rs",
        "            approval: field(key::APPROVAL).map(",
        "            approval: field(key::APPROVAL).filter(|value| **value != Value::Null).map(",
    ),
    (
        "stream kind limit off by one",
        "src/envelope.rs",
        "if is_identifier(kind) && kind.len() <= 32 {",
        "if is_identifier(kind) && kind.len() < 32 {",
    ),
    (
        "stream kind limit missing",
        "src/envelope.rs",
        "if is_identifier(kind) && kind.len() <= 32 {",
        "if is_identifier(kind) {",
    ),
    (
        "schema name limit off by one",
        "src/envelope.rs",
        "if name.len() <= 64 && name.split('.').all(is_identifier) {",
        "if name.len() < 64 && name.split('.').all(is_identifier) {",
    ),
    (
        "schema name allows empty segments",
        "src/envelope.rs",
        "if name.len() <= 64 && name.split('.').all(is_identifier) {",
        "if name.len() <= 64 && !name.is_empty() && name.split('.').filter(|part| !part.is_empty()).all(is_identifier) {",
    ),
    (
        "identifiers may start with an underscore",
        "src/envelope.rs",
        "bytes.next().is_some_and(|first| first.is_ascii_lowercase())",
        "bytes.next().is_some_and(|first| first.is_ascii_lowercase() || first == b'_')",
    ),
    (
        "identifiers may contain uppercase",
        "src/envelope.rs",
        "&& bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')",
        "&& bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')",
    ),
    (
        "received event hashes the whole message",
        "src/event.rs",
        """        let hash = EventHash::of(message.payload());
""",
        """        let hash = EventHash::of(bytes);
""",
    ),
    (
        "signed event hash skips the first byte",
        "src/event.rs",
        """        let hash = EventHash::of(&encoded);
""",
        """        let hash = EventHash::of(encoded.get(1..).unwrap_or_default());
""",
    ),
    (
        "received event is not verified",
        "src/event.rs",
        """        self.message.verify(key)?;
""",
        "",
    ),
    # The log and the device registry.
    (
        "link skips the previous-hash check",
        "src/log.rs",
        """        if body.prev_hash != self.hash {
            return Err(ChainError::BrokenLink { seq });
        }
""",
        "",
    ),
    (
        "link allows an equal HLC",
        "src/log.rs",
        "        if body.hlc <= self.hlc {",
        "        if body.hlc < self.hlc {",
    ),
    (
        "link skips the HLC check",
        "src/log.rs",
        """        if body.hlc <= self.hlc {
            return Err(ChainError::ClockRegressed { seq });
        }
""",
        "",
    ),
    (
        "link skips the gap check",
        "src/log.rs",
        "        if self.seq.checked_add(1) != Some(seq) {",
        "        if false && self.seq.checked_add(1) != Some(seq) {",
    ),
    (
        "link calls any event at the head a duplicate",
        "src/log.rs",
        "            return if event.hash() == self.hash {",
        "            return if true {",
    ),
    (
        "link calls the head earlier",
        "src/log.rs",
        "        if seq < self.seq {",
        "        if seq <= self.seq {",
    ),
    (
        "head of an event forgets its HLC",
        "src/log.rs",
        "LogHead { seq: body.origin_seq.get(), hash: event.hash(), hlc: body.hlc }",
        "LogHead { seq: body.origin_seq.get(), hash: event.hash(), hlc: Hlc::ZERO }",
    ),
    (
        "writer chains every event to zero",
        "src/log.rs",
        "            prev_hash: self.head.hash,",
        "            prev_hash: EventHash::ZERO,",
    ),
    (
        "commit doesn't move the head",
        "src/log.rs",
        """        self.writer.head = LogHead::of(&self.event);
""",
        "",
    ),
    (
        "prepare moves the head",
        "src/log.rs",
        """        let event = SignedEvent::sign(body, &self.signer)?;
""",
        """        let event = SignedEvent::sign(body, &self.signer)?;
        self.head = LogHead::of(&event);
""",
    ),
    (
        "writer ignores the latest HLC",
        "src/log.rs",
        "        let latest = config.latest_hlc.max(config.head.hlc);",
        "        let latest = config.head.hlc;",
    ),
    (
        "writer ignores the head's HLC",
        "src/log.rs",
        "        let latest = config.latest_hlc.max(config.head.hlc);",
        "        let latest = config.latest_hlc;",
    ),
    (
        "observe does nothing",
        "src/log.rs",
        "        self.clock.observe(remote, now)",
        "        Ok(remote)",
    ),
    (
        "registry skips the location check",
        "src/verify.rs",
        "        if body.location != record.location {",
        "        if false && body.location != record.location {",
    ),
    (
        "revocation trusts the cut without its hash",
        "src/verify.rs",
        """            let trusted = seq < revocation.after_seq
""",
        """            let trusted = seq <= revocation.after_seq
""",
    ),
    (
        "revocation distrusts the cut",
        "src/verify.rs",
        "                || (seq == revocation.after_seq && event.hash() == revocation.last_trusted);",
        "                || false;",
    ),
    (
        "revocation ignored",
        "src/verify.rs",
        "        if let Some(revocation) = record.revocation {",
        "        if let Some(revocation) = record.revocation.filter(|_| false) {",
    ),
    (
        "revocations loosen",
        "src/verify.rs",
        "            Some(existing) if existing.after_seq < revocation.after_seq => Ok(()),",
        "            Some(existing) if existing.after_seq > revocation.after_seq => Ok(()),",
    ),
    (
        "revocations unchecked",
        "src/verify.rs",
        "        if (revocation.after_seq == 0) != (revocation.last_trusted == EventHash::ZERO) {",
        "        if false {",
    ),
    (
        "enrolling again replaces",
        "src/verify.rs",
        "            Some(_) => Err(RegistryError::AlreadyEnrolled(device)),",
        "            Some(_) => { self.devices.insert(device, DeviceRecord { location, key, revocation: None }); Ok(()) }",
    ),
    # The writer, restored and resumed as a store keeps it.
    (
        "restore leaves the clock behind the head",
        "src/log.rs",
        "self.clock = HlcClock::resume(latest_hlc.max(head.hlc), self.max_forward_drift);",
        "self.clock = HlcClock::resume(latest_hlc, self.max_forward_drift);",
    ),
    (
        "restore keeps the writer's clock",
        "src/log.rs",
        "self.clock = HlcClock::resume(latest_hlc.max(head.hlc), self.max_forward_drift);",
        "self.clock = HlcClock::resume(self.clock.last().max(head.hlc), self.max_forward_drift);",
    ),
    (
        "restore keeps the writer's head",
        "src/log.rs",
        """        self.head = head;
        self.clock = HlcClock::resume""",
        "        self.clock = HlcClock::resume",
    ),
    (
        "restore forgets the drift limit",
        "src/log.rs",
        "self.clock = HlcClock::resume(latest_hlc.max(head.hlc), self.max_forward_drift);",
        "self.clock = HlcClock::resume(latest_hlc.max(head.hlc), Duration::MAX);",
    ),
    (
        "the latest HLC is the head's",
        "src/log.rs",
        """    pub const fn latest_hlc(&self) -> Hlc {
        self.clock.last()""",
        """    pub const fn latest_hlc(&self) -> Hlc {
        self.head.hlc""",
    ),
    (
        "a head rebuilt from its parts loses its HLC",
        "src/log.rs",
        "        LogHead { seq, hash, hlc }",
        "        LogHead { seq, hash, hlc: Hlc::ZERO }",
    ),
    # The size limit (ADR-0019). Only the unit tests make events near 256 KiB, and pin the limit
    # to the byte, at the writer and at the registry.
    (
        "the writer makes events over the size limit",
        "src/log.rs",
        """        if event.to_bytes().len() > MAX_EVENT_BYTES {
            return Err(AppendError::TooLarge);
        }
""",
        "",
        "unit",
    ),
    (
        "the writer's size limit is a byte short",
        "src/log.rs",
        "        if event.to_bytes().len() > MAX_EVENT_BYTES {",
        "        if event.to_bytes().len() >= MAX_EVENT_BYTES {",
        "unit",
    ),
    (
        "the registry verifies events over the size limit",
        "src/verify.rs",
        """        if bytes.len() > MAX_EVENT_BYTES {
            return Err(Rejection::TooLarge);
        }
""",
        "",
        "unit",
    ),
    (
        "the registry's size limit is a byte short",
        "src/verify.rs",
        "        if bytes.len() > MAX_EVENT_BYTES {",
        "        if bytes.len() >= MAX_EVENT_BYTES {",
        "unit",
    ),
    (
        "identifiers for aggregates repeat",
        "src/log.rs",
        """        self.ids.generate(now)
    }""",
        """        let _ = now;
        Id::parse("0192f0c1-0000-7000-8000-000000000001").map_err(|_| IdError::Exhausted)
    }""",
    ),
]
