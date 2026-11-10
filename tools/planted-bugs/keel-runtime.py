"""Planted bugs for keel-runtime: see run.py."""

# Integration tests of known answers, which --props leaves out.
KNOWN_ANSWERS = ["end_to_end", "publication", "sync"]

# The cell's ingest test runs a location with its cloud end to end, through the runtime: the
# durable peer's naming is seen there alone.
ALSO = ["keel-cloud"]

BUGS = [
    # Signing in, and what every event records.
    (
        "runtime: anyone may sign in",
        "src/runtime.rs",
        """        self.profile.member(member).ok_or(RuntimeError::UnknownMember(member))?;
        self.member = Some(member);""",
        """        self.member = Some(member);""",
        # The property signs in only a member of the demo's team.
        "unit",
    ),
    (
        "runtime: events record the local calendar date, not the business date",
        "src/runtime.rs",
        """            business_date: self.profile.business_day().business_date_of(now)?,""",
        """            business_date: keel_types::BusinessDayPolicy::new(
                self.profile.business_day().time_zone(),
                0,
                0,
            )?
            .business_date_of(now)?,""",
        # The property's clock reads 9 am, when the two agree; tickets don't show dates.
        "unit",
    ),
    (
        "runtime: events record no correlation",
        "src/runtime.rs",
        """        correlation: Some(meta.correlation),""",
        """        correlation: None,""",
        # Tickets don't show correlations.
        "unit",
    ),
    (
        "runtime: an order started belongs to no one",
        "src/runtime.rs",
        """            owner: self.member,""",
        """            owner: None,""",
        # Tickets don't show owners.
        "unit",
    ),
    # Each intent answers from inside its write.
    (
        "runtime: an intent's ticket is made after its write",
        "src/runtime.rs",
        """            append(w, order.cast(), &event, meta)?;
            view(w, order, &payments, profile, locale)
        })""",
        """            append(w, order.cast(), &event, meta)
        })?;
        self.ticket(order)""",
        # The property's orders always price: only an order that overflows shows it.
        "unit",
    ),
    # Abandoning.
    (
        "runtime: an order is abandoned with its lines still on it",
        "src/runtime.rs",
        """            for line in live {
                let event = loaded.decide(location, device, OrderCommand::RemoveLine(line))?;
                append(w, order.cast(), &event, &meta)?;
                loaded = w.load(Order::new(order))?;
            }""",
        """            let _ = live;""",
    ),
    # Paying cash.
    (
        "runtime: a negative tender is taken",
        "src/runtime.rs",
        """        if tendered.is_negative() {
            return Err(RuntimeError::NegativeTender);
        }
""",
        "",
    ),
    (
        "runtime: a tender is short only of what is due before cash rounding",
        "src/runtime.rs",
        """            if tendered.compare(cash.due)?.is_lt() {""",
        """            if tendered.compare(due)?.is_lt() {""",
    ),
    (
        "runtime: change ignores cash rounding",
        "src/runtime.rs",
        """            let change = tendered.checked_sub(cash.due)?;""",
        """            let change = tendered.checked_sub(due)?;""",
    ),
    (
        "runtime: a cash payment captures what is due after rounding",
        "src/runtime.rs",
        """                let captured = PaymentCaptured {
                    amount: due,""",
        """                let captured = PaymentCaptured {
                    amount: cash.due,""",
    ),
    (
        "runtime: a cash payment records no rounding",
        "src/runtime.rs",
        """                        rounding: (!cash.rounding.is_zero()).then_some(cash.rounding),""",
        """                        rounding: None,""",
        # Caught when cash rounds down: the cash tendered is then less than the payment's amount.
    ),
    (
        "runtime: an order with nothing due starts a payment",
        "src/runtime.rs",
        """            if due.is_positive() {
                let started =""",
        """            if !due.is_negative() {
                let started =""",
        # The demo sells nothing for nothing, and checkout refuses an order with no lines either
        # way.
        "unit",
    ),
    # Tickets.
    (
        "runtime: a closed ticket lists its lines by identifier",
        "src/ticket.rs",
        """    let lines = order
        .lines()
        .iter()
        .filter_map(|line| {
            let charge = closed.lines.iter().find(|charge| charge.line == line.id())?;
            Some(ticket_line(line, charge.gross, locale))
        })
        .collect();""",
        """    let lines = closed
        .lines
        .iter()
        .filter_map(|charge| Some(ticket_line(order.line(charge.line)?, charge.gross, locale)))
        .collect();""",
    ),
    (
        "runtime: a paid ticket lists its taxes by identifier",
        "src/ticket.rs",
        """    charges.sort_by_key(|charge| {
        rules.taxes.iter().position(|tax| tax.id == charge.tax).unwrap_or(usize::MAX)
    });
""",
        "",
    ),
    (
        "runtime: a ticket's cash due ignores cash rounding",
        "src/ticket.rs",
        """    let cash = if due.is_positive() { cash_due(profile, due)?.due } else { due };""",
        """    let cash = due;""",
    ),
    (
        "runtime: a ticket lists taxes that taxed nothing",
        "src/ticket.rs",
        """        .filter(|tax| tax.taxable.is_positive())
        .map(|tax| TicketTax {""",
        """        .map(|tax| TicketTax {""",
    ),
    (
        "runtime: a ticket lists the lines taken off",
        "src/ticket.rs",
        """    let lines = order
        .live_lines()
        .zip(&totals.lines)""",
        """    let lines = order
        .lines()
        .iter()
        .zip(&totals.lines)""",
    ),
    (
        "runtime: a modifier chosen under another shows at its parent's depth",
        "src/views.rs",
        """        ticket_modifiers(&chosen.modifiers, depth.saturating_add(1), locale, out);""",
        """        ticket_modifiers(&chosen.modifiers, depth, locale, out);""",
    ),
    (
        "runtime: open orders include those closed",
        "src/runtime.rs",
        """            .orders(OrderState::Active)?
            .into_iter()""",
        """            .orders(OrderState::Active)?
            .into_iter()
            .chain(self.store.orders(OrderState::Closed)?)""",
    ),
    # The menu and the item view.
    (
        "runtime: the menu says modifiers must be chosen wherever they are offered",
        "src/runtime.rs",
        """                            required: groups().any(|group| group.min > 0),""",
        """                            required: groups().next().is_some(),""",
        # The property drives intents, not the menu.
        "unit",
    ),
    (
        "runtime: the menu names a variant by its item alone",
        "src/runtime.rs",
        """                            name: profile.variant_name(variant)?.clone(),""",
        """                            name: item.name.clone(),""",
        "unit",
    ),
    (
        "runtime: the item view leaves out the groups under a modifier",
        "src/views.rs",
        """                    groups: group_views(profile, &modifier.groups, locale),""",
        """                    groups: Vec::new(),""",
        "unit",
    ),
    # Print jobs (ADR-0024).
    (
        "print: an unconfirmed job is tried again",
        "src/runtime.rs",
        "            Outcome::Unconfirmed(_) => {}",
        """            Outcome::Unconfirmed(_) => {
                let due = self.clock.now();
                self.store.write(|w| w.retry(&key, due))?;
            }""",
    ),
    (
        "print: a job running when the runtime opens is handed to the driver",
        "src/runtime.rs",
        "            .filter(|(_, queued)| queued.state == keel_store::EffectState::Pending)",
        "            .filter(|(_, queued)| queued.state != keel_store::EffectState::Done)",
    ),
    (
        "print: a job not sent waits as long each time",
        "src/print.rs",
        "    let wait = 2_u64.checked_shl(doublings).unwrap_or(60);",
        "    let wait = 2_u64.checked_shl(doublings.min(0)).unwrap_or(60);",
    ),
    (
        "print: a job not sent waits past a minute",
        "src/print.rs",
        "    Duration::from_secs(wait).min(MAX_WAIT)",
        "    Duration::from_secs(wait).min(MAX_WAIT.saturating_mul(2))",
        # A job fails to be sent six times in a row in few runs; the known answer counts them.
        "unit",
    ),
    (
        "print: jobs go to the driver two at a time",
        "src/runtime.rs",
        """        if self.sending.is_some() {
            return Ok(None);
        }
        let now = self.clock.now();""",
        """        let now = self.clock.now();""",
    ),
    (
        "print: any job's outcome is taken",
        "src/runtime.rs",
        "        if self.sending != Some(job) {\n            return Err(RuntimeError::UnknownJob(job));",
        "        if self.sending.is_none() {\n            return Err(RuntimeError::UnknownJob(job));",
    ),
    (
        "print: the job with the driver can be resolved",
        "src/runtime.rs",
        "        if queued.state != keel_store::EffectState::Running || self.sending == Some(job) {",
        "        if queued.state != keel_store::EffectState::Running {",
    ),
    (
        "print: a sale that owes nothing opens the drawer",
        "src/runtime.rs",
        """            if due.is_positive() {
                let kick = Doc::new(vec![Block::DrawerKick])?;""",
        """            if !due.is_negative() {
                let kick = Doc::new(vec![Block::DrawerKick])?;""",
    ),
    (
        "print: printing a job again leaves it unconfirmed",
        "src/runtime.rs",
        """                    w.fail(&key)?;
                    w.enqueue(&effect, now)?;""",
        """                    w.enqueue(&effect, now)?;""",
    ),
    (
        "print: a reprint printed again is marked twice",
        "src/runtime.rs",
        """        if is_reprint(doc) {
            return Ok(doc.clone());
        }""",
        """        if is_reprint(doc) && doc.blocks().is_empty() {
            return Ok(doc.clone());
        }""",
        # The property doesn't read documents; the known answers do.
        "unit",
    ),
    (
        "print: a receipt shows no change",
        "src/receipt.rs",
        """        if let Some(change) = captured.change() {
            blocks.push(Block::row(words.change, money(change)));""",
        """        if let Some(change) = captured.change().filter(|_| false) {
            blocks.push(Block::row(words.change, money(change)));""",
        # As above.
        "unit",
    ),
    # Found in review.
    (
        "print: jobs the clock left behind wait for it",
        "src/runtime.rs",
        """            if queued.due > now {
                self.store.write(|w| w.reschedule(&key, now))?;
            }""",
        "",
    ),
    (
        "print: a receipt's lines aren't capped by blocks",
        "src/receipt.rs",
        "            || lines.len().saturating_add(these.len()) > MAX_LINES_BLOCKS",
        "            || lines.len().saturating_add(these.len()) > usize::MAX",
        "unit",
    ),
    (
        "print: a receipt too big for the outbox fails the sale",
        "src/runtime.rs",
        "                .filter(|doc| doc.encode().len() <= keel_store::MAX_PAYLOAD)",
        "                .filter(|doc| !doc.blocks().is_empty())",
        "unit",
    ),
    (
        "print: a reprint is recognised only in English",
        "src/receipt.rs",
        "    [ENGLISH.reprint, SPANISH.reprint]",
        "    [ENGLISH.reprint]",
        "unit",
    ),
    (
        "print: jobs share process IDs",
        "src/print.rs",
        "    u64::from_be_bytes(tail).wrapping_add(u64::from(attempts))",
        "    u64::from(attempts).wrapping_add(u64::from(tail.len() > 99))",
        "unit",
    ),
    # Replication (ADR-0026).
    (
        "sync: a device writes before its log is settled",
        "src/runtime.rs",
        "        if self.sync.as_ref().is_some_and(|sync| !sync.may_write(self.clock.now())) {",
        "        if self.sync.as_ref().is_some_and(|sync| !sync.may_write(self.clock.now()) && false) {",
        "unit",
    ),
    (
        "sync: an intent's events wait for the next round",
        "src/sync.rs",
        "            let events = self.store.log(device, pushed, PUSH)?;",
        "            let events = self.store.log(device, u64::MAX, PUSH)?;",
        "unit",
    ),
    (
        "sync: a roster naming another key for the device is taken",
        "src/sync.rs",
        "        if roster.data().location != self.profile.data().location || me.key != self.key {",
        "        if roster.data().location != self.profile.data().location {",
        "unit",
    ),
    # Found in the slice's review.
    (
        "sync: a tick a set-back clock put off is waited for",
        "src/sync.rs",
        "            .is_some_and(|at| at <= now || at.as_millis().saturating_sub(now.as_millis()) > longest)",
        "            .is_some_and(|at| at <= now && longest > 0)",
        "unit",
    ),
    (
        "sync: a device that hears no one never writes",
        "src/sync.rs",
        "        self.replicator.settled() || waited >= island || waited < 0",
        "        self.replicator.settled() || waited < 0",
        "unit",
    ),
    (
        "sync: a numbering that fails isn't tried again",
        "../keel-sync/src/replicator.rs",
        "            if self.serving() && (!was_serving || self.owed) {",
        "            if self.serving() && !was_serving {",
        "unit",
    ),
    (
        "sync: a hub the roster demotes goes on serving",
        "../keel-sync/src/replicator.rs",
        "        self.settled && !self.forked() && self.is_hub() && self.roles.hub.is_some()",
        "        self.settled && !self.forked() && self.is_hub()",
        "unit",
    ),
    (
        "runtime: today's orders are every day's",
        "src/runtime.rs",
        ".filter(|order| order.business_date == Some(today))",
        ".filter(|order| order.business_date.is_some())",
        "unit",
    ),
    (
        "runtime: today's orders leave out the closed",
        "src/runtime.rs",
        "[OrderState::Active, OrderState::Closed, OrderState::Voided, OrderState::Abandoned]",
        "[OrderState::Active, OrderState::Voided, OrderState::Abandoned]",
        "unit",
    ),
    (
        "runtime: a cash offer is the multiple at or below what is due",
        "src/ticket.rs",
        ".and_then(|unit| due.checked_div(unit)?.checked_add(1)?.checked_mul(unit));",
        ".and_then(|unit| due.checked_div(unit)?.checked_add(0)?.checked_mul(unit));",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: one cash offer too many",
        "src/ticket.rs",
        "            && offers.len() < crate::CASH_OFFERS",
        "            && offers.len() <= crate::CASH_OFFERS",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: an order that owes nothing is offered more",
        "src/ticket.rs",
        "    if due <= 0 {\n        return offers;",
        "    if due < 0 {\n        return offers;",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: a cash offer may repeat",
        "src/ticket.rs",
        "            && !offers.contains(&above)\n",
        "",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: a currency without decimals is offered single units",
        "src/ticket.rs",
        "    let major = if minor_units == 0 {\n        100\n    } else {",
        "    let major = if minor_units == 0 {\n        1\n    } else {",
        "unit",
    ),
    # The cloud (ADR-0028).
    (
        "runtime: a hub names no durable peer, so no device hears it is backed up",
        "src/sync.rs",
        "            .filter(|cloud| hub.is_some() && *cloud != device);",
        "            .filter(|_| false);",
    ),
    (
        "runtime: a device's backed-up mark is its own head",
        "src/sync.rs",
        "            backed_up: sync.replicator.durable().get(&self.device).copied().unwrap_or(0),",
        "            backed_up: self.store.head(self.device)?.seq(),",
    ),
    # Catalog publishing (ADR-0028).
    (
        "runtime: any device publishes",
        "src/publication.rs",
        "        if sync.roster.cloud().is_none_or(|cloud| cloud.device != self.device) {\n            return Err(RuntimeError::NotCloud);",
        "        if sync.roster.cloud().is_none() {\n            return Err(RuntimeError::NotCloud);",
    ),
    (
        "runtime: a publication of another location is taken",
        "src/publication.rs",
        "            .filter(|profile| profile.data().location == self.profile.data().location))",
        "            )",
    ),
    (
        "runtime: the earliest publication is taken, not the latest",
        "src/publication.rs",
        "        for (stream, published) in found.iter().rev() {",
        "        for (stream, published) in found.iter() {",
    ),
    (
        "runtime: a member the new team doesn't have stays signed in",
        "src/publication.rs",
        "            self.member = None;\n",
        "",
    ),
    (
        "runtime: a publication is looked for at no frame",
        "src/sync.rs",
        "        // look began; the frame itself is stored, and its answers go out.\n        let _ = self.take_publications();",
        "        // look began; the frame itself is stored, and its answers go out.",
    ),
    (
        "runtime: a device restarted opens with the provisioned profile",
        "src/sync.rs",
        "        // first frames go out.\n        let _ = self.take_publications();",
        "        // first frames go out.",
    ),
    (
        "runtime: a device that publishes doesn't take its own",
        "src/publication.rs",
        "        })?;\n        self.take_publications()?;",
        "        })?;",
    ),
    # Found in step 8's review.
    (
        "runtime: the cloud publishes before its log is settled",
        "src/publication.rs",
        "        if !sync.may_write(self.clock.now()) {",
        "        if !sync.may_write(self.clock.now()) && false {",
    ),
    (
        "runtime: a device takes no publication before it joins its network",
        "src/publication.rs",
        "        self.watch_cloud(roster.cloud().map(|cloud| cloud.device));\n        self.take_publications()",
        "        self.watch_cloud(roster.cloud().map(|cloud| cloud.device));\n        Ok(false)",
    ),
    (
        "runtime: another cloud's log is looked at from where the last one's was",
        "src/publication.rs",
        "            self.published = Watch { cloud, looked: 0 };",
        "            self.published = Watch { cloud, looked: self.published.looked };",
    ),
]
