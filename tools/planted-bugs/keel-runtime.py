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
        "src/staff.rs",
        """        if self.profile.member(member).is_none() {
            return Err(RuntimeError::UnknownMember(member));
        }""",
        "",
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
        """                append(w, order.cast(), &event, meta)?;
            }
            view(w, order, &payments, profile, locale)
        })""",
        """                append(w, order.cast(), &event, meta)?;
            }
            Ok::<(), RuntimeError>(())
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
                    amount: applied,""",
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
        "            if applied.is_positive() || part.is_some() {",
        "            if !applied.is_negative() || part.is_some() {",
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
            let (_, gross, taken) =
                charges.lines.iter().find(|(known, _, _)| *known == line.id())?;""",
        """    let lines = charges
        .lines
        .iter()
        .filter_map(|(id, gross, taken)| {
            let line = order.line(*id)?;""",
    ),
    (
        "runtime: a paid ticket lists its taxes by identifier",
        "src/ticket.rs",
        """    taxes.sort_by_key(|(tax, _)| {
        rules.taxes.iter().position(|known| known.id == *tax).unwrap_or(usize::MAX)
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
        """            .filter(|tax| tax.taxable.is_positive())
            .map(|tax| (tax.id, tax.tax))""",
        """            .map(|tax| (tax.id, tax.tax))""",
    ),
    (
        "runtime: a ticket lists the lines taken off",
        "src/ticket.rs",
        "    let on: Vec<Id<Line>> = order.lines_on(check.id()).map(Line::id).collect();",
        "    let on: Vec<Id<Line>> = order.lines().iter().map(Line::id).collect();",
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
        """            let mut paid = payments.clone();
            if applied.is_positive() || part.is_some() {""",
        """            let mut paid = payments.clone();
            if applied.is_zero() {
                let kick = Doc::new(vec![Block::DrawerKick])?;
                w.enqueue(&print::effect(drawer, JobKind::Drawer, &kick, None)?, meta.now)?;
            }
            if applied.is_positive() || part.is_some() {""",
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
        "        .filter(|doc| doc.encode().len() <= keel_store::MAX_PAYLOAD)",
        "        .filter(|doc| !doc.blocks().is_empty())",
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
    # Staff, roles and approvals (ADR-0029).
    (
        "runtime: PIN entry is never locked",
        "src/staff.rs",
        "        if let Some(seconds) = self.pins.locked(now) {",
        "        if let Some(seconds) = self.pins.locked(now).filter(|_| false) {",
    ),
    (
        "runtime: a right PIN starts the count of wrong ones again, whoever's it is",
        "src/staff.rs",
        "        if self.profile.verifies_pin(member, pin) {\n            Ok(())",
        "        if self.profile.verifies_pin(member, pin) {\n            self.pins = PinGuard::default();\n            Ok(())",
    ),
    (
        "runtime: every lock is as short as the first",
        "src/staff.rs",
        "            let lock = FIRST_LOCK.saturating_mul(1 << doublings).min(LONGEST_LOCK);",
        "            let lock = FIRST_LOCK;",
    ),
    (
        "runtime: a clock set back makes a lock longer than the longest",
        "src/staff.rs",
        "        let left = left.min(millis(LONGEST_LOCK));",
        "",
    ),
    (
        "runtime: an approval never expires",
        "src/staff.rs",
        "        (pending.asked == *asked && (0..=millis(APPROVAL_LIFE)).contains(&age))",
        "        (pending.asked == *asked && (0..=i64::MAX).contains(&age))",
    ),
    (
        "runtime: an approval outlives a sign-in",
        "src/staff.rs",
        "        self.member = Some(member);\n        self.pending = None;",
        "        self.member = Some(member);",
    ),
    (
        "runtime: an approval is never spent",
        "src/staff.rs",
        "        if granted.is_some() {\n            self.pending = None;",
        "        if granted.is_some() {",
    ),
    (
        "runtime: anyone's approval approves anything",
        "src/staff.rs",
        "        if !self.profile.may_approve(approver, member, &request) {",
        "        if false {",
    ),
    (
        "runtime: the approved events don't name their approval",
        "src/staff.rs",
        "        meta.approval = Some(approval);\n",
        "        let _ = approval;\n",
    ),
    (
        "runtime: dropping an empty order needs an approval",
        "src/runtime.rs",
        "        let granted = if loaded.lines().is_empty() {",
        "        let granted = if false {",
    ),
    (
        "runtime: anyone reprints, signed in or not",
        "src/runtime.rs",
        "        let granted = self.gate(Request::of(Action::ReceiptReprint), Some(order.cast()))?;",
        "        let granted: Option<Granted> = None;",
    ),
    # Found in step 1's review.
    (
        "runtime: quiet never starts the count of wrong PINs again",
        "src/staff.rs",
        "        if quiet {\n            self.wrong = 0;",
        "        if quiet && false {\n            self.wrong = 0;",
    ),
    (
        "runtime: an approval covers any request of the one who asked",
        "src/staff.rs",
        "        (pending.asked == *asked && (0..=millis(APPROVAL_LIFE)).contains(&age))",
        "        (pending.asked.actor == asked.actor && (0..=millis(APPROVAL_LIFE)).contains(&age))",
    ),
    (
        "runtime: an approver is taken before they are known to be one",
        "src/staff.rs",
        "        if !self.profile.may_approve(approver, actor, &asked.request) {",
        "        if false {",
    ),
    (
        "runtime: an order emptied line by line is dropped with no approval",
        "src/runtime.rs",
        "        let granted = if loaded.lines().is_empty() {",
        "        let granted = if loaded.live_lines().next().is_none() {",
    ),
    (
        "runtime: fewer of a line needs no approval",
        "src/runtime.rs",
        """        let request = if fewer {
            Some(Request::of(Action::LineRemove))""",
        """        let request = if false {
            Some(Request::of(Action::LineRemove))""",
    ),
    (
        "runtime: an approval is asked for a reprint that can't be",
        "src/runtime.rs",
        "        // Someone is signed in first: whether a receipt exists is for them to learn.\n        self.member.ok_or(RuntimeError::NotSignedIn)?;",
        "        self.gate(Request::of(Action::ReceiptReprint), Some(order.cast()))?;",
    ),
    (
        "runtime: anyone but the one who asked is an approver",
        "src/staff.rs",
        "            .filter(|&member| self.profile.may_approve(member, asked.actor, &asked.request))",
        "            .filter(|&member| member != asked.actor)",
    ),
    (
        "runtime: a member whose PIN or role changed stays signed in",
        "src/publication.rs",
        "                (Some(before), Some(after)) => before.role != after.role || before.pin != after.pin,",
        "                (Some(_), Some(_)) => false,",
    ),
    # Discounts (ADR-0030): the gate.
    (
        "discount: the gate is asked for no amount",
        "src/runtime.rs",
        """            amount: Some(effect.total),
            percent: Some(effect.percent),
        };
        let granted""",
        """            amount: None,
            percent: Some(effect.percent),
        };
        let granted""",
    ),
    (
        "discount: the gate is asked for no percentage",
        "src/runtime.rs",
        """            percent: Some(effect.percent),
        };
        let granted""",
        """            percent: None,
        };
        let granted""",
    ),
    (
        "discount: a discount isn't gated",
        "src/runtime.rs",
        """            percent: Some(effect.percent),
        };
        let granted = self.gate(request, Some(order.cast()))?;""",
        """            percent: Some(effect.percent),
        };
        let granted: Option<Granted> = None;
        let _ = request;""",
    ),
    (
        "discount: a discount the order refuses asks for an approval first",
        "src/runtime.rs",
        """        let command = OrderCommand::ApplyDiscount(applied.clone());
        loaded.decide(self.location(), self.device, command.clone())?;""",
        """        let command = OrderCommand::ApplyDiscount(applied.clone());""",
    ),
    (
        "discount: a receipt leaves out a line's discounts",
        "src/receipt.rs",
        "        for discount in &line.discounts {",
        "        for discount in line.discounts.iter().take(0) {",
    ),
    (
        "discount: a receipt leaves out the whole order's discounts",
        "src/receipt.rs",
        "    for discount in &ticket.discounts {",
        "    for discount in ticket.discounts.iter().take(0) {",
    ),
    (
        "reprice: the gate is asked for the unit difference, not the line's",
        "src/runtime.rs",
        "        let difference = at(price)?.checked_sub(at(rung)?)?;",
        "        let difference = price.checked_sub(rung)?;",
    ),
    (
        "reprice: a price set by hand isn't gated",
        "src/runtime.rs",
        """            Request { action: Action::PriceOverride, amount: Some(amount), percent: None };
        let granted = self.gate(request, Some(order.cast()))?;""",
        """            Request { action: Action::PriceOverride, amount: Some(amount), percent: None };
        let _ = request;
        let granted: Option<Granted> = None;""",
    ),
    (
        "reprice: a price the order refuses asks for an approval first",
        "src/runtime.rs",
        """        let command = OrderCommand::RepriceLine(repriced);
        // The order's own rules come before an approval is asked for one it would refuse.
        loaded.decide(self.location(), self.device, command.clone())?;""",
        """        let command = OrderCommand::RepriceLine(repriced);""",
    ),
    (
        "notes: a receipt leaves out a line's notes",
        "src/receipt.rs",
        "        if let Some(notes) = &line.notes {",
        "        if let Some(notes) = line.notes.as_ref().filter(|_| false) {",
    ),
    # Split checks and partial payments (ADR-0030).
    (
        "split: an order's ticket leaves out its checks' taxes",
        "src/ticket.rs",
        "        for (tax, amount) in other.taxes {",
        "        for (tax, amount) in other.taxes.into_iter().take(0) {",
    ),
    (
        "split: an order's ticket counts one check's amounts",
        "src/ticket.rs",
        "        self.total = self.total.checked_add(other.total)?;",
        "        self.total = other.total;",
    ),
    (
        "split: a part pays all the check owes",
        "src/runtime.rs",
        "                Some(part) => (part, Cash { due: part, rounding: Money::zero(currency) }),",
        "                Some(_) => (due, cash_due(profile, due)?),",
    ),
    (
        "split: the order closes with its first check",
        "src/runtime.rs",
        "            .all(|check| !check.is_open() || order.lines_on(check.id()).next().is_none())",
        "            .any(|check| !check.is_open() || order.lines_on(check.id()).next().is_none())",
    ),
    (
        "split: no checks are opened for it",
        "src/runtime.rs",
        "        let more = usize::from(ways).saturating_sub(open.len());",
        "        let more = 0_usize;",
    ),
    (
        "split: a check's receipt shows all the order's payments",
        "src/runtime.rs",
        "        .filter(|payment| payment.info().is_some_and(|info| info.check == check))",
        "        .filter(|payment| payment.info().is_some())",
    ),
    (
        "split: a check's receipt doesn't say which check",
        "src/runtime.rs",
        "        check: number,",
        "        check: None,",
    ),
    (
        "split: a part closes the check",
        "src/runtime.rs",
        "            if checkout.balance(check)?.due.is_positive() {",
        "            if false {",
    ),
    # Park and recall (ADR-0030).
    (
        "recall: an order asked for already is asked for again",
        "src/runtime.rs",
        "        if ours || asked {",
        "        if ours {",
    ),
    (
        "recall: an order owned here is asked for",
        "src/runtime.rs",
        "        let ours = loaded.ownership().is_some_and(|ownership| ownership.device == self.device);",
        "        let ours = false;",
    ),
    (
        "recall: taking an order over isn't gated",
        "src/runtime.rs",
        """        let granted = self.gate(Request::of(Action::OrderVoid), Some(order.cast()))?;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(order, command, &meta, granted.as_ref(), stream)?;""",
        """        let granted: Option<Granted> = None;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(order, command, &meta, granted.as_ref(), stream)?;""",
    ),
    (
        "recall: a ticket names no owner",
        "src/ticket.rs",
        "        owner: order.ownership().map(|ownership| ownership.device),",
        "        owner: None,",
    ),
    # Found in step 2's review (ADR-0030).
    (
        "review: an amount larger than its line is accepted",
        "src/runtime.rs",
        "            && effect.taken != amount",
        "            && false",
    ),
    (
        "review: a partly paid check takes a discount",
        "src/runtime.rs",
        "        self.check_unpaid(&loaded, touched(&loaded, applied.line))?;",
        "",
    ),
    (
        "review: a discount is asked for as it takes alone",
        "src/runtime.rs",
        """            amount: Some(effect.total),
            percent: Some(effect.percent),
        };
        let granted = self.gate(request, Some(order.cast()))?;""",
        """            amount: Some(effect.taken),
            percent: Some(effect.percent),
        };
        let granted = self.gate(request, Some(order.cast()))?;""",
    ),
    (
        "review: a price is asked for from the last one set",
        "src/runtime.rs",
        "        let amount = self.override_amount(unit_price, now.item().unit_price, now.quantity())?;",
        "        let amount = self.override_amount(unit_price, now.unit_price(), now.quantity())?;",
    ),
    (
        "review: more of a discounted line isn't asked for",
        "src/runtime.rs",
        "        } else if more && (!now.discounts().is_empty() || !loaded.discounts().is_empty()) {",
        "        } else if false {",
    ),
    (
        "review: more of a line priced by hand isn't asked for",
        "src/runtime.rs",
        "        } else if more && now.repriced().is_some() {",
        "        } else if false {",
    ),
    (
        "review: an emptied split order stays open",
        "src/runtime.rs",
        """            if finished(&loaded) {
                let event = loaded.decide(location, device, OrderCommand::Close)?;
                append(w, order.cast(), &event, meta)?;""",
        """            if false {
                let event = loaded.decide(location, device, OrderCommand::Close)?;
                append(w, order.cast(), &event, meta)?;""",
    ),
    (
        "review: pay_cash pays the main check",
        "src/runtime.rs",
        "            [only] => *only,",
        "            [_] => order.cast(),",
    ),
    (
        "review: a ticket lists no lone check but the main",
        "src/ticket.rs",
        "    if checks.len() > 1 || checks.iter().any(|known| known.check != main) {",
        "    if checks.len() > 1 || main != main {",
    ),
    (
        "review: a split order's reprint is its last check's alone",
        "src/runtime.rs",
        "            originals.retain(|(_, known)| *known != cause);",
        "            originals.clear();",
    ),
    (
        "review: a smaller line on a partly paid check is allowed",
        "src/runtime.rs",
        """        if fewer {
            self.check_unpaid(&loaded, now.allocation().iter().map(|share| share.check))?;
        }""",
        "",
    ),
    # Drawers and day reports (ADR-0031).
    (
        "drawer: cash goes into no drawer",
        "src/runtime.rs",
        "                if let (Some(open), PaymentEvent::Captured(captured)) = (drawer_session, &event)",
        "                if let (Some(open), PaymentEvent::Captured(captured)) = (None::<Id<Drawer>>, &event)",
    ),
    (
        "drawer: the drawer counts the amount, not what the customer paid",
        "src/runtime.rs",
        "                    && let Some(amount) = captured.received().filter(|amount| amount.is_positive())",
        "                    && let Some(amount) = Some(captured.amount).filter(|amount| amount.is_positive())",
    ),
    (
        "drawer: a second drawer opens",
        "src/drawer.rs",
        """        if self.open_drawer_id()?.is_some() {
            return Err(RuntimeError::DrawerOpen);
        }""",
        "",
    ),
    (
        "drawer: a paid-out isn't gated",
        "src/drawer.rs",
        "        let granted = if kind == Movement::PaidOut {",
        "        let granted = if false {",
    ),
    (
        "drawer: a no-sale isn't gated",
        "src/drawer.rs",
        "        let granted = self.gate(Request::of(Action::DrawerNoSale), Some(open.cast()))?;",
        "        let granted = None;",
    ),
    (
        "drawer: a variance is asked for as nothing",
        "src/drawer.rs",
        "        let granted = if variance.is_zero() {",
        "        let granted = if true {",
    ),
    (
        "drawer: a short close is asked for as its sign",
        "src/drawer.rs",
        "            let size = if variance.is_negative() { variance.checked_neg()? } else { variance };",
        "            let size = variance;",
    ),
    (
        "drawer: the open drawer shows what it should hold",
        "src/views.rs",
        """        if let Some(closed) = session.closed()
            && let Some(variance) = closed.variance()""",
        """        if let Some(closed) = session.closed().or(Some(&keel_domain::drawer::DrawerClosed { counted: sums.taken, expected: sums.taken }))
            && let Some(variance) = closed.variance()""",
    ),
    (
        "report: a Z prints with a drawer open",
        "src/report.rs",
        "        if kind == ReportKind::Z",
        "        if kind == ReportKind::Z && false",
    ),
    (
        "report: discounts on the whole order aren't counted",
        "src/report.rs",
        """                for discount in &ticket.discounts {
                    discounts = discounts.checked_add(discount.amount.money)?;
                }""",
        "",
    ),
    (
        "report: cash a drawer counted is reported as with no drawer",
        "src/report.rs",
        "            if !counted.contains(&payment) {",
        "            if true {",
    ),
    (
        "report: a dropped order with lines isn't voided",
        "src/report.rs",
        "            if order.state == OrderState::Voided || !loaded.lines().is_empty() {",
        "            if order.state == OrderState::Voided {",
    ),
    (
        "report: another day's orders are counted",
        "src/report.rs",
        """                if order.business_date != Some(date) {
                    continue;
                }
                let ticket = self.ticket(order.id)?;""",
        """                let ticket = self.ticket(order.id)?;""",
    ),
]
