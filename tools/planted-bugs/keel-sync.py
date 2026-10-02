"""Planted bugs for keel-sync: see run.py.

The property tests here are the protocol property, which runs replicas of a model store over a
faulty network, several of them candidates for the Store Hub, and checks the protocol's rules
frame by frame, with its named regression tests; and keel-sim's seeds, which run real stores
through crashes, rollbacks, splits and clock jumps and check the rules as they go. The simulator
runs 32 seeds by default; `KEEL_SIM_SEEDS` sets more.
"""

# keel-sim's simulations exercise the replicator with real stores: their tests run with ours.
ALSO = ["keel-sim"]

# Known answers, frame by frame, live in tests/ beside the property test, since they share its
# model replica; --props leaves them out.
KNOWN_ANSWERS = ["election", "replicator", "store_replica"]

BUGS = [
    # Frames. Replicas encode frames from their own state, which is always well formed, so the
    # strictness of decoding shows only in frames made by hand: the unit tests' refused frames.
    (
        "a have lists devices with nothing held",
        "src/frame.rs",
        "            .filter(|(_, position)| **position > 0)\n",
        "",
        "unit",  # A replica's version vector never lists a device it holds nothing of.
    ),
    (
        "a version vector may repeat a device or list one out of order",
        "src/frame.rs",
        """        if vv.last_key_value().is_some_and(|(last, _)| *last >= device) {
            return Err(FrameError::Malformed);
        }
""",
        "",
        "unit",  # Replicas encode version vectors from ordered maps.
    ),
    (
        "a position of 0 is accepted",
        "src/frame.rs",
        "let position = position.as_u64().filter(|position| *position > 0);",
        "let position = position.as_u64();",
        "unit",  # Replicas never send a position of 0.
    ),
    (
        "any protocol version is spoken",
        "src/frame.rs",
        """        if version != PROTOCOL {
            return Err(FrameError::Version(version));
        }
""",
        "",
        "unit",  # Every replica in the tests speaks version 1.
    ),
    (
        "frames over the limit are decoded",
        "src/frame.rs",
        """        if bytes.len() > MAX_FRAME {
            return Err(FrameError::TooLarge);
        }
""",
        "",
        "unit",  # Batches stay far below the limit, as they must.
    ),
    (
        "a batch's number decodes as 0",
        "src/frame.rs",
        "batch: batch.as_u64().ok_or(FrameError::Malformed)?,",
        "batch: batch.as_u64().map(|_| 0).ok_or(FrameError::Malformed)?,",
    ),
    (
        "a have's version vector decodes as empty",
        "src/frame.rs",
        "vv: version_vector(vv)?,",
        "vv: version_vector(vv).map(|_| VersionVector::new())?,",
    ),
    (
        "a have's acknowledgement decodes as 0",
        "src/frame.rs",
        "acked: acked.as_u64().ok_or(FrameError::Malformed)?,",
        "acked: acked.as_u64().map(|_| 0).ok_or(FrameError::Malformed)?,",
    ),
    (
        "a have never asks, once decoded",
        "src/frame.rs",
        "asks: asks.as_bool().ok_or(FrameError::Malformed)?,",
        "asks: asks.as_bool().map(|_| false).ok_or(FrameError::Malformed)?,",
    ),
    # Frames received.
    (
        "frames for another location are taken",
        "src/replicator.rs",
        "            Ok(Frame::Have(have)) if have.location == self.location => {",
        "            Ok(Frame::Have(have)) => {",
        "unit",  # Every replica in the tests keeps one location's events.
    ),
    (
        "frames from strangers are taken",
        "src/replicator.rs",
        "            Ok(_) if !self.peers.contains_key(&from) => Ok(self.drop_frame()),\n",
        "",
        "unit",  # Frames in the tests come only from peers.
    ),
    (
        "a replica is its own peer",
        "src/replicator.rs",
        "            .filter(|peer| *peer != device)\n",
        "",
        "unit",  # No test but the known answer lists a replica among its own peers.
    ),
    # Asking for a have.
    (
        "a have that asks goes unanswered",
        "src/replicator.rs",
        """        if have.asks {
            outgoing.push(self.have(from));""",
        """        if have.asks {
            let _ = self.have(from);""",
    ),
    (
        "a have always asks",
        "src/replicator.rs",
        "        let asks = peer.is_some_and(|peer| peer.known.is_none());",
        "        let asks = peer.is_some();",
    ),
    (
        "a have never asks",
        "src/replicator.rs",
        "        let asks = peer.is_some_and(|peer| peer.known.is_none());",
        "        let asks = peer.is_none();",
    ),
    # Acknowledgements and stalls.
    (
        "a batch is acknowledged by any have",
        "src/replicator.rs",
        """        if let Some(waiting) = &peer.waiting
            && waiting.batch == have.acked
        {""",
        "        if let Some(waiting) = &peer.waiting {",
    ),
    (
        "a batch the peer took none of doesn't stall",
        "src/replicator.rs",
        "            if !took_some {",
        "            if !took_some && waiting.batch == 0 {",
    ),
    (
        "a batch counts as taken if the peer holds any of its devices",
        "src/replicator.rs",
        ".any(|(device, first)| have.vv.get(device).is_some_and(|held| held >= first));",
        ".any(|(device, _)| have.vv.contains_key(device));",
    ),
    (
        "what a peer holds is what it said, merged with what it said before",
        "src/replicator.rs",
        "        peer.known = Some(have.vv);",
        """        let mut vv = have.vv;
        for (device, held) in peer.known.take().unwrap_or_default() {
            let position = vv.entry(device).or_insert(0);
            *position = (*position).max(held);
        }
        peer.known = Some(vv);""",
    ),
    (
        "a batch received isn't acknowledged",
        "src/replicator.rs",
        "            peer.received = batch.batch;\n",
        "",
    ),
    (
        "acknowledgements hold off the rounds",
        "src/replicator.rs",
        "            peer.received = batch.batch;\n",
        """            peer.received = batch.batch;
            peer.round_at = now;
""",
    ),
    (
        "events received are sent back to their sender",
        "src/replicator.rs",
        """                    if let Some(known) = self.peers.get_mut(&from).and_then(|p| p.known.as_mut()) {
                        let held = known.entry(device).or_insert(0);
                        *held = (*held).max(position);
                    }
""",
        "",
    ),
    (
        "events received aren't passed on until a round",
        "src/replicator.rs",
        """        if stored_any {
            outgoing.extend(self.push_all(replica, now)?);
        }""",
        """        if stored_any {
            outgoing.extend(self.push(replica, from, now)?);
        }""",
    ),
    # Keeping in step with the store. Only something writing to the store behind the
    # replicator's back puts it out of step, which neither the property nor the simulator does.
    (
        "a replicator out of step with its store stays so after receiving",
        "src/replicator.rs",
        """        if !in_step {
            self.ours = replica.version_vector()?;
        }
        if claims {""",
        """        if claims {""",
        "unit",
    ),
    (
        "a replicator out of step with its store stays so after appending",
        "src/replicator.rs",
        """        if !in_step {
            self.ours = replica.version_vector()?;
        }
        if events.iter().any(is_claim) {""",
        "        if events.iter().any(is_claim) {",
        "unit",
    ),
    (
        "any later position counts as the next",
        "src/replicator.rs",
        "        if held.checked_add(1) == Some(position) {",
        "        if position > held {",
        "unit",
    ),
    # Settling.
    (
        "a device's log is settled from the start",
        "src/replicator.rs",
        """            peers,
            settled: false,""",
        """            peers,
            settled: true,""",
    ),
    (
        "a log settles only once a peer holds less of it",
        "src/replicator.rs",
        "Some(known) if known.get(&self.device).copied().unwrap_or(0) > own => return,",
        "Some(known) if known.get(&self.device).copied().unwrap_or(0) >= own => return,",
    ),
    (
        "a log settles when a peer holds more of it",
        "src/replicator.rs",
        "Some(known) if known.get(&self.device).copied().unwrap_or(0) > own => return,",
        "Some(known) if known.get(&self.device).copied().unwrap_or(0) < own => return,",
    ),
    (
        "a log settles against the first peer to answer",
        "src/replicator.rs",
        "        self.settled = all || waited;",
        "        self.settled = all || waited || !all;",
    ),
    (
        "a log waits for every peer to answer",
        "src/replicator.rs",
        "        self.settled = all || waited;",
        "        self.settled = all;",
    ),
    # Sending.
    (
        "a batch goes while another waits",
        "src/replicator.rs",
        "        if peer.waiting.is_some() || peer.stalled {",
        "        if peer.stalled {",
    ),
    (
        "a stalled peer is sent batches",
        "src/replicator.rs",
        "        if peer.waiting.is_some() || peer.stalled {",
        "        if peer.waiting.is_some() {",
    ),
    (
        "batch numbers repeat",
        "src/replicator.rs",
        "        peer.batches = peer.batches.wrapping_add(1);\n",
        "",
    ),
    (
        "batches are numbered from 0 at every start",
        "src/replicator.rs",
        "                    batches: first,",
        "                    batches: 0,",
    ),
    (
        "batches are numbered from the clock's reading at every start",
        "src/replicator.rs",
        "        let first = nonce >> 1;",
        "        let first = u64::try_from(now.as_micros()).unwrap_or(nonce & 0);",
    ),
    (
        "a batch starts at the peer's last event",
        "src/replicator.rs",
        "        for event in replica.events_after(lag.device, lag.theirs, limit)? {",
        "        for event in replica.events_after(lag.device, lag.theirs.saturating_sub(1), limit)? {",
    ),
    (
        "a batch skips the event after the peer's",
        "src/replicator.rs",
        "        for event in replica.events_after(lag.device, lag.theirs, limit)? {",
        "        for event in replica.events_after(lag.device, lag.theirs.saturating_add(1), limit)? {",
    ),
    (
        "the first device lagging takes the whole batch",
        "src/replicator.rs",
        "            let share = room.div_ceil(count).max(1);",
        "            let share = room;",
        "unit",  # How a batch is shared is fairness, which no property bounds.
    ),
    (
        "a peer's own log takes a share like any other's",
        "src/replicator.rs",
        "            .partition(|lag| own_first && lag.device == to);",
        "            .partition(|lag| own_first && lag.device == to && lag.device != to);",
        # Only how soon a device restored from an older copy settles changes: the simulator
        # counts the devices that fork for want of their own log, but can't fail on them.
        "unit",
    ),
    (
        "a peer's own log it refused still comes first",
        "src/replicator.rs",
        """            if let Some(first) = waiting.first.get(&from) {
                peer.own_refused = have.vv.get(&from).is_none_or(|held| held < first);
            }
""",
        "",
        # Only a device that forked its log refuses it, and its own log fills every batch it is
        # sent in about 1 seed in 1,000 (seed 1645): too rarely for a planted run's 32 seeds.
        # The named regression test pins it.
        "unit",
    ),
    (
        "a batch holds one event too many",
        "src/replicator.rs",
        """        let taken = u32::try_from(batch.events.len()).unwrap_or(u32::MAX);
        let room = self.config.batch_events.saturating_sub(taken);
        let lagging""",
        """        let taken = u32::try_from(batch.events.len()).unwrap_or(u32::MAX);
        let room = self.config.batch_events.saturating_sub(taken).saturating_add(1);
        let lagging""",
    ),
    (
        "the byte budget admits an event too many",
        "src/replicator.rs",
        "                && batch.bytes.saturating_add(size) > self.config.batch_bytes",
        "                && batch.bytes > self.config.batch_bytes",
    ),
    (
        "an event larger than the byte budget is never sent",
        "src/replicator.rs",
        """            if !batch.events.is_empty()
                && batch.bytes.saturating_add(size) > self.config.batch_bytes""",
        """            if batch.bytes.saturating_add(size) > self.config.batch_bytes""",
    ),
    # Time.
    (
        "a batch waiting is never taken as lost",
        "src/replicator.rs",
        "            if peer.waiting.as_ref().is_some_and(|w| elapsed(w.sent, now, self.config.ack_timeout))",
        "            if peer.waiting.as_ref().is_some_and(|w| elapsed(w.sent, now, self.config.ack_timeout) && w.batch == 0)",
    ),
    (
        "rounds don't end a stall",
        "src/replicator.rs",
        """                peer.round_at = now;
                peer.stalled = false;
""",
        """                peer.round_at = now;
""",
    ),
    (
        "rounds tell the peer nothing",
        "src/replicator.rs",
        """                peer.stalled = false;
                outgoing.push(self.have(to));
""",
        """                peer.stalled = false;
""",
    ),
    (
        "a replica repeats itself only a round apart before hearing from a peer",
        "src/replicator.rs",
        "        if self.known.is_none() { config.ack_timeout } else { config.round }",
        "        config.round",
        "unit",  # Only how soon a lost `have` is told again changes, which no property bounds.
    ),
    (
        "a clock set back holds off the rounds",
        "src/replicator.rs",
        "    now.duration_since(since).is_none_or(|passed| passed >= span)",
        "    now.duration_since(since).is_some_and(|passed| passed >= span)",
        # A clock set back holds off rounds and timeouts for as long as it was set back. The
        # simulator's clocks jump back at most 10 s, which only delays agreement; the known
        # answer sets a clock back further than the time since the last round.
        "unit",
    ),
    (
        "the next tick forgets acknowledgement timeouts",
        "src/replicator.rs",
        "                [round, timeout, heartbeat]",
        "                [round, timeout.filter(|_| false), heartbeat]",
    ),
    (
        "the next tick forgets rounds",
        "src/replicator.rs",
        "                [round, timeout, heartbeat]",
        "                [round.filter(|_| false), timeout, heartbeat]",
    ),
    (
        "the next tick forgets heartbeats",
        "src/replicator.rs",
        "                [round, timeout, heartbeat]",
        "                [round, timeout, heartbeat.filter(|_| false)]",
    ),
    # The store adapter.
    (
        "the store adapter skips an event",
        "src/replica.rs",
        "Ok(self.store.log(device, after, limit)?.iter().map(SignedEvent::to_bytes).collect())",
        "Ok(self.store.log(device, after.saturating_add(1), limit)?.iter().map(SignedEvent::to_bytes).collect())",
    ),
    (
        "the store adapter stores each event in a write of its own",
        "src/replica.rs",
        "        self.store.write(|w| events.iter().map(|bytes| w.receive(bytes, registry, now)).collect())",
        "        events.iter().map(|bytes| self.store.write(|w| w.receive(bytes, registry, now))).collect()",
        # A batch half stored is still every log in order, and the simulator restarts a node
        # whose write fails, so only the known answer, which interrupts a write, can tell.
        "unit",
    ),
    # Sequencing, store durability and the durable-ack watermark (ADR-0020).
    (
        "sequencing: the hub numbers before its log is settled, or once it forked",
        "src/replicator.rs",
        """        if !self.serving() {
            return Ok(Vec::new());
        }
        let mut written""",
        """        if !self.is_hub() {
            return Ok(Vec::new());
        }
        let mut written""",
    ),
    (
        "sequencing: the hub doesn't number what it receives",
        "src/replicator.rs",
        "        if stored_any || (self.serving() && !was_serving) {",
        "        if self.serving() && !was_serving {",
    ),
    (
        "sequencing: the hub doesn't number as it begins to serve on a have",
        "src/replicator.rs",
        """        if self.serving() && !was_serving {
            outgoing.extend(self.sequence(replica, now)?);
        }
        Ok(outgoing)""",
        """        Ok(outgoing)""",
    ),
    (
        "sequencing: the hub doesn't number as it begins to serve at a tick",
        "src/replicator.rs",
        """            if self.serving() && !was_serving {
                outgoing.extend(self.sequence(replica, now)?);
            }
            outgoing.extend(self.elect(replica, now)?);""",
        """            let _ = was_serving;
            outgoing.extend(self.elect(replica, now)?);""",
    ),
    (
        "sequencing: the hub doesn't number its own events",
        "src/replicator.rs",
        """        if !events.is_empty() {
            outgoing.extend(self.sequence(replica, now)?);
        }""",
        """        let _ = events.is_empty();""",
    ),
    (
        "sequencing: records aren't passed on",
        "src/replicator.rs",
        "        self.take_in(replica, &written, now)",
        "        let _ = written;\n        Ok(Vec::new())",
    ),
    (
        "store durability: the least any peer holds",
        "src/replicator.rs",
        """            .filter_map(|peer| peer.known.as_ref()?.get(&self.device).copied())
            .max()""",
        """            .filter_map(|peer| peer.known.as_ref()?.get(&self.device).copied())
            .min()""",
        # The simulator checks that no event a device was told is store-durable is lost, which a
        # smaller claim can't break, and a device there has one peer, the hub: only a unit test,
        # with two peers, sees the least claimed instead of the most.
        "unit",
    ),
    (
        "the watermark follows the last it heard",
        "src/replicator.rs",
        "            if position > *held {",
        "            if position != *held {",
    ),
    (
        "the watermark isn't passed on when it rises",
        "src/replicator.rs",
        "        if self.merge_durable(vv) { self.relay_durable(from) } else { Vec::new() }",
        "        let _ = (self.merge_durable(vv), from);\n        Vec::new()",
        # Each round passes it on anyway: only a unit test sees it held back until then.
        "unit",
    ),
    (
        "the watermark isn't sent with each round",
        "src/replicator.rs",
        "                outgoing.extend(self.durable_for(to));",
        "                let _ = to;",
    ),
    (
        "the watermark goes back to the durable peer",
        "src/replicator.rs",
        "        if self.durable.is_empty() || self.roles.durable == Some(to) {",
        "        if self.durable.is_empty() {",
    ),
    (
        "any peer's have is the watermark",
        "src/replicator.rs",
        "        if self.roles.durable == Some(from) && self.merge_durable(&have.vv) {",
        "        if self.merge_durable(&have.vv) {",
    ),
    (
        "a watermark from another location is taken",
        "src/replicator.rs",
        "            Ok(Frame::Durable(durable)) if durable.location == self.location => {",
        "            Ok(Frame::Durable(durable)) => {",
        # Every replica in the property test and the simulator is at one location.
        "unit",
    ),
    (
        "a have that asks isn't answered with the watermark",
        "src/replicator.rs",
        """            outgoing.push(self.have(from));
            outgoing.extend(self.durable_for(from));""",
        """            outgoing.push(self.have(from));""",
        # The next round brings it anyway: only a unit test sees it wait.
        "unit",
    ),
    (
        "a durable frame is an events frame",
        "src/frame.rs",
        "const DURABLE: u64 = 2;",
        "const DURABLE: u64 = 1;",
    ),
    # Ownership (ADR-0021): the hub answers requests for orders, then numbers what it wrote.
    (
        "ownership: the hub doesn't answer requests",
        "src/replicator.rs",
        "        let mut written = replica.answer_requests(now)?;",
        "        let mut written = Vec::new();",
    ),
    (
        "ownership: the hub numbers before it answers",
        "src/replicator.rs",
        """        let mut written = replica.answer_requests(now)?;
        written.extend(replica.sequence(now)?);""",
        """        let mut written = replica.sequence(now)?;
        written.extend(replica.answer_requests(now)?);""",
    ),
    (
        "ownership: the store never answers",
        "src/replica.rs",
        "        self.store.answer_requests(now)",
        "        let _ = now;\n        Ok(Vec::new())",
    ),
    # Heartbeats (ADR-0022): the frame, and what a replica says in it.
    (
        "heartbeats: a heartbeat is a durable frame",
        "src/frame.rs",
        "const HEARTBEAT: u64 = 3;",
        "const HEARTBEAT: u64 = 2;",
        # Both ends read the kind they write, and no other frame has a heartbeat's seven fields to
        # take it for: only the pinned bytes see the kind.
        "unit",
    ),
    (
        "heartbeats: acting decodes as false",
        "src/frame.rs",
        "let acting = acting.as_bool().ok_or(FrameError::Malformed)?;",
        "let acting = acting.as_bool().map(|_| false).ok_or(FrameError::Malformed)?;",
    ),
    (
        "heartbeats: a beat decodes as 0",
        "src/frame.rs",
        "beat if epoch > 0 => Some(beat.as_u64().ok_or(FrameError::Malformed)?),",
        "beat if epoch > 0 => Some(beat.as_u64().map(|_| 0).ok_or(FrameError::Malformed)?),",
    ),
    (
        "heartbeats: the hub is sent as the location",
        "src/frame.rs",
        "heartbeat.hub.map_or(Value::Null, |hub| Value::Bytes(hub.to_bytes().to_vec())),",
        "heartbeat.hub.map_or(Value::Null, |_| Value::Bytes(heartbeat.location.to_bytes().to_vec())),",
    ),
    (
        "heartbeats: a term without its hub is accepted",
        "src/frame.rs",
        "hub if epoch > 0 => Some(id(hub)?),",
        "hub if epoch > 0 => id(hub).ok(),",
        # Replicas send a term's hub with it: only the refused frames show it.
        "unit",
    ),
    (
        "heartbeats: a hub without a term is accepted",
        "src/frame.rs",
        "hub if epoch > 0 => Some(id(hub)?),",
        "hub => Some(id(hub)?),",
        # Replicas send no hub without a term: only the refused frames show it.
        "unit",
    ),
    (
        "heartbeats: a beat without a term is accepted",
        "src/frame.rs",
        "beat if epoch > 0 => Some(beat.as_u64().ok_or(FrameError::Malformed)?),",
        "beat => Some(beat.as_u64().ok_or(FrameError::Malformed)?),",
        # Replicas send no beat without a term: only the refused frames show it.
        "unit",
    ),
    (
        "heartbeats: a floor decodes as 0",
        "src/frame.rs",
        "let floor = floor.as_u64().ok_or(FrameError::Malformed)?;",
        "let floor = floor.as_u64().map(|_| 0).ok_or(FrameError::Malformed)?;",
    ),
    (
        "heartbeats: the beat is sent as the floor",
        "src/frame.rs",
        "                heartbeat.floor.map_or(Value::Null, Value::Unsigned),",
        "                heartbeat.beat.map_or(Value::Null, Value::Unsigned),",
    ),
    (
        "heartbeats: a floor without a term is accepted",
        "src/frame.rs",
        "                    floor if epoch > 0 => {",
        "                    floor => {",
        # Replicas send no floor without a term: only the refused frames show it.
        "unit",
    ),
    (
        "heartbeats: a beat without a floor is accepted",
        "src/frame.rs",
        "                    Value::Null if beat.is_none() => None,",
        "                    Value::Null => None,",
        "unit",  # A replica giving a beat always gives a floor no lower.
    ),
    (
        "heartbeats: a floor below the beat is accepted",
        "src/frame.rs",
        "                        if beat.is_some_and(|beat| beat > floor) {",
        "                        if beat.is_some_and(|beat| beat > floor) && floor == u64::MAX {",
        "unit",  # A replica's floor is never below the beat it gives.
    ),
    (
        "heartbeats: a priority decodes as 0",
        "src/frame.rs",
        ".and_then(|priority| u8::try_from(priority).ok())",
        ".and_then(|priority| u8::try_from(priority).ok().map(|_| 0))",
    ),
    (
        "heartbeats: acting without a beat is accepted",
        "src/frame.rs",
        "Value::Null if !acting => None,",
        "Value::Null => None,",
        "unit",  # A replica acting always gives its beat.
    ),
    (
        "heartbeats: an epoch past the largest is accepted",
        "src/frame.rs",
        ".filter(|&epoch| epoch <= MAX_EPOCH)",
        ".filter(|&epoch| epoch <= u64::MAX)",
        "unit",  # No claim's epoch passes the largest.
    ),
    (
        "heartbeats: none go out as a period begins",
        "src/replicator.rs",
        "            outgoing.extend(self.peers.keys().map(|&to| self.heartbeat(to)));\n",
        "",
    ),
    (
        "heartbeats: none go out at a start",
        "src/replicator.rs",
        ".flat_map(|&to| [replicator.have(to), replicator.heartbeat(to)])",
        ".flat_map(|&to| [replicator.have(to)])",
        # A start's heartbeat gives no priority, its log unsettled, and the next comes a period
        # later: only the known answer, which looks for it, sees it missing.
        "unit",
    ),
    (
        "heartbeats: the hub's beat doesn't rise",
        "src/replicator.rs",
        "                self.beat = reading.max(self.beat.saturating_add(1));",
        "                self.beat = self.beat.max(reading.min(1));",
    ),
    (
        "heartbeats: a hub's beat goes on above the beats it is given, not the floors",
        "src/replicator.rs",
        "                    self.beat = self.beat.max(heartbeat.floor.unwrap_or(0));",
        "                    self.beat = self.beat.max(heartbeat.beat.unwrap_or(0));",
    ),
    (
        "heartbeats: a hub's beat ignores the floors it is given",
        "src/replicator.rs",
        "                if !heartbeat.acting && heartbeat.hub == Some(self.device) {",
        "                if heartbeat.acting && heartbeat.hub == Some(self.device) {",
    ),
    (
        "heartbeats: the floor given is the beat",
        "src/replicator.rs",
        "        let floor = self.election.floor(term).max(beat);",
        "        let floor = beat;",
    ),
    (
        "heartbeats: a replica that can't be the hub now gives its priority",
        "src/replicator.rs",
        "        let eligible = self.settled && !self.forked();",
        "        let eligible = true;",
    ),
    (
        "heartbeats: a hub says it acts before it serves",
        "src/replicator.rs",
        "        let acting = self.serving();",
        "        let acting = self.is_hub();",
    ),
    (
        "heartbeats: a beat had through a peer is passed on",
        "src/election.rs",
        """        let (epoch, hub) = term?;
        let (of, heard) = self.peers.get(&hub)?.acting?;
        (of == epoch && self.is_recent(heard.period, silence)).then_some(heard.beat)""",
        """        let heard = self.latest.get(&term?)?;
        self.is_recent(heard.period, silence).then_some(heard.beat)""",
    ),
    (
        "heartbeats: a later term's beat is passed on as the replica's own term's",
        "src/election.rs",
        "(of == epoch && self.is_recent(heard.period, silence)).then_some(heard.beat)",
        "(of >= epoch && self.is_recent(heard.period, silence)).then_some(heard.beat)",
    ),
    # The election (ADR-0022).
    (
        "election: something heard stays recent a period too long",
        "src/election.rs",
        "        self.period.saturating_sub(period) <= silence",
        "        self.period.saturating_sub(period) <= silence.saturating_add(1)",
    ),
    (
        "election: something heard stays recent a period too short",
        "src/election.rs",
        "        self.period.saturating_sub(period) <= silence",
        "        self.period.saturating_sub(period) < silence",
    ),
    (
        "election: a beat heard again counts as new",
        "src/election.rs",
        "        Some(heard) if heard.beat >= beat => heard,",
        "        Some(heard) if heard.beat > beat => heard,",
    ),
    (
        "election: another hub of its epoch counts",
        "src/election.rs",
        "term.is_none_or(|(own, of)| epoch > own || (epoch == own && hub == of))",
        "term.is_none_or(|(own, _)| epoch >= own)",
    ),
    (
        "election: the beats of two hubs of one epoch are compared",
        "src/election.rs",
        "        let heard = later(self.latest.get(&term).copied(), beat, period);",
        """        let rivals = self.latest.iter().filter(|(term, _)| term.0 == heartbeat.epoch);
        let heard = later(rivals.map(|(_, heard)| *heard).max_by_key(|heard| heard.beat), beat, period);""",
    ),
    (
        "election: a heartbeat acting as another device's hub gives a beat",
        "src/election.rs",
        """        if heartbeat.acting && hub != from {
            return;
        }
""",
        "",
        # No replica says it acts as another's hub: only a frame made up for a known answer can.
        "unit",
    ),
    (
        "election: a peer's floor isn't kept",
        "src/election.rs",
        "        let known = heartbeat.floor.max(heartbeat.beat);",
        "        let known = heartbeat.beat;",
    ),
    (
        "election: a floor heard replaces a later one",
        "src/election.rs",
        "            *floor = (*floor).max(known);",
        "            *floor = known;",
    ),
    (
        "election: a replica gives the latest floor of any term",
        "src/election.rs",
        "        self.floors.get(&term?).copied()",
        "        term.and(self.floors.values().copied().max())",
    ),
    (
        "election: learning of a claim isn't hearing its hub",
        "src/election.rs",
        "        self.learned = Some(self.period);",
        "        self.learned = None;",
    ),
    (
        "election: a lower priority is preferred",
        "src/election.rs",
        "                && (peer.priority > priority || (peer.priority == priority && from < own))",
        "                && (peer.priority < priority || (peer.priority == priority && from < own))",
    ),
    (
        "election: of one priority, the higher device is preferred",
        "src/election.rs",
        "                && (peer.priority > priority || (peer.priority == priority && from < own))",
        "                && (peer.priority > priority || (peer.priority == priority && from > own))",
    ),
    (
        "election: a replica claims before it has listened for the periods of silence",
        "src/replicator.rs",
        "            && self.election.period() >= silence\n",
        "",
    ),
    (
        "election: a replica claims while it hears a hub",
        "src/replicator.rs",
        "            && !self.election.hears_hub(self.heard_term(), silence)\n",
        "",
    ),
    (
        "election: a replica claims while it hears a preferred candidate",
        "src/replicator.rs",
        "\n            && !self.election.hears_preferred(self.device, priority.get(), silence);",
        ";",
    ),
    (
        "election: a replica claims unsettled",
        "src/replicator.rs",
        """        let ready = !self.is_hub()
            && self.settled""",
        """        let ready = !self.is_hub()""",
    ),
    (
        "election: a replica whose log forked claims",
        "src/replicator.rs",
        """            && self.settled
            && !self.forked()""",
        """            && self.settled""",
    ),
    (
        "election: a candidate doesn't wait to catch up",
        "src/replicator.rs",
        "        if self.caught_up() {",
        "        if self.caught_up() || self.settled {",
        # Claiming before catching up only cuts what peers already hold, whose records stop
        # counting and whose events are numbered again: only the known answer sees it.
        "unit",
    ),
    (
        "election: a candidate waits for ever to catch up",
        "src/election.rs",
        "        self.period.saturating_sub(since) >= silence",
        "        self.period.saturating_sub(since) >= u64::MAX",
        # Once the network heals, every candidate catches up: only the known answer, which
        # keeps one behind, sees it wait.
        "unit",
    ),
    (
        "election: a candidate waits on the silent hub's word of its own log",
        "src/replicator.rs",
        "                .filter(|(peer, _)| **peer != term.device)\n",
        "",
    ),
    (
        "election: a claim isn't sent",
        "src/replicator.rs",
        "                let mut outgoing = self.take_in(replica, &[*claim], now)?;",
        "                let mut outgoing = Vec::new();\n                let _ = claim;",
    ),
    (
        "election: a hub doesn't hear of the claim that deposes it",
        "src/replicator.rs",
        """        if claims {
            self.read_terms(replica)?;
        }""",
        """        let _ = claims;""",
    ),
    (
        "election: periods are counted off the clock",
        "src/replicator.rs",
        """            self.heartbeat_at = now;
            self.election.tick(self.epoch());""",
        """            let missed = now.duration_since(self.heartbeat_at).map_or(1, |passed| passed.as_secs());
            self.heartbeat_at = now;
            for _ in 0..missed.max(1) {
                self.election.tick(self.epoch());
            }""",
    ),
    # Forks (ADR-0019, ADR-0022): a replica whose own log a peer refuses can't be the hub.
    (
        "forks: a gap counts as a refusal",
        "src/replicator.rs",
        "&& have.vv.get(&self.device).copied().unwrap_or(0).checked_add(1) == Some(first)",
        "&& have.vv.get(&self.device).copied().unwrap_or(0) < first",
        # Replicas here never lose events, so a batch never comes after a gap in what a peer
        # holds: only the known answer sees a gap taken for a refusal.
        "unit",
    ),
    (
        "forks: a refusal ends at once",
        "src/replicator.rs",
        "        if peer.refused_ours.is_some_and(|refused| held >= refused) {",
        "        if peer.refused_ours.is_some() {",
    ),
    (
        "forks: a refusal never ends",
        "src/replicator.rs",
        "        if peer.refused_ours.is_some_and(|refused| held >= refused) {",
        "        if peer.refused_ours.is_some_and(|refused| held >= refused && refused == 0) {",
    ),
    (
        "forks: a peer refusing its own log counts as refusing the replica's",
        "src/replicator.rs",
        "            if let Some(&first) = waiting.first.get(&self.device)",
        "            if let Some(&first) = waiting.first.get(&from)",
    ),
    (
        "forks: a hub whose log forked goes on serving",
        "src/replicator.rs",
        "        self.settled && !self.forked() && self.is_hub()",
        "        self.settled && self.is_hub()",
    ),
]
